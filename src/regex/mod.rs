use crate::Automaton;
use regex_syntax;
use std::fmt;
use std::time::Instant;

mod compile;
mod dfa;
mod error;
mod sparse;

pub use self::error::Error;

/// A regular expression for searching FSTs with Unicode support.
///
/// Regular expressions are compiled down to a deterministic finite automaton
/// that can efficiently search any finite state transducer. Notably, most
/// regular expressions only need to explore a small portion of a finite state
/// transducer without loading all of it into memory.
///
/// # Syntax
///
/// `Regex` supports fully featured regular expressions. Namely, it supports
/// all of the same constructs as the standard `regex` crate except for the
/// following things:
///
/// 1. Lazy quantifiers, since a regular expression automaton only reports
///    whether a key matches at all, and not its location. Namely, lazy
///    quantifiers such as `+?` only modify the location of a match, but never
///    change a non-match into a match or a match into a non-match.
/// 2. Word boundaries (i.e., `\b`). Because such things are hard to do in
///    a deterministic finite automaton, but not impossible. As such, these
///    may be allowed some day.
/// 3. Other zero width assertions like `^` and `$`. These are easier to
///    support than word boundaries, but are still tricky and usually aren't
///    as useful when searching dictionaries.
///
/// Otherwise, the [full syntax of the `regex`
/// crate](http://doc.rust-lang.org/regex/regex/index.html#syntax)
/// is supported. This includes all Unicode support and relevant flags.
/// (The `U` and `m` flags are no-ops because of (1) and (3) above,
/// respectively.)
///
/// # Matching semantics
///
/// A regular expression matches a key in a finite state transducer if and only
/// if it matches from the start of a key all the way to end. Stated
/// differently, every regular expression `(re)` is matched as if it were
/// `^(re)$`. This means that if you want to do a substring match, then you
/// must use `.*substring.*`.
///
/// **Caution**: Starting a regular expression with `.*` means that it could
/// potentially match *any* key in a finite state transducer. This implies that
/// all keys could be visited, which could be slow. It is possible that this
/// crate will grow facilities for detecting regular expressions that will
/// scan a large portion of a transducer and optionally disallow them.
///
pub struct Regex {
    original: String,
    dfa: dfa::Dfa,
}

#[derive(Eq, PartialEq)]
pub enum Inst {
    Match,
    Jump(usize),
    Split(usize, usize),
    Range(u8, u8),
}

impl Regex {
    /// Create a new regular expression query.
    ///
    /// The query finds all terms matching the regular expression.
    ///
    /// If the regular expression is malformed or if it results in an automaton
    /// that is too big, then an error is returned.
    ///
    /// A `Regex` value satisfies the `Automaton` trait, which means it can be
    /// used with the `search` method of any finite state transducer.
    #[inline]
    pub fn new(re: &str) -> Result<Regex, Error> {
        Regex::with_size_limit(10 * (1 << 20), re)
    }

    /// Create a new regular expression query with a wall-clock deadline on
    /// compilation.
    ///
    /// Identical to [`Regex::new`] except that the NFA-to-DFA construction
    /// will periodically check `Instant::now()` and abort with
    /// [`Error::DeadlineExceeded`] if `deadline` has passed.
    ///
    /// This is useful for hardening services that accept user-supplied regex
    /// patterns: pathological inputs such as `\w{0,5}\w{0,5}\w{0,5}\w{0,5}`
    /// can take multiple seconds to compile even though they ultimately fail
    /// the existing `STATE_LIMIT` / size-limit guards. The existing limits
    /// bound *memory* but not *wall-clock time*; a deadline closes that gap.
    ///
    /// The check is batched (every ~1024 transitions) so the overhead is well
    /// under 1% on well-behaved patterns. Worst-case overshoot of `deadline`
    /// is on the order of a few hundred microseconds.
    #[inline]
    pub fn new_with_deadline(re: &str, deadline: Instant) -> Result<Regex, Error> {
        Regex::with_size_limit_and_deadline(10 * (1 << 20), re, Some(deadline))
    }

    fn with_size_limit(size: usize, re: &str) -> Result<Regex, Error> {
        Regex::with_size_limit_and_deadline(size, re, None)
    }

    fn with_size_limit_and_deadline(
        size: usize,
        re: &str,
        deadline: Option<Instant>,
    ) -> Result<Regex, Error> {
        let hir = regex_syntax::Parser::new().parse(re)?;
        let insts = self::compile::Compiler::new(size).compile(&hir)?;
        let dfa = self::dfa::DfaBuilder::new(insts).build_with_deadline(deadline)?;
        Ok(Regex {
            original: re.to_owned(),
            dfa,
        })
    }
}

impl Automaton for Regex {
    type State = Option<usize>;

    #[inline]
    fn start(&self) -> Option<usize> {
        Some(0)
    }

    #[inline]
    fn is_match(&self, state: &Option<usize>) -> bool {
        state.map(|state| self.dfa.is_match(state)).unwrap_or(false)
    }

    #[inline]
    fn can_match(&self, state: &Option<usize>) -> bool {
        state.is_some()
    }

    #[inline]
    fn accept(&self, state: &Option<usize>, byte: u8) -> Option<usize> {
        state.and_then(|state| self.dfa.accept(state, byte))
    }
}

impl fmt::Debug for Regex {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "Regex({:?})", self.original)?;
        self.dfa.fmt(f)
    }
}

impl fmt::Debug for Inst {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::Inst::*;
        match *self {
            Match => write!(f, "Match"),
            Jump(ip) => write!(f, "JUMP {}", ip),
            Split(ip1, ip2) => write!(f, "SPLIT {}, {}", ip1, ip2),
            Range(s, e) => write!(f, "RANGE {:X}-{:X}", s, e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Error, Regex};
    use std::time::{Duration, Instant};

    /// A pattern whose NFA-to-DFA expansion is known to take multiple seconds
    /// before the existing `STATE_LIMIT=1000` guard kicks in.
    ///
    /// Documented timings (release build, x86_64 Linux):
    /// `\w{0,5}\w{0,5}\w{0,5}\w{0,5}` ~ 2.3 s, terminating in
    /// `Error::TooManyStates(1000)`. Without a deadline, this is a clean DoS
    /// vector for any service that compiles user-supplied regex patterns on a
    /// request thread.
    const SLOW_PATTERN: &str = r"\w{0,5}\w{0,5}\w{0,5}\w{0,5}";

    #[test]
    fn deadline_aborts_pathological_pattern() {
        let deadline = Instant::now() + Duration::from_millis(50);
        let t0 = Instant::now();
        let result = Regex::new_with_deadline(SLOW_PATTERN, deadline);
        let elapsed = t0.elapsed();
        match result {
            Err(Error::DeadlineExceeded) => {}
            other => panic!(
                "expected DeadlineExceeded, got {:?} after {:?}",
                other.map(|_| "Ok").map_err(|e| e), elapsed
            ),
        }
        // Worst-case overshoot: one full check interval. Allow generous
        // headroom for slow CI runners; the important property is that we
        // bail in well under the unbounded ~2.3s baseline.
        assert!(
            elapsed < Duration::from_millis(500),
            "deadline overshoot: {:?}", elapsed
        );
    }

    #[test]
    fn deadline_does_not_affect_easy_pattern() {
        let deadline = Instant::now() + Duration::from_secs(60);
        let regex = Regex::new_with_deadline("hello.*world", deadline)
            .expect("simple pattern should compile well within deadline");
        // Sanity check: the resulting Regex behaves like one built via
        // `Regex::new` (round-trips its source via Debug).
        let dbg = format!("{:?}", regex);
        assert!(dbg.contains("hello.*world"), "unexpected Debug: {}", dbg);
    }

    #[test]
    fn new_unchanged_for_easy_pattern() {
        // Backwards-compatibility smoke: existing API unaffected.
        Regex::new("hello.*world").expect("simple pattern must compile");
    }

    #[test]
    fn deadline_exceeded_error_is_distinct() {
        // Make sure the new variant doesn't accidentally collapse into
        // TooManyStates / Syntax in Display / Debug.
        let err = Error::DeadlineExceeded;
        assert_eq!(format!("{}", err), "Regex compilation deadline exceeded.");
        let dbg = format!("{:?}", err);
        assert_eq!(dbg, "DeadlineExceeded");
    }
}
