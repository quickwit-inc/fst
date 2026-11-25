use crate::Automaton;

use super::{Error, Regex};

fn matches(re: &Regex, input: &str) -> bool {
    let mut state = re.start();
    for &byte in input.as_bytes() {
        state = re.accept(&state, byte);
        if !re.can_match(&state) {
            return false;
        }
    }
    re.is_match(&state)
}

#[test]
fn test_suffix_sharing_and_state_deduplication() {
    let unfactored_35 = (1..=9)
        .map(|i| format!("10{i}."))
        .chain((1..=9).map(|i| format!("1{i}..")))
        .chain((2..=9).map(|i| format!("{i}...")))
        .chain((1..=9).map(|i| format!("{i}....")))
        .collect::<Vec<_>>()
        .join("|");
    let factored_35 = "(101|102|103|104|105|106|107|108|109).|\
                       (11|12|13|14|15|16|17|18|19)..|\
                       (2|3|4|5|6|7|8|9)...|\
                       (1|2|3|4|5|6|7|8|9)....";

    let re_unfactored = Regex::new(&unfactored_35).unwrap();
    let re_factored = Regex::new(factored_35).unwrap();
    assert_eq!(re_unfactored.dfa.num_states(), re_factored.dfa.num_states());
    assert_eq!(re_unfactored.dfa.num_states(), 60);

    for (input, expected) in [
        ("1015", true),
        ("109a", true),
        ("109é", true),
        ("109😀", true),
        ("1599", true),
        ("5432", true),
        ("98765", true),
        ("1001", false),
        ("999", false),
        ("100000", false),
        ("101\n", false),
    ] {
        assert_eq!(
            matches(&re_unfactored, input),
            expected,
            "unfactored: {input}"
        );
        assert_eq!(matches(&re_factored, input), expected, "factored: {input}");
    }

    // Many wildcards sharing a `.*` suffix stay compact (19 states vs >1,000 before).
    let wildcards_180 = (0..180)
        .map(|i| format!("p{i:03}abcdef.*"))
        .collect::<Vec<_>>()
        .join("|");
    let re_wildcards = Regex::new(&wildcards_180).unwrap();
    assert_eq!(re_wildcards.dfa.num_states(), 19);
    assert!(matches(&re_wildcards, "p042abcdef_xyz"));
    assert!(!matches(&re_wildcards, "p999abcdef"));

    // Overlapping prefixes sharing `.*` stay compact (13 states vs >1,000 before).
    let overlapping = (0..60)
        .map(|i| format!("pre{i:02}.*"))
        .chain(std::iter::once("pre.*".to_string()))
        .collect::<Vec<_>>()
        .join("|");
    let re_overlap = Regex::new(&overlapping).unwrap();
    assert!(re_overlap.dfa.num_states() <= 15);
    assert!(matches(&re_overlap, "pre"));
    assert!(matches(&re_overlap, "pre42xyz"));
    assert!(!matches(&re_overlap, "pr"));

    // Shared suffixes and reordered alternations produce identical state counts.
    assert_eq!(
        Regex::new("foo.*|bar.*").unwrap().dfa.num_states(),
        Regex::new("(foo|bar).*").unwrap().dfa.num_states()
    );
    assert_eq!(
        Regex::new("(a|b)c").unwrap().dfa.num_states(),
        Regex::new("(b|a)c").unwrap().dfa.num_states()
    );
}

#[test]
fn test_edge_cases_and_unicode() {
    // Empty regex matches only "".
    let empty = Regex::new("").unwrap();
    assert!(matches(&empty, ""));
    assert!(!matches(&empty, "a"));

    // Empty-matching nested repetitions.
    for pat in ["()*", "(a*)*", "(a?)*", "(a|)*"] {
        let re = Regex::new(pat).unwrap();
        assert!(matches(&re, ""), "pat={pat}");
        if pat != "()*" {
            assert!(matches(&re, "a"), "pat={pat}");
            assert!(matches(&re, "aaaa"), "pat={pat}");
        }
        assert!(!matches(&re, "b"), "pat={pat}");
    }

    // Bounded quantifiers.
    let bounded = Regex::new("a{2,4}").unwrap();
    assert!(!matches(&bounded, "a"));
    assert!(matches(&bounded, "aa"));
    assert!(matches(&bounded, "aaa"));
    assert!(matches(&bounded, "aaaa"));
    assert!(!matches(&bounded, "aaaaa"));

    let zero_rep = Regex::new("a{0,0}b").unwrap();
    assert!(matches(&zero_rep, "b"));
    assert!(!matches(&zero_rep, "ab"));

    // Unicode 1, 2, 3, and 4 byte UTF-8 characters.
    let dot = Regex::new(".").unwrap();
    for ch in ["a", "é", "中", "😀"] {
        assert!(matches(&dot, ch), "dot should match {ch}");
    }
    assert!(!matches(&dot, "\n"));
    assert!(!matches(&dot, "ab"));

    let dot_all = Regex::new("(?s).").unwrap();
    assert!(matches(&dot_all, "\n"));
    assert!(matches(&dot_all, "😀"));

    // Case-insensitive alternation with shared suffix.
    let ci = Regex::new("(?i)Foo.*|(?i)Bar.*").unwrap();
    assert!(matches(&ci, "foo123"));
    assert!(matches(&ci, "FOO"));
    assert!(matches(&ci, "bAr_baz"));
    assert!(!matches(&ci, "baz"));
}

#[test]
fn test_error_conditions() {
    assert!(matches!(Regex::new("a+?"), Err(Error::NoLazy)));
    assert!(matches!(Regex::new("^a"), Err(Error::NoEmpty)));
    assert!(matches!(Regex::new(r"(?-u:[a-z])"), Err(Error::NoBytes)));
    // Validation must still run even when concatenated with an empty class.
    assert!(matches!(Regex::new(r"(a+?)[^\s\S]"), Err(Error::NoLazy)));

    // A regex requiring more than 1,000 DFA states returns TooManyStates(1_000).
    let mut distinct_prefixes = Vec::new();
    let mut rng_state: u64 = 0x1234_5678_9abc_def0;
    for _ in 0..250 {
        let mut prefix = String::new();
        for _ in 0..8 {
            rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let ch = (b'a' + ((rng_state >> 32) % 26) as u8) as char;
            prefix.push(ch);
        }
        prefix.push_str(".*");
        distinct_prefixes.push(prefix);
    }
    let huge = distinct_prefixes.join("|");
    assert!(matches!(
        Regex::new(&huge),
        Err(Error::TooManyStates(1_000))
    ));
}

/// Evaluates full-string matches directly on `Hir` by tracking reachable
/// byte offsets, used as a reference oracle against `Regex`.
fn hir_matches(hir: &regex_syntax::hir::Hir, input: &str) -> bool {
    use regex_syntax::hir::{Class, Hir, HirKind};
    use std::collections::HashSet;

    fn eval(hir: &Hir, input: &str, pos: usize) -> HashSet<usize> {
        let mut out = HashSet::new();
        match hir.kind() {
            HirKind::Empty => {
                out.insert(pos);
            }
            HirKind::Literal(lit) => {
                if input.as_bytes()[pos..].starts_with(&lit.0) {
                    out.insert(pos + lit.0.len());
                }
            }
            HirKind::Class(Class::Unicode(class)) => {
                if let Some(ch) = input[pos..].chars().next() {
                    if class
                        .ranges()
                        .iter()
                        .any(|range| range.start() <= ch && ch <= range.end())
                    {
                        out.insert(pos + ch.len_utf8());
                    }
                }
            }
            HirKind::Class(Class::Bytes(_)) => {}
            HirKind::Capture(cap) => return eval(&cap.sub, input, pos),
            HirKind::Concat(subs) => {
                let mut cur = HashSet::new();
                cur.insert(pos);
                for sub in subs {
                    let mut next = HashSet::new();
                    for offset in cur {
                        next.extend(eval(sub, input, offset));
                    }
                    cur = next;
                }
                return cur;
            }
            HirKind::Alternation(subs) => {
                for sub in subs {
                    out.extend(eval(sub, input, pos));
                }
            }
            HirKind::Repetition(rep) => {
                let mut cur = HashSet::new();
                cur.insert(pos);
                for _ in 0..rep.min {
                    let mut next = HashSet::new();
                    for offset in cur {
                        next.extend(eval(&rep.sub, input, offset));
                    }
                    cur = next;
                }
                if let Some(max) = rep.max {
                    out.extend(&cur);
                    for _ in rep.min..max {
                        let mut next = HashSet::new();
                        for offset in &cur {
                            next.extend(eval(&rep.sub, input, *offset));
                        }
                        out.extend(&next);
                        cur = next;
                    }
                } else {
                    // Fixed-point closure for unbounded repetition.
                    out.extend(&cur);
                    let mut frontier: Vec<usize> = cur.into_iter().collect();
                    while let Some(offset) = frontier.pop() {
                        for next_offset in eval(&rep.sub, input, offset) {
                            if out.insert(next_offset) {
                                frontier.push(next_offset);
                            }
                        }
                    }
                }
            }
            HirKind::Look(_) => unreachable!(),
        }
        out
    }

    eval(hir, input, 0).contains(&input.len())
}

mod prop_equiv {
    use super::*;
    use proptest::prelude::*;

    fn atom_strategy() -> BoxedStrategy<String> {
        prop_oneof![
            Just("a".to_string()),
            Just("b".to_string()),
            Just("c".to_string()),
            Just(".".to_string()),
            Just("[ab]".to_string()),
            Just("[a-c]".to_string()),
            Just("".to_string()),
            Just("é".to_string()),
        ]
        .boxed()
    }

    fn quantified_strategy() -> BoxedStrategy<String> {
        (
            atom_strategy(),
            prop_oneof![
                Just(""),
                Just("*"),
                Just("+"),
                Just("?"),
                Just("{0,2}"),
                Just("{1,2}"),
                Just("{2}"),
            ],
        )
            .prop_map(|(atom, quantifier)| {
                if atom.is_empty() {
                    atom
                } else {
                    format!("(?:{atom}){quantifier}")
                }
            })
            .boxed()
    }

    fn branch_strategy() -> BoxedStrategy<String> {
        prop::collection::vec(quantified_strategy(), 1..=4)
            .prop_map(|parts| parts.concat())
            .boxed()
    }

    fn regex_strategy() -> BoxedStrategy<String> {
        prop::collection::vec(branch_strategy(), 1..=4)
            .prop_map(|branches| branches.join("|"))
            .boxed()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(300))]
        #[test]
        fn prop_dfa_matches_hir_reference(
            pattern in regex_strategy(),
            inputs in prop::collection::vec("[abcé\n]{0,5}", 1..=15),
        ) {
            let hir = regex_syntax::Parser::new().parse(&pattern).unwrap();
            let re = Regex::new(&pattern).unwrap();
            for input in &inputs {
                prop_assert_eq!(
                    matches(&re, input),
                    hir_matches(&hir, input),
                    "mismatch for pattern={:?}, input={:?}",
                    pattern,
                    input
                );
            }
        }
    }
}
