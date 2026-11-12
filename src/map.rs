use anyhow::{anyhow, Context, Result};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    pub name: String,
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DirMap {
    pub path: String,
    pub derived_from: String,
    pub what: String,
    pub entry: Option<Anchor>,
    pub key_symbols: Vec<(Anchor, String)>,
    pub depends_on: Vec<(String, String)>,
    pub look_here_for: Vec<String>,
    pub subdirs: Vec<(String, String)>,
    pub hashes: BTreeMap<String, String>,
}

fn render_anchor(a: &Anchor) -> String {
    format!("{} ({}:{})", a.name, a.file, a.line)
}

fn parse_anchor(s: &str) -> Result<Anchor> {
    let (name, rest) = s
        .split_once(" (")
        .ok_or_else(|| anyhow!("anchor missing ' (': {s}"))?;
    let inner = rest
        .strip_suffix(')')
        .ok_or_else(|| anyhow!("anchor missing ')': {s}"))?;
    let (file, line) = inner
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("anchor missing ':line': {s}"))?;
    Ok(Anchor {
        name: name.trim().to_string(),
        file: file.trim().to_string(),
        line: line.trim().parse().context("anchor line is not a number")?,
    })
}

pub fn render(map: &DirMap) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n", map.path));
    out.push_str(&format!("derived-from: {}\n\n", map.derived_from));
    out.push_str(&format!("What: {}\n", map.what));

    if let Some(entry) = &map.entry {
        out.push_str(&format!("Entry: {}\n", render_anchor(entry)));
    }

    if !map.key_symbols.is_empty() {
        out.push_str("Key symbols:\n");
        for (anchor, desc) in &map.key_symbols {
            out.push_str(&format!("  {}  {}\n", render_anchor(anchor), desc));
        }
    }

    if !map.depends_on.is_empty() {
        let joined: Vec<String> = map
            .depends_on
            .iter()
            .map(|(p, label)| format!("{p} ({label})"))
            .collect();
        out.push_str(&format!("Depends on: {}\n", joined.join(", ")));
    }

    if !map.look_here_for.is_empty() {
        out.push_str(&format!("Look here for: {}\n", map.look_here_for.join(", ")));
    }

    if !map.subdirs.is_empty() {
        let joined: Vec<String> = map
            .subdirs
            .iter()
            .map(|(name, desc)| format!("./{name} ({desc})"))
            .collect();
        out.push_str(&format!("Subdirs: {}\n", joined.join(", ")));
    }

    out.push_str("Hashes:\n");
    for (file, hash) in &map.hashes {
        out.push_str(&format!("  {file} {hash}\n"));
    }

    out
}

fn split_labeled_list(value: &str) -> Vec<(String, String)> {
    // "src/chain (RPC), src/ledger (balances)" -> [("src/chain","RPC"), ...]
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    for ch in value.chars() {
        match ch {
            '(' => {
                depth += 1;
                current.push(ch);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut current));
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        out.push(current);
    }

    out.into_iter()
        .filter_map(|item| {
            let item = item.trim();
            let (name, rest) = item.split_once(" (")?;
            let desc = rest.strip_suffix(')')?;
            Some((name.trim().to_string(), desc.trim().to_string()))
        })
        .collect()
}

pub fn parse(text: &str) -> Result<DirMap> {
    let mut map = DirMap::default();
    let mut section: Option<&'static str> = None;
    let mut saw_header = false;

    for line in text.lines() {
        // Indented lines belong to the section opened by the last unindented header.
        if line.starts_with("  ") {
            let body = line.trim();
            match section {
                Some("key_symbols") => {
                    let (anchor_part, desc) = body
                        .split_once(")  ")
                        .ok_or_else(|| anyhow!("key symbol line missing ')  ' separator: {body}"))?;
                    let anchor = parse_anchor(&format!("{anchor_part})"))?;
                    map.key_symbols.push((anchor, desc.trim().to_string()));
                }
                Some("hashes") => {
                    let (file, hash) = body
                        .split_once(' ')
                        .ok_or_else(|| anyhow!("hash line missing space: {body}"))?;
                    map.hashes.insert(file.to_string(), hash.trim().to_string());
                }
                _ => return Err(anyhow!("indented line outside a section: {body}")),
            }
            continue;
        }

        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }

        if let Some(path) = line.strip_prefix("# ") {
            map.path = path.trim().to_string();
            saw_header = true;
            section = None;
        } else if let Some(sha) = line.strip_prefix("derived-from: ") {
            map.derived_from = sha.trim().to_string();
            section = None;
        } else if let Some(what) = line.strip_prefix("What: ") {
            map.what = what.trim().to_string();
            section = None;
        } else if let Some(entry) = line.strip_prefix("Entry: ") {
            map.entry = Some(parse_anchor(entry.trim())?);
            section = None;
        } else if line == "Key symbols:" {
            section = Some("key_symbols");
        } else if line == "Hashes:" {
            section = Some("hashes");
        } else if let Some(deps) = line.strip_prefix("Depends on: ") {
            map.depends_on = split_labeled_list(deps);
            section = None;
        } else if let Some(look) = line.strip_prefix("Look here for: ") {
            map.look_here_for = look.split(',').map(|s| s.trim().to_string()).collect();
            section = None;
        } else if let Some(subs) = line.strip_prefix("Subdirs: ") {
            map.subdirs = split_labeled_list(subs)
                .into_iter()
                .map(|(name, desc)| (name.trim_start_matches("./").to_string(), desc))
                .collect();
            section = None;
        } else {
            return Err(anyhow!("unrecognized line in map file: {line}"));
        }
    }

    if !saw_header {
        return Err(anyhow!("map file has no '# <path>' header"));
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> DirMap {
        let mut hashes = BTreeMap::new();
        hashes.insert("batcher.rs".to_string(), "aaaa1111".to_string());
        hashes.insert("builder.rs".to_string(), "bbbb2222".to_string());
        DirMap {
            path: "src/settlement".to_string(),
            derived_from: "a3f91c2".to_string(),
            what: "Off-chain settlement batching and on-chain submission.".to_string(),
            entry: Some(Anchor {
                name: "submit_batch".to_string(),
                file: "batcher.rs".to_string(),
                line: 5,
            }),
            key_symbols: vec![
                (
                    Anchor {
                        name: "BatchBuilder".to_string(),
                        file: "builder.rs".to_string(),
                        line: 1,
                    },
                    "accumulates transfers up to the gas ceiling".to_string(),
                ),
                (
                    Anchor {
                        name: "reconcile".to_string(),
                        file: "builder.rs".to_string(),
                        line: 11,
                    },
                    "compares on-chain events to the local ledger".to_string(),
                ),
            ],
            depends_on: vec![("src/chain".to_string(), "RPC".to_string())],
            look_here_for: vec!["batch sizing".to_string(), "gas ceilings".to_string()],
            subdirs: vec![("adapters".to_string(), "per-chain RPC quirks".to_string())],
            hashes,
        }
    }

    #[test]
    fn round_trips() {
        let original = sample();
        let text = render(&original);
        let parsed = parse(&text).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn render_is_deterministic() {
        assert_eq!(render(&sample()), render(&sample()));
    }

    #[test]
    fn renders_expected_shape() {
        let text = render(&sample());
        assert!(text.starts_with("# src/settlement\n"), "got:\n{text}");
        assert!(text.contains("derived-from: a3f91c2"), "got:\n{text}");
        assert!(
            text.contains(
                "  BatchBuilder (builder.rs:1)  accumulates transfers up to the gas ceiling"
            ),
            "got:\n{text}"
        );
    }

    #[test]
    fn parses_map_with_no_entry_and_no_subdirs() {
        let text = "# src/leaf\nderived-from: deadbeef\n\nWhat: A leaf.\nKey symbols:\n  foo (a.rs:3)  does foo\nLook here for: foo\nHashes:\n  a.rs 1234\n";
        let m = parse(text).unwrap();
        assert_eq!(m.path, "src/leaf");
        assert!(m.entry.is_none());
        assert!(m.subdirs.is_empty());
        assert_eq!(m.key_symbols.len(), 1);
        assert_eq!(m.hashes.get("a.rs").unwrap(), "1234");
    }

    #[test]
    fn rejects_map_without_header() {
        assert!(parse("What: no header\n").is_err());
    }
}
