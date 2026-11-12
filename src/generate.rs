use crate::hash::hash_dir;
use crate::llm::{DescribeRequest, DescribeResponse};
use crate::map::{Anchor, DirMap};
use crate::parse::Symbol;
use std::collections::BTreeMap;

/// Resolve a model-supplied symbol name to a parser-produced anchor.
/// Returns None when the name does not appear in the symbol table, which is
/// how a hallucinated name is prevented from ever reaching a map file.
fn resolve(name: &str, symbols: &[Symbol]) -> Option<Anchor> {
    symbols.iter().find(|s| s.name == name).map(|s| Anchor {
        name: s.name.clone(),
        file: s.file.clone(),
        line: s.line,
    })
}

pub fn assemble(
    dir_path: &str,
    derived_from: &str,
    symbols: &[Symbol],
    file_contents: &BTreeMap<String, String>,
    response: &DescribeResponse,
) -> DirMap {
    let entry = response
        .entry_symbol
        .as_deref()
        .and_then(|name| resolve(name, symbols));

    let key_symbols = response
        .key_symbols
        .iter()
        .filter_map(|(name, desc)| resolve(name, symbols).map(|a| (a, desc.clone())))
        .collect();

    let subdirs = response
        .subdir_notes
        .iter()
        .map(|(name, note)| (name.clone(), note.clone()))
        .collect();

    DirMap {
        path: dir_path.to_string(),
        derived_from: derived_from.to_string(),
        what: response.what.clone(),
        entry,
        key_symbols,
        depends_on: Vec::new(),
        look_here_for: response.look_here_for.clone(),
        subdirs,
        hashes: hash_dir(file_contents),
    }
}

pub fn request_for(
    custom_id: &str,
    dir_path: &str,
    symbols: &[Symbol],
    file_names: &[String],
    child_summaries: &[(String, String)],
) -> DescribeRequest {
    let mut symbol_names: Vec<String> = symbols.iter().map(|s| s.name.clone()).collect();
    symbol_names.sort();
    symbol_names.dedup();

    DescribeRequest {
        custom_id: custom_id.to_string(),
        dir_path: dir_path.to_string(),
        symbol_names,
        file_names: file_names.to_vec(),
        child_summaries: child_summaries.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anchor::validate;
    use crate::parse::SymbolKind;

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

    fn contents() -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        m.insert("batcher.rs".to_string(), "fn submit_batch() {}".to_string());
        m.insert("builder.rs".to_string(), "struct BatchBuilder;".to_string());
        m
    }

    #[test]
    fn resolves_names_to_parser_anchors() {
        let resp = DescribeResponse {
            custom_id: "d1".into(),
            what: "Batching.".into(),
            entry_symbol: Some("submit_batch".into()),
            key_symbols: vec![("BatchBuilder".into(), "builds batches".into())],
            look_here_for: vec!["batching".into()],
            subdir_notes: vec![],
        };
        let m = assemble("src/settlement", "abc123", &symbols(), &contents(), &resp);

        let entry = m.entry.as_ref().unwrap();
        assert_eq!(
            entry,
            &Anchor {
                name: "submit_batch".into(),
                file: "batcher.rs".into(),
                line: 5
            }
        );
        assert_eq!(m.key_symbols[0].0.line, 1);
        assert_eq!(m.key_symbols[0].0.file, "builder.rs");
        assert_eq!(m.what, "Batching.");
        assert_eq!(m.derived_from, "abc123");
    }

    #[test]
    fn hallucinated_symbol_names_are_dropped_not_emitted() {
        let resp = DescribeResponse {
            custom_id: "d1".into(),
            what: "Batching.".into(),
            entry_symbol: Some("does_not_exist".into()),
            key_symbols: vec![
                ("BatchBuilder".into(), "real".into()),
                ("also_fake".into(), "fabricated".into()),
            ],
            look_here_for: vec![],
            subdir_notes: vec![],
        };
        let m = assemble("src/settlement", "abc123", &symbols(), &contents(), &resp);

        assert!(m.entry.is_none(), "unresolvable entry must be dropped");
        assert_eq!(m.key_symbols.len(), 1, "only real symbols survive");
        assert_eq!(m.key_symbols[0].0.name, "BatchBuilder");
    }

    #[test]
    fn assembled_maps_always_pass_anchor_validation() {
        let resp = DescribeResponse {
            custom_id: "d1".into(),
            what: "Batching.".into(),
            entry_symbol: Some("submit_batch".into()),
            key_symbols: vec![
                ("BatchBuilder".into(), "real".into()),
                ("ghost".into(), "fake".into()),
            ],
            look_here_for: vec![],
            subdir_notes: vec![],
        };
        let m = assemble("src/settlement", "abc123", &symbols(), &contents(), &resp);
        assert_eq!(
            validate(&m, &symbols()),
            vec![],
            "assemble must never produce an anchor that fails validation"
        );
    }

    #[test]
    fn records_hashes_for_every_file() {
        let resp = DescribeResponse {
            custom_id: "d1".into(),
            what: "x".into(),
            ..Default::default()
        };
        let m = assemble("src/settlement", "abc123", &symbols(), &contents(), &resp);
        assert_eq!(m.hashes.len(), 2);
        assert!(m.hashes.contains_key("batcher.rs"));
    }

    #[test]
    fn request_carries_names_only() {
        let req = request_for(
            "d1",
            "src/settlement",
            &symbols(),
            &["batcher.rs".to_string()],
            &[],
        );
        assert_eq!(req.custom_id, "d1");
        assert_eq!(req.symbol_names, vec!["BatchBuilder", "submit_batch"]);
        assert_eq!(req.file_names, vec!["batcher.rs"]);
    }
}
