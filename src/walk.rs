use anyhow::Result;
use ignore::WalkBuilder;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDir {
    pub rel_path: String,
    pub files: Vec<String>,
    pub subdirs: Vec<String>,
}

const DEFAULT_IGNORES: &[&str] = &[
    "node_modules",
    "dist",
    "build",
    "target",
    ".venv",
    "venv",
    "__pycache__",
    ".git",
    "vendor",
];

pub const SOURCE_EXTENSIONS: &[&str] = &["rs", "py", "go", "ts", "tsx", "js", "jsx"];

fn is_source_file(name: &str) -> bool {
    match name.rsplit_once('.') {
        Some((_, ext)) => SOURCE_EXTENSIONS.contains(&ext),
        None => false,
    }
}

fn is_ignored_component(name: &str) -> bool {
    DEFAULT_IGNORES.contains(&name)
}

pub fn walk(root: &Path) -> Result<Vec<SourceDir>> {
    let mut files_by_dir: BTreeMap<String, Vec<String>> = BTreeMap::new();

    let walker = WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .add_custom_ignore_filename(".semmapignore")
        .build();

    for entry in walker {
        let entry = entry?;
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");

        if rel.split('/').any(is_ignored_component) {
            continue;
        }
        let (dir, name) = match rel.rsplit_once('/') {
            Some((d, n)) => (d.to_string(), n.to_string()),
            None => (String::new(), rel.clone()),
        };
        if !is_source_file(&name) {
            continue;
        }
        files_by_dir.entry(dir).or_default().push(name);
    }

    // Every ancestor of a source directory is itself a mapped directory.
    let mut all_dirs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for dir in files_by_dir.keys() {
        let mut current = dir.clone();
        loop {
            all_dirs.entry(current.clone()).or_default();
            match current.rsplit_once('/') {
                Some((parent, _)) => current = parent.to_string(),
                None => {
                    if current.is_empty() {
                        break;
                    }
                    current = String::new();
                }
            }
        }
    }

    // Attach child directory names to each parent.
    let dir_names: Vec<String> = all_dirs.keys().cloned().collect();
    for dir in &dir_names {
        if dir.is_empty() {
            continue;
        }
        let (parent, name) = match dir.rsplit_once('/') {
            Some((p, n)) => (p.to_string(), n.to_string()),
            None => (String::new(), dir.clone()),
        };
        if let Some(subdirs) = all_dirs.get_mut(&parent) {
            subdirs.push(name);
        }
    }

    let mut out: Vec<SourceDir> = all_dirs
        .into_iter()
        .map(|(rel_path, mut subdirs)| {
            subdirs.sort();
            subdirs.dedup();
            let mut files = files_by_dir.get(&rel_path).cloned().unwrap_or_default();
            files.sort();
            SourceDir {
                rel_path,
                files,
                subdirs,
            }
        })
        .collect();

    // Deepest first, so leaves are generated before the parents that summarize them.
    out.sort_by(|a, b| {
        let depth = |p: &str| {
            if p.is_empty() {
                0
            } else {
                p.matches('/').count() + 1
            }
        };
        depth(&b.rel_path)
            .cmp(&depth(&a.rel_path))
            .then_with(|| a.rel_path.cmp(&b.rel_path))
    });

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> &'static Path {
        Path::new("tests/fixtures/demo")
    }

    #[test]
    fn finds_source_dirs_and_skips_vendored() {
        let dirs = walk(fixture()).unwrap();
        let paths: Vec<&str> = dirs.iter().map(|d| d.rel_path.as_str()).collect();

        assert!(paths.contains(&"src/settlement"), "got {paths:?}");
        assert!(paths.contains(&"src/chain"), "got {paths:?}");
        assert!(paths.contains(&"src"), "got {paths:?}");
        assert!(paths.contains(&""), "root should be present, got {paths:?}");
        assert!(
            !paths.iter().any(|p| p.contains("node_modules")),
            "node_modules must be filtered, got {paths:?}"
        );
    }

    #[test]
    fn leaves_come_before_parents() {
        let dirs = walk(fixture()).unwrap();
        let pos = |p: &str| dirs.iter().position(|d| d.rel_path == p).unwrap();
        assert!(pos("src/settlement") < pos("src"));
        assert!(pos("src") < pos(""));
    }

    #[test]
    fn records_files_and_subdirs() {
        let dirs = walk(fixture()).unwrap();
        let settlement = dirs.iter().find(|d| d.rel_path == "src/settlement").unwrap();
        assert_eq!(settlement.files, vec!["batcher.rs", "builder.rs"]);
        assert!(settlement.subdirs.is_empty());

        let src = dirs.iter().find(|d| d.rel_path == "src").unwrap();
        assert_eq!(src.subdirs, vec!["chain", "settlement"]);
    }
}
