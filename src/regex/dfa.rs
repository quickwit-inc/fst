use std::collections::{HashMap, HashSet};
use std::fmt;
use std::ops::RangeInclusive;

use super::sparse::SparseSet;
use super::Error;
use super::Inst;

/// With deduplicate_insts, this is quite unlikely to be hit.
const STATE_LIMIT: usize = 1_000; // currently at least 2MB >_<

/// Deduplicates NFA instructions by finding equivalent branches and merging them into a single
/// branch.
///
/// Two instructions are duplicates when they perform the same `Inst` step and reach
/// duplicate targets, with `Jump(target)` treated as its target.
///
/// Minimal Example (`101|121`):
/// ```text
/// Before:
///   Split -+-> '1' -> '0' -> '1' -----+
///          |                          v
///          +-> '1' -> '2' -> '1' -> Match
///
/// After:
///   Split -+-> '1' -> '0' ----+
///          |                  v
///          +-> '1' -> '2' -> '1' -> Match
/// ```
///
/// This is a simple example, but state counts grow fast with `.*`. For example,
/// `.*a.*|.*b.*` has two separate `.*` loops that can be merged into one.
/// `.*` matches only valid UTF-8 sequences (excluding `\n`), so each `.`
/// expands into multiple UTF-8 byte branches.
///
/// Without this optimization `DfaBuilder` may exceed `STATE_LIMIT`.
///
/// Note: We have a single common `Inst::Match` at the last entry of `insts`.
/// `Compiler::compile` (`src/regex/compile.rs`) appends this.
fn deduplicate_insts(insts: &mut [Inst]) {
    type Ip = usize;

    #[derive(Hash, Eq, PartialEq)]
    enum DedupKey {
        Match,
        ByteRange {
            // Keeps previously split instructions from re-merging so the loop
            // is guaranteed to terminate.
            current_replacement_ip: Option<Ip>,
            match_bytes: RangeInclusive<u8>,
            target: Option<Ip>,
        },
        Or {
            // Same as above: prevents re-merging to guarantee termination.
            current_replacement_ip: Option<Ip>,
            first_target: Option<Ip>,
            second_target: Option<Ip>,
        },
    }

    // For each `ip`, stores the `ip` that replaces it (`None` until visited, so
    // identical loops stay merged unless a later pass splits them).
    let mut replacement_ips: Vec<Option<Ip>> = vec![None; insts.len()];
    let mut key_to_ip: HashMap<DedupKey, Ip> = HashMap::with_capacity(insts.len());

    // Scan backwards from the trailing `Match` until a pass makes no changes.
    // Every entry in `replacement_ips` can only move downward, so this loop
    // always terminates.
    loop {
        key_to_ip.clear();
        let mut changed = false;

        for (ip, inst) in insts.iter().enumerate().rev() {
            let current_replacement_ip = replacement_ips[ip];
            let dedup_key = match *inst {
                // A `Jump` consumes no input and aliases `target_ip`, so we can just copy the
                // Ip of the `Jump`'s target.
                //
                // Example (`101|121`):
                // 101 ends with `1 -> Jump(Match)`
                // 121 ends with `1 -> Match`.
                // Rewriting the ip to both point directly to Match allows the two branches to merge one
                // instruction earlier.
                Inst::Jump(target_ip) => {
                    let target = replacement_ips[target_ip];
                    if current_replacement_ip != target {
                        replacement_ips[ip] = target;
                        changed = true;
                    }
                    continue;
                }
                Inst::Match => DedupKey::Match,
                // `Range` falls through to `ip + 1` (`Compiler` always ends with `Match`).
                Inst::Range(start, end) => DedupKey::ByteRange {
                    current_replacement_ip,
                    match_bytes: start..=end,
                    target: replacement_ips[ip + 1],
                },
                Inst::Split(first_ip, second_ip) => DedupKey::Or {
                    current_replacement_ip,
                    first_target: replacement_ips[first_ip],
                    second_target: replacement_ips[second_ip],
                },
            };

            // Instructions with the same `dedup_key` get the first `ip` seen for it.
            let new_replacement_ip = Some(*key_to_ip.entry(dedup_key).or_insert(ip));
            if current_replacement_ip != new_replacement_ip {
                replacement_ips[ip] = new_replacement_ip;
                changed = true;
            }
        }

        if !changed {
            break;
        }
    }

    // Rewrite instructions to `Jump` their replacement `ip`s.
    for (ip, inst) in insts.iter_mut().enumerate() {
        // In practice this will always be `Some` because we scan backwards and the last
        // instruction is always `Match`.
        if let Some(replacement_ip) = replacement_ips[ip] {
            if ip != replacement_ip {
                *inst = Inst::Jump(replacement_ip);
            }
        }
    }
}

pub struct DfaBuilder {
    dfa: Dfa,
    cache: HashMap<Vec<usize>, usize>,
}

pub struct Dfa {
    insts: Vec<Inst>,
    states: Vec<State>,
}

struct State {
    insts: Vec<usize>,
    next: [Option<usize>; 256],
    is_match: bool,
}

impl DfaBuilder {
    pub fn new(mut insts: Vec<Inst>) -> Self {
        deduplicate_insts(&mut insts);
        DfaBuilder {
            dfa: Dfa {
                insts,
                states: Vec::with_capacity(16),
            },
            cache: HashMap::with_capacity(1024),
        }
    }

    pub fn build(mut self) -> Result<Dfa, Error> {
        let mut cur = SparseSet::new(self.dfa.insts.len());
        let mut next = SparseSet::new(self.dfa.insts.len());

        self.dfa.add(&mut cur, 0);
        let mut states = vec![self.cached_state(&cur).unwrap()];
        let mut seen = HashSet::new();
        while let Some(s) = states.pop() {
            for b in 0..256 {
                let ns = self.run_state(&mut cur, &mut next, s, b as u8);
                if let Some(ns) = ns {
                    if !seen.contains(&ns) {
                        seen.insert(ns);
                        states.push(ns);
                    }
                }
                if self.dfa.states.len() > STATE_LIMIT {
                    return Err(Error::TooManyStates(STATE_LIMIT));
                }
            }
        }
        Ok(self.dfa)
    }

    fn run_state(
        &mut self,
        cur: &mut SparseSet,
        next: &mut SparseSet,
        state: usize,
        byte: u8,
    ) -> Option<usize> {
        cur.clear();
        for &ip in &self.dfa.states[state].insts {
            cur.add(ip);
        }
        self.dfa.run(cur, next, byte);
        let next_state = self.cached_state(next);
        self.dfa.states[state].next[byte as usize] = next_state;
        next_state
    }

    fn cached_state(&mut self, set: &SparseSet) -> Option<usize> {
        use super::Inst::*;
        use std::collections::hash_map::Entry;

        // There are probably many ways to optimize this routine. ---AG

        let mut insts = vec![];
        let mut is_match = false;
        for i in 0..set.len() {
            let ip = set.get(i);
            match self.dfa.insts[ip] {
                Jump(_) | Split(_, _) => {}
                Range(_, _) => insts.push(ip),
                Match => {
                    is_match = true;
                    insts.push(ip);
                }
            }
        }
        if insts.is_empty() {
            return None;
        }
        insts.sort_unstable();
        Some(match self.cache.entry(insts.clone()) {
            Entry::Occupied(v) => *v.get(),
            Entry::Vacant(v) => {
                self.dfa.states.push(State {
                    insts,
                    next: [None; 256],
                    is_match,
                });
                *v.insert(self.dfa.states.len() - 1)
            }
        })
    }
}

impl Dfa {
    pub fn is_match(&self, si: usize) -> bool {
        self.states[si].is_match
    }

    pub fn accept(&self, si: usize, byte: u8) -> Option<usize> {
        self.states[si].next[byte as usize]
    }

    #[cfg(test)]
    pub fn num_states(&self) -> usize {
        self.states.len()
    }

    fn add(&self, set: &mut SparseSet, ip: usize) {
        use super::Inst::*;

        if set.contains(ip) {
            return;
        }
        set.add(ip);
        match self.insts[ip] {
            Match | Range(_, _) => {}
            Jump(ip) => self.add(set, ip),
            Split(ip1, ip2) => {
                self.add(set, ip1);
                self.add(set, ip2);
            }
        }
    }

    fn run(&self, from: &SparseSet, to: &mut SparseSet, byte: u8) -> bool {
        use super::Inst::*;
        to.clear();
        let mut is_match = false;
        for i in 0..from.len() {
            let ip = from.get(i);
            match self.insts[ip] {
                Jump(_) | Split(_, _) => {}
                Match => is_match = true,
                Range(s, e) => {
                    if s <= byte && byte <= e {
                        self.add(to, ip + 1);
                    }
                }
            }
        }
        is_match
    }
}

impl fmt::Debug for Dfa {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for (i, inst) in self.insts.iter().enumerate() {
            writeln!(f, "{:03} {:?}", i, inst)?;
        }
        writeln!(f, "------------")?;
        for (i, state) in self.states.iter().enumerate() {
            if state.is_match {
                writeln!(f, "{:03}* {:?}", i, state.insts)?;
            } else {
                writeln!(f, "{:03}  {:?}", i, state.insts)?;
            }
            for j in 0..256 {
                if let Some(si) = state.next[j] {
                    writeln!(f, "{:03}   {:X} => {}", i, j, si)?;
                }
            }
        }
        Ok(())
    }
}
