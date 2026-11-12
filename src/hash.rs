use crate::map::DirMap;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Freshness {
    Fresh,
    Stale(Vec<String>),
}

pub fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let digest = hasher.finalize();
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

pub fn hash_dir(files: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    files
        .iter()
        .map(|(name, content)| (name.clone(), hash_content(content)))
        .collect()
}

pub fn freshness(map: &DirMap, current: &BTreeMap<String, String>) -> Freshness {
    let mut changed = Vec::new();

    for (file, recorded) in &map.hashes {
        match current.get(file) {
            Some(now) if now == recorded => {}
            Some(_) => changed.push(file.clone()),
            None => changed.push(file.clone()),
        }
    }
    for file in current.keys() {
        if !map.hashes.contains_key(file) {
            changed.push(file.clone());
        }
    }

    changed.sort();
    changed.dedup();
    if changed.is_empty() {
        Freshness::Fresh
    } else {
        Freshness::Stale(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map_with_hashes(pairs: &[(&str, &str)]) -> DirMap {
        let mut hashes = BTreeMap::new();
        for (f, h) in pairs {
            hashes.insert(f.to_string(), h.to_string());
        }
        DirMap {
            hashes,
            ..Default::default()
        }
    }

    fn current(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(f, h)| (f.to_string(), h.to_string()))
            .collect()
    }

    #[test]
    fn hash_is_stable_and_16_hex_chars() {
        let a = hash_content("fn main() {}\n");
        assert_eq!(a, hash_content("fn main() {}\n"));
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, hash_content("fn main() { }\n"));
    }

    #[test]
    fn identical_hashes_are_fresh() {
        let m = map_with_hashes(&[("a.rs", "1111"), ("b.rs", "2222")]);
        assert_eq!(
            freshness(&m, &current(&[("a.rs", "1111"), ("b.rs", "2222")])),
            Freshness::Fresh
        );
    }

    #[test]
    fn changed_file_is_stale() {
        let m = map_with_hashes(&[("a.rs", "1111")]);
        assert_eq!(
            freshness(&m, &current(&[("a.rs", "9999")])),
            Freshness::Stale(vec!["a.rs".to_string()])
        );
    }

    #[test]
    fn added_file_is_stale() {
        let m = map_with_hashes(&[("a.rs", "1111")]);
        assert_eq!(
            freshness(&m, &current(&[("a.rs", "1111"), ("new.rs", "3333")])),
            Freshness::Stale(vec!["new.rs".to_string()])
        );
    }

    #[test]
    fn removed_file_is_stale() {
        let m = map_with_hashes(&[("a.rs", "1111"), ("gone.rs", "2222")]);
        assert_eq!(
            freshness(&m, &current(&[("a.rs", "1111")])),
            Freshness::Stale(vec!["gone.rs".to_string()])
        );
    }

    #[test]
    fn hash_dir_hashes_every_file() {
        let mut files = BTreeMap::new();
        files.insert("a.rs".to_string(), "one".to_string());
        files.insert("b.rs".to_string(), "two".to_string());
        let out = hash_dir(&files);
        assert_eq!(out.len(), 2);
        assert_eq!(out.get("a.rs").unwrap(), &hash_content("one"));
    }
}
