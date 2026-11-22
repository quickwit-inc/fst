use std::collections::HashMap;

use regex_syntax::hir::{Class, Hir, HirKind};
use utf8_ranges::Utf8Sequences;

use super::{Error, Inst};

pub(super) const MATCH_IP: usize = 0;
const FAIL_IP: usize = 1;

#[inline]
fn strip_outer_captures(mut hir: &Hir) -> &Hir {
    while let HirKind::Capture(capture) = hir.kind() {
        hir = &capture.sub;
    }
    hir
}

pub struct Nfa {
    pub insts: Vec<Inst>,
    pub start: usize,
}

/// Compiles an HIR regex into an NFA right-to-left, starting from `MATCH_IP`.
///
/// Each instruction stores its explicit `next` target. Compiling right-to-left
/// means `next` is known before an instruction is created, so `inst_to_ip` can
/// reuse identical instructions across branches. Shared suffixes (such as
/// `101.|102.` or UTF-8 continuation bytes inside `.`) collapse to the same
/// instruction indices, letting `DfaBuilder` merge their DFA states directly.
pub struct Compiler<'a> {
    size_limit: usize,
    insts: Vec<Inst>,
    inst_to_ip: HashMap<Inst, usize>,
    /// Reuses zero-or-more loops (`e*`) that share the same body and `after_ip`.
    ///
    /// Loops are cyclic: the loop's `Inst::Or` placeholder is allocated before
    /// its body is compiled, so `inst_to_ip` alone assigns a fresh `loop_ip` to
    /// each copy. Grouping compiled loops by `after_ip` lets alternations like
    /// `foo.*|bar.*` share a single loop node.
    loops_by_after_ip: HashMap<usize, Vec<(&'a Hir, usize)>>,
}

impl<'a> Compiler<'a> {
    pub fn new(size_limit: usize) -> Compiler<'a> {
        Compiler {
            size_limit,
            insts: vec![Inst::Match, Inst::Fail],
            inst_to_ip: HashMap::new(),
            loops_by_after_ip: HashMap::new(),
        }
    }

    pub fn compile(mut self, hir: &'a Hir) -> Result<Nfa, Error> {
        let start = self.compile_hir(hir, MATCH_IP)?;
        Ok(Nfa {
            insts: self.insts,
            start,
        })
    }

    /// Compiles `hir` right-to-left, jumping to `after_ip` after `hir` matches.
    ///
    /// Invariant: every returned instruction index other than `FAIL_IP` has a
    /// path to `MATCH_IP`. Branches that evaluate to `FAIL_IP` are dropped from
    /// `Inst::Or` and range transitions, keeping unreachable branches out of
    /// the NFA.
    fn compile_hir(&mut self, hir: &'a Hir, after_ip: usize) -> Result<usize, Error> {
        let ip = match hir.kind() {
            HirKind::Empty => after_ip,
            HirKind::Literal(literal) => {
                let mut cur_ip = after_ip;
                for &byte in literal.0.iter().rev() {
                    cur_ip = self.get_or_push_range(byte, byte, cur_ip);
                }
                cur_ip
            }
            HirKind::Class(class) => match class {
                Class::Bytes(class_bytes) => {
                    if class_bytes.ranges().is_empty() {
                        FAIL_IP
                    } else {
                        return Err(Error::NoBytes);
                    }
                }
                Class::Unicode(class_unicode) => {
                    if after_ip == FAIL_IP {
                        FAIL_IP
                    } else {
                        let mut branch_ips = Vec::new();
                        for &char_range in class_unicode.ranges() {
                            for utf8_seq in Utf8Sequences::new(char_range.start(), char_range.end())
                            {
                                let mut cur_ip = after_ip;
                                for byte_range in utf8_seq.as_slice().iter().rev() {
                                    cur_ip = self.get_or_push_range(
                                        byte_range.start,
                                        byte_range.end,
                                        cur_ip,
                                    );
                                }
                                branch_ips.push(cur_ip);
                            }
                        }
                        self.any_of(branch_ips)
                    }
                }
            },
            HirKind::Capture(capture) => self.compile_hir(&capture.sub, after_ip)?,
            HirKind::Concat(hirs) => {
                let mut cur_ip = after_ip;
                for sub in hirs.iter().rev() {
                    cur_ip = self.compile_hir(sub, cur_ip)?;
                }
                cur_ip
            }
            HirKind::Alternation(es) => {
                let mut branch_ips = Vec::with_capacity(es.len());
                for e in es {
                    branch_ips.push(self.compile_hir(e, after_ip)?);
                }
                self.any_of(branch_ips)
            }
            HirKind::Repetition(repetition) => {
                if !repetition.greedy {
                    return Err(Error::NoLazy);
                }
                if after_ip == FAIL_IP {
                    // Still compile the subexpression so invalid constructs
                    // (such as lazy quantifiers or byte classes) are rejected.
                    self.compile_hir(&repetition.sub, FAIL_IP)?;
                    FAIL_IP
                } else {
                    let mut cur_ip = if let Some(max) = repetition.max {
                        let mut cur_ip = after_ip;
                        for _ in repetition.min..max {
                            let body_ip = self.compile_hir(&repetition.sub, cur_ip)?;
                            let optional_ip = self.either(body_ip, after_ip);
                            if optional_ip == cur_ip {
                                break;
                            }
                            cur_ip = optional_ip;
                        }
                        cur_ip
                    } else {
                        self.compile_zero_or_more(&repetition.sub, after_ip)?
                    };
                    for _ in 0..repetition.min {
                        cur_ip = self.compile_hir(&repetition.sub, cur_ip)?;
                    }
                    cur_ip
                }
            }
            HirKind::Look(_) => return Err(Error::NoEmpty),
        };
        self.check_size()?;
        Ok(ip)
    }

    fn compile_zero_or_more(&mut self, sub: &'a Hir, after_ip: usize) -> Result<usize, Error> {
        let body_hir = strip_outer_captures(sub);
        if let Some(loops) = self.loops_by_after_ip.get(&after_ip) {
            if let Some(&(_, existing_ip)) = loops.iter().find(|&&(hir, _)| hir == body_hir) {
                return Ok(existing_ip);
            }
        }
        let loop_ip = self.push_inst(Inst::Or(FAIL_IP, FAIL_IP));
        let body_ip = self.compile_hir(body_hir, loop_ip)?;
        let start_ip = if body_ip == loop_ip || body_ip == FAIL_IP {
            after_ip
        } else {
            self.insts[loop_ip] = Inst::Or(body_ip, after_ip);
            loop_ip
        };
        self.loops_by_after_ip
            .entry(after_ip)
            .or_default()
            .push((body_hir, start_ip));
        Ok(start_ip)
    }

    #[inline]
    fn get_or_push_range(&mut self, start: u8, end: u8, next: usize) -> usize {
        if next == FAIL_IP {
            return FAIL_IP;
        }
        self.get_or_push_inst(Inst::Range { start, end, next })
    }

    /// Combines two branch indices with `Inst::Or`, ordering `(lo, hi)` so
    /// swapped branches produce the same instruction for `inst_to_ip`.
    #[inline]
    fn either(&mut self, first_ip: usize, second_ip: usize) -> usize {
        if first_ip == FAIL_IP {
            return second_ip;
        }
        if second_ip == FAIL_IP || first_ip == second_ip {
            return first_ip;
        }
        self.get_or_push_inst(Inst::Or(first_ip.min(second_ip), first_ip.max(second_ip)))
    }

    /// Deduplicates and sorts `branch_ips` so alternations with different
    /// branch orders produce identical `Inst::Or` chains.
    fn any_of(&mut self, mut branch_ips: Vec<usize>) -> usize {
        branch_ips.retain(|&ip| ip != FAIL_IP);
        branch_ips.sort_unstable();
        branch_ips.dedup();
        branch_ips
            .into_iter()
            .rev()
            .reduce(|acc, ip| self.either(ip, acc))
            .unwrap_or(FAIL_IP)
    }

    #[inline]
    fn get_or_push_inst(&mut self, inst: Inst) -> usize {
        if let Some(&ip) = self.inst_to_ip.get(&inst) {
            return ip;
        }
        let ip = self.push_inst(inst);
        self.inst_to_ip.insert(inst, ip);
        ip
    }

    #[inline]
    fn push_inst(&mut self, inst: Inst) -> usize {
        let ip = self.insts.len();
        self.insts.push(inst);
        ip
    }

    fn check_size(&self) -> Result<(), Error> {
        use std::mem::size_of;

        if self.insts.len() * size_of::<Inst>() > self.size_limit {
            Err(Error::CompiledTooBig(self.size_limit))
        } else {
            Ok(())
        }
    }
}
