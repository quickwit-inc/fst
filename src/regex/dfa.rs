use std::collections::HashMap;
use std::fmt;

use super::compile::Nfa;
use super::sparse::SparseSet;
use super::Error;
use super::Inst;

const STATE_LIMIT: usize = 1_000;
const NO_TRANSITION: u16 = u16::MAX;

/// Partitions `0..=255` into contiguous byte ranges split at every
/// `Inst::Range` boundary in the NFA.
///
/// Every byte inside a single range triggers the exact same NFA transitions.
/// `DfaBuilder` simulates only each range's starting byte instead of all 256
/// bytes, and `Dfa` stores one transition column per range.
struct ByteRanges {
    byte_to_range: [u8; 256],
    range_starts: Vec<u8>,
}

impl ByteRanges {
    fn from_insts(insts: &[Inst]) -> Self {
        let mut boundaries = [false; 257];
        for inst in insts {
            if let Inst::Range { start, end, .. } = *inst {
                boundaries[start as usize] = true;
                boundaries[end as usize + 1] = true;
            }
        }
        let mut byte_to_range = [0u8; 256];
        let mut range_starts = vec![0u8];
        for byte in 1..256usize {
            if boundaries[byte] {
                range_starts.push(byte as u8);
            }
            byte_to_range[byte] = (range_starts.len() - 1) as u8;
        }
        ByteRanges {
            byte_to_range,
            range_starts,
        }
    }
}

pub struct DfaBuilder {
    nfa: Nfa,
}

/// Compact DFA backed by a flat `u16` transition table indexed by
/// `state * num_byte_ranges + byte_to_range[byte]`.
///
/// `u16` holds every valid state index because `STATE_LIMIT` is 1,000, leaving
/// `u16::MAX` (`NO_TRANSITION`) for missing transitions.
pub struct Dfa {
    byte_to_range: [u8; 256],
    is_match: Vec<bool>,
    transitions: Vec<u16>,
    num_byte_ranges: usize,
}

impl DfaBuilder {
    pub fn new(nfa: Nfa) -> Self {
        DfaBuilder { nfa }
    }

    pub fn build(self) -> Result<Dfa, Error> {
        let byte_ranges = ByteRanges::from_insts(&self.nfa.insts);
        let num_byte_ranges = byte_ranges.range_starts.len();

        let mut reached_ips = SparseSet::new(self.nfa.insts.len());
        let mut pending_ips = Vec::new();
        let mut candidate_ips = Vec::new();

        let mut cache: HashMap<Vec<usize>, u16> = HashMap::with_capacity(64);
        let mut state_ips: Vec<Vec<usize>> = Vec::with_capacity(16);
        let mut is_match: Vec<bool> = Vec::with_capacity(16);
        let mut transitions: Vec<u16> = Vec::with_capacity(16 * num_byte_ranges);

        Self::expand_or(
            &self.nfa.insts,
            &mut reached_ips,
            &mut pending_ips,
            self.nfa.start,
        );
        let start_is_match =
            Self::collect_state_ips(&self.nfa.insts, &reached_ips, &mut candidate_ips);
        if candidate_ips.is_empty() {
            return Ok(Dfa {
                byte_to_range: [0; 256],
                is_match: Vec::new(),
                transitions: Vec::new(),
                num_byte_ranges: 0,
            });
        }

        let start_ips = candidate_ips.clone();
        cache.insert(start_ips.clone(), 0);
        state_ips.push(start_ips);
        is_match.push(start_is_match);
        transitions.resize(num_byte_ranges, NO_TRANSITION);

        let mut state = 0;
        while state < state_ips.len() {
            for (range_id, &range_start) in byte_ranges.range_starts.iter().enumerate() {
                reached_ips.clear();
                for &ip in &state_ips[state] {
                    if let Inst::Range { start, end, next } = self.nfa.insts[ip] {
                        if start <= range_start && range_start <= end {
                            Self::expand_or(
                                &self.nfa.insts,
                                &mut reached_ips,
                                &mut pending_ips,
                                next,
                            );
                        }
                    }
                }
                let next_is_match =
                    Self::collect_state_ips(&self.nfa.insts, &reached_ips, &mut candidate_ips);
                if candidate_ips.is_empty() {
                    continue;
                }
                let next_state = if let Some(&existing_state) = cache.get(candidate_ips.as_slice())
                {
                    existing_state
                } else {
                    if state_ips.len() >= STATE_LIMIT {
                        return Err(Error::TooManyStates(STATE_LIMIT));
                    }
                    let new_state = state_ips.len() as u16;
                    let new_state_ips = candidate_ips.clone();
                    cache.insert(new_state_ips.clone(), new_state);
                    state_ips.push(new_state_ips);
                    is_match.push(next_is_match);
                    transitions.resize(transitions.len() + num_byte_ranges, NO_TRANSITION);
                    new_state
                };
                transitions[state * num_byte_ranges + range_id] = next_state;
            }
            state += 1;
        }

        Ok(Dfa {
            byte_to_range: byte_ranges.byte_to_range,
            is_match,
            transitions,
            num_byte_ranges,
        })
    }

    /// Follows all `Inst::Or` branches from `start_ip` using an explicit stack
    /// so long alternation chains avoid recursion stack overflow.
    fn expand_or(
        insts: &[Inst],
        reached_ips: &mut SparseSet,
        pending_ips: &mut Vec<usize>,
        start_ip: usize,
    ) {
        pending_ips.push(start_ip);
        while let Some(ip) = pending_ips.pop() {
            if reached_ips.contains(ip) {
                continue;
            }
            reached_ips.add(ip);
            match insts[ip] {
                Inst::Match | Inst::Fail | Inst::Range { .. } => {}
                Inst::Or(ip1, ip2) => {
                    pending_ips.push(ip2);
                    pending_ips.push(ip1);
                }
            }
        }
    }

    /// Extracts the `Range` and `Match` instruction indices from `reached_ips`
    /// in sorted order so identical sets hash to the same `cache` key
    /// regardless of `Inst::Or` traversal order.
    fn collect_state_ips(insts: &[Inst], reached_ips: &SparseSet, out: &mut Vec<usize>) -> bool {
        out.clear();
        let mut is_match = false;
        for i in 0..reached_ips.len() {
            let ip = reached_ips.get(i);
            match insts[ip] {
                Inst::Or(_, _) | Inst::Fail => {}
                Inst::Range { .. } => out.push(ip),
                Inst::Match => {
                    is_match = true;
                    out.push(ip);
                }
            }
        }
        out.sort_unstable();
        is_match
    }
}

impl Dfa {
    #[inline]
    pub fn start(&self) -> Option<usize> {
        (!self.is_match.is_empty()).then_some(0)
    }

    #[inline]
    pub fn is_match(&self, state: usize) -> bool {
        self.is_match[state]
    }

    #[inline]
    pub fn accept(&self, state: usize, byte: u8) -> Option<usize> {
        let range_id = self.byte_to_range[byte as usize] as usize;
        let next = self.transitions[state * self.num_byte_ranges + range_id];
        (next != NO_TRANSITION).then_some(next as usize)
    }

    #[cfg(test)]
    pub fn num_states(&self) -> usize {
        self.is_match.len()
    }
}

impl fmt::Debug for Dfa {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for (state, &matched) in self.is_match.iter().enumerate() {
            let marker = if matched { "*" } else { " " };
            writeln!(f, "{:03}{} ", state, marker)?;
            for byte in 0..256usize {
                if let Some(next) = self.accept(state, byte as u8) {
                    writeln!(f, "{:03}   {:X} => {}", state, byte, next)?;
                }
            }
        }
        Ok(())
    }
}
