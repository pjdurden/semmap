#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Struct,
    Class,
    Enum,
    Trait,
    Interface,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub file: String,
    pub line: usize,
}

use tree_sitter::{Language, Parser, Query, QueryCursor};

const RUST_QUERY: &str = r#"
(function_item name: (identifier) @name) @fn
(struct_item name: (type_identifier) @name) @struct
(enum_item name: (type_identifier) @name) @enum
(trait_item name: (type_identifier) @name) @trait
"#;

const PYTHON_QUERY: &str = r#"
(function_definition name: (identifier) @name) @fn
(class_definition name: (identifier) @name) @class
"#;

const GO_QUERY: &str = r#"
(function_declaration name: (identifier) @name) @fn
(method_declaration name: (field_identifier) @name) @fn
(type_declaration (type_spec name: (type_identifier) @name)) @struct
"#;

const TS_QUERY: &str = r#"
(function_declaration name: (identifier) @name) @fn
(class_declaration name: (type_identifier) @name) @class
(interface_declaration name: (type_identifier) @name) @interface
"#;

fn language_for(file_name: &str) -> Option<(Language, &'static str)> {
    let ext = file_name.rsplit_once('.')?.1;
    match ext {
        "rs" => Some((tree_sitter_rust::language(), RUST_QUERY)),
        "py" => Some((tree_sitter_python::language(), PYTHON_QUERY)),
        "go" => Some((tree_sitter_go::language(), GO_QUERY)),
        "ts" | "js" => Some((tree_sitter_typescript::language_typescript(), TS_QUERY)),
        "tsx" | "jsx" => Some((tree_sitter_typescript::language_tsx(), TS_QUERY)),
        _ => None,
    }
}

fn kind_for(capture_name: &str) -> Option<SymbolKind> {
    match capture_name {
        "fn" => Some(SymbolKind::Function),
        "struct" => Some(SymbolKind::Struct),
        "class" => Some(SymbolKind::Class),
        "enum" => Some(SymbolKind::Enum),
        "trait" => Some(SymbolKind::Trait),
        "interface" => Some(SymbolKind::Interface),
        _ => None,
    }
}

pub fn extract(file_name: &str, source: &str) -> Vec<Symbol> {
    let Some((language, query_src)) = language_for(file_name) else {
        return Vec::new();
    };

    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };
    let Ok(query) = Query::new(&language, query_src) else {
        return Vec::new();
    };

    let capture_names = query.capture_names();
    let mut cursor = QueryCursor::new();
    let mut out = Vec::new();

    for m in cursor.matches(&query, tree.root_node(), source.as_bytes()) {
        // Each match has a @name capture (the identifier) and one kind capture
        // (@fn / @struct / ...). The kind capture also carries the definition's
        // start line, which is what we anchor to.
        let mut name: Option<String> = None;
        let mut kind: Option<SymbolKind> = None;
        let mut line: Option<usize> = None;

        for cap in m.captures {
            let cap_name = capture_names[cap.index as usize];
            if cap_name == "name" {
                name = cap
                    .node
                    .utf8_text(source.as_bytes())
                    .ok()
                    .map(|s| s.to_string());
            } else if let Some(k) = kind_for(cap_name) {
                kind = Some(k);
                // tree-sitter rows are 0-indexed; anchors are 1-indexed.
                line = Some(cap.node.start_position().row + 1);
            }
        }

        if let (Some(name), Some(kind), Some(line)) = (name, kind, line) {
            out.push(Symbol {
                name,
                kind,
                file: file_name.to_string(),
                line,
            });
        }
    }

    out.sort_by(|a, b| a.line.cmp(&b.line).then_with(|| a.name.cmp(&b.name)));
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_rust_symbols_with_one_indexed_lines() {
        let src = "pub struct Batch {\n    pub items: Vec<u64>,\n}\n\npub fn submit_batch() {}\n";
        let syms = extract("batcher.rs", src);

        let batch = syms.iter().find(|s| s.name == "Batch").unwrap();
        assert_eq!(batch.kind, SymbolKind::Struct);
        assert_eq!(batch.line, 1, "lines must be 1-indexed");
        assert_eq!(batch.file, "batcher.rs");

        let submit = syms.iter().find(|s| s.name == "submit_batch").unwrap();
        assert_eq!(submit.kind, SymbolKind::Function);
        assert_eq!(submit.line, 5);
    }

    #[test]
    fn extracts_python_symbols() {
        let src = "class RpcClient:\n    def call(self):\n        pass\n\n\ndef connect(url):\n    return 1\n";
        let syms = extract("rpc.py", src);
        assert_eq!(
            syms.iter().find(|s| s.name == "RpcClient").unwrap().kind,
            SymbolKind::Class
        );
        assert_eq!(syms.iter().find(|s| s.name == "connect").unwrap().line, 6);
    }

    #[test]
    fn extracts_go_symbols() {
        let src = "package main\n\ntype Client struct{}\n\nfunc Connect() {}\n";
        let syms = extract("rpc.go", src);
        assert_eq!(syms.iter().find(|s| s.name == "Connect").unwrap().line, 5);
        assert!(syms.iter().any(|s| s.name == "Client"));
    }

    #[test]
    fn extracts_typescript_symbols() {
        let src = "export interface Batch { id: string }\n\nexport function submit() {}\n";
        let syms = extract("batcher.ts", src);
        assert_eq!(
            syms.iter().find(|s| s.name == "Batch").unwrap().kind,
            SymbolKind::Interface
        );
        assert_eq!(syms.iter().find(|s| s.name == "submit").unwrap().line, 3);
    }

    #[test]
    fn unsupported_extension_returns_empty() {
        assert!(extract("README.md", "# hello\n").is_empty());
    }
}
