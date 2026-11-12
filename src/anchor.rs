use crate::map::{Anchor, DirMap};
use crate::parse::Symbol;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorViolation {
    pub anchor: Anchor,
    pub reason: String,
}

fn check_one(anchor: &Anchor, symbols: &[Symbol]) -> Option<AnchorViolation> {
    let by_name: Vec<&Symbol> = symbols.iter().filter(|s| s.name == anchor.name).collect();

    if by_name.is_empty() {
        return Some(AnchorViolation {
            anchor: anchor.clone(),
            reason: format!("no symbol named `{}` in this directory", anchor.name),
        });
    }
    let in_file: Vec<&&Symbol> = by_name.iter().filter(|s| s.file == anchor.file).collect();
    if in_file.is_empty() {
        return Some(AnchorViolation {
            anchor: anchor.clone(),
            reason: format!("symbol `{}` is not in file `{}`", anchor.name, anchor.file),
        });
    }
    if !in_file.iter().any(|s| s.line == anchor.line) {
        return Some(AnchorViolation {
            anchor: anchor.clone(),
            reason: format!(
                "symbol `{}` is at line {} in `{}`, not line {}",
                anchor.name, in_file[0].line, anchor.file, anchor.line
            ),
        });
    }
    None
}

pub fn validate(map: &DirMap, symbols: &[Symbol]) -> Vec<AnchorViolation> {
    let mut out = Vec::new();
    if let Some(entry) = &map.entry {
        if let Some(v) = check_one(entry, symbols) {
            out.push(v);
        }
    }
    for (anchor, _) in &map.key_symbols {
        if let Some(v) = check_one(anchor, symbols) {
            out.push(v);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::SymbolKind;
    use std::collections::BTreeMap;

    fn symbols() -> Vec<Symbol> {
        vec![
            Symbol {
                name: "submit_batch".into(),
                kind: SymbolKind::Function,
                file: "batcher.rs".into(),
                line: 5,
            },
            Symbol {
                name: "BatchBuilder".into(),
                kind: SymbolKind::Struct,
                file: "builder.rs".into(),
                line: 1,
            },
        ]
    }

    fn map_with(entry: Option<Anchor>, keys: Vec<(Anchor, String)>) -> DirMap {
        DirMap {
            path: "src/settlement".into(),
            derived_from: "abc123".into(),
            what: "x".into(),
            entry,
            key_symbols: keys,
            depends_on: vec![],
            look_here_for: vec![],
            subdirs: vec![],
            hashes: BTreeMap::new(),
        }
    }

    #[test]
    fn valid_anchors_produce_no_violations() {
        let m = map_with(
            Some(Anchor {
                name: "submit_batch".into(),
                file: "batcher.rs".into(),
                line: 5,
            }),
            vec![(
                Anchor {
                    name: "BatchBuilder".into(),
                    file: "builder.rs".into(),
                    line: 1,
                },
                "d".into(),
            )],
        );
        assert_eq!(validate(&m, &symbols()), vec![]);
    }

    #[test]
    fn wrong_line_is_a_violation() {
        let m = map_with(
            Some(Anchor {
                name: "submit_batch".into(),
                file: "batcher.rs".into(),
                line: 99,
            }),
            vec![],
        );
        let v = validate(&m, &symbols());
        assert_eq!(v.len(), 1);
        assert!(v[0].reason.contains("line"), "got {:?}", v[0].reason);
    }

    #[test]
    fn unknown_symbol_name_is_a_violation() {
        let m = map_with(
            None,
            vec![(
                Anchor {
                    name: "ghost".into(),
                    file: "batcher.rs".into(),
                    line: 5,
                },
                "d".into(),
            )],
        );
        let v = validate(&m, &symbols());
        assert_eq!(v.len(), 1);
        assert!(
            v[0].reason.contains("no symbol named"),
            "got {:?}",
            v[0].reason
        );
    }

    #[test]
    fn wrong_file_is_a_violation() {
        let m = map_with(
            Some(Anchor {
                name: "submit_batch".into(),
                file: "builder.rs".into(),
                line: 5,
            }),
            vec![],
        );
        assert_eq!(validate(&m, &symbols()).len(), 1);
    }

    #[test]
    fn reports_every_violation_not_just_the_first() {
        let m = map_with(
            Some(Anchor {
                name: "ghost_a".into(),
                file: "batcher.rs".into(),
                line: 1,
            }),
            vec![
                (
                    Anchor {
                        name: "ghost_b".into(),
                        file: "batcher.rs".into(),
                        line: 2,
                    },
                    "d".into(),
                ),
                (
                    Anchor {
                        name: "ghost_c".into(),
                        file: "batcher.rs".into(),
                        line: 3,
                    },
                    "d".into(),
                ),
            ],
        );
        assert_eq!(validate(&m, &symbols()).len(), 3);
    }
}
