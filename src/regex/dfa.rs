use std::collections::HashMap;
use std::fmt;

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
            if let Inst::Range(start, end) = *inst {
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
    insts: Vec<Inst>,
}

struct StateTable {
    num_byte_ranges: usize,
    cache: HashMap<Vec<usize>, u16>,
    state_ips: Vec<Vec<usize>>,
    is_match: Vec<bool>,
    transitions: Vec<u16>,
}

impl StateTable {
    fn new(num_byte_ranges: usize) -> Self {
        StateTable {
            num_byte_ranges,
            cache: HashMap::with_capacity(64),
            state_ips: Vec::with_capacity(16),
            is_match: Vec::with_capacity(16),
            transitions: Vec::with_capacity(16 * num_byte_ranges),
        }
    }

    fn get_or_add_state(&mut self, ips: &[usize], matched: bool) -> Result<u16, Error> {
        if let Some(&existing_state) = self.cache.get(ips) {
            return Ok(existing_state);
        }
        if self.state_ips.len() >= STATE_LIMIT {
            return Err(Error::TooManyStates(STATE_LIMIT));
        }
        let new_state = self.state_ips.len() as u16;
        self.cache.insert(ips.to_vec(), new_state);
        self.state_ips.push(ips.to_vec());
        self.is_match.push(matched);
        self.transitions
            .resize(self.transitions.len() + self.num_byte_ranges, NO_TRANSITION);
        Ok(new_state)
    }
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
    pub fn new(insts: Vec<Inst>) -> Self {
        DfaBuilder { insts }
    }

    pub fn build(self) -> Result<Dfa, Error> {
        let byte_ranges = ByteRanges::from_insts(&self.insts);
        let num_byte_ranges = byte_ranges.range_starts.len();

        let mut table = StateTable::new(num_byte_ranges);
        let mut visited_ips = SparseSet::new(self.insts.len());
        let mut pending_ips = Vec::new();
        let mut candidate_ips = Vec::new();

        let start_is_match = expand_epsilon(
            &self.insts,
            &mut visited_ips,
            &mut pending_ips,
            &mut candidate_ips,
            0,
        );
        if !candidate_ips.is_empty() {
            // Sort so identical sets of reachable instructions hash to the
            // same `cache` key regardless of `Inst::Split` traversal order.
            candidate_ips.sort_unstable();
            table.get_or_add_state(&candidate_ips, start_is_match)?;
        }

        let mut state = 0;
        while state < table.state_ips.len() {
            for (range_id, &range_start) in byte_ranges.range_starts.iter().enumerate() {
                visited_ips.clear();
                candidate_ips.clear();
                let mut next_is_match = false;
                for &ip in &table.state_ips[state] {
                    if let Inst::Range(start, end) = self.insts[ip] {
                        if start <= range_start && range_start <= end {
                            next_is_match |= expand_epsilon(
                                &self.insts,
                                &mut visited_ips,
                                &mut pending_ips,
                                &mut candidate_ips,
                                ip + 1,
                            );
                        }
                    }
                }
                if candidate_ips.is_empty() {
                    continue;
                }
                candidate_ips.sort_unstable();
                let next_state = table.get_or_add_state(&candidate_ips, next_is_match)?;
                table.transitions[state * num_byte_ranges + range_id] = next_state;
            }
            state += 1;
        }

        Ok(Dfa {
            byte_to_range: byte_ranges.byte_to_range,
            is_match: table.is_match,
            transitions: table.transitions,
            num_byte_ranges,
        })
    }
}

/// Follows all `Inst::Jump` and `Inst::Split` branches from `start_ip` using an
/// explicit stack, collecting every reached `Range` and `Match` instruction
/// index into `out_ips` and returning whether `Inst::Match` was reached.
fn expand_epsilon(
    insts: &[Inst],
    visited_ips: &mut SparseSet,
    pending_ips: &mut Vec<usize>,
    out_ips: &mut Vec<usize>,
    start_ip: usize,
) -> bool {
    let mut is_match = false;
    pending_ips.push(start_ip);
    while let Some(ip) = pending_ips.pop() {
        if visited_ips.contains(ip) {
            continue;
        }
        visited_ips.add(ip);
        match insts[ip] {
            Inst::Jump(next) => pending_ips.push(next),
            Inst::Split(ip1, ip2) => {
                pending_ips.push(ip2);
                pending_ips.push(ip1);
            }
            Inst::Range(_, _) => out_ips.push(ip),
            Inst::Match => {
                is_match = true;
                out_ips.push(ip);
            }
        }
    }
    is_match
}

impl Dfa {
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
