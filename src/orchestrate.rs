use crate::generate::{assemble, request_for};
use crate::hash::{freshness, hash_dir, Freshness};
use crate::llm::Llm;
use crate::map::{render, DirMap};
use crate::parse::extract;
use crate::walk::walk;
use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct RunStats {
    pub generated: usize,
    pub skipped_unchanged: usize,
    pub dropped: usize,
}

fn read_dir_contents(dir: &Path, files: &[String]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for name in files {
        if let Ok(text) = fs::read_to_string(dir.join(name)) {
            out.insert(name.clone(), text);
        }
    }
    out
}

pub fn run(
    root: &Path,
    out_dir: &Path,
    derived_from: &str,
    llm: &dyn Llm,
    existing: &BTreeMap<String, DirMap>,
) -> Result<(BTreeMap<String, DirMap>, RunStats)> {
    // walk() returns deepest-first, so a directory's children are always
    // finished before the directory itself is described.
    let dirs = walk(root)?;
    let mut finished: BTreeMap<String, DirMap> = BTreeMap::new();
    let mut regenerated: BTreeSet<String> = BTreeSet::new();
    let mut stats = RunStats::default();

    for dir in &dirs {
        let abs = root.join(&dir.rel_path);
        let contents = read_dir_contents(&abs, &dir.files);
        let current_hashes = hash_dir(&contents);

        let child_path = |name: &str| {
            if dir.rel_path.is_empty() {
                name.to_string()
            } else {
                format!("{}/{}", dir.rel_path, name)
            }
        };

        // A parent embeds its children's summaries, so a regenerated child makes
        // its ancestors stale even when their own files are untouched.
        let child_changed = dir
            .subdirs
            .iter()
            .any(|name| regenerated.contains(&child_path(name)));

        // Content-hash short-circuit: an unchanged directory with unchanged
        // children keeps its map and costs nothing. This is what makes
        // incremental runs nearly free.
        if let Some(prev) = existing.get(&dir.rel_path) {
            if !child_changed && freshness(prev, &current_hashes) == Freshness::Fresh {
                finished.insert(dir.rel_path.clone(), prev.clone());
                stats.skipped_unchanged += 1;
                continue;
            }
        }

        let mut symbols = Vec::new();
        for (name, text) in &contents {
            symbols.extend(extract(name, text));
        }

        // Parents summarize children's maps, never children's source.
        let child_summaries: Vec<(String, String)> = dir
            .subdirs
            .iter()
            .filter_map(|name| {
                finished
                    .get(&child_path(name))
                    .map(|m| (name.clone(), m.what.clone()))
            })
            .collect();

        let custom_id = if dir.rel_path.is_empty() {
            "root".to_string()
        } else {
            dir.rel_path.replace('/', "__")
        };

        let request = request_for(
            &custom_id,
            &dir.rel_path,
            &symbols,
            &dir.files,
            &child_summaries,
        );
        let responses = llm.describe(vec![request])?;

        let Some(response) = responses.into_iter().find(|r| r.custom_id == custom_id) else {
            // Fail closed: no usable response means no map for this directory.
            stats.dropped += 1;
            continue;
        };

        let map = assemble(&dir.rel_path, derived_from, &symbols, &contents, &response);

        let dest = out_dir.join(&dir.rel_path);
        fs::create_dir_all(&dest)?;
        fs::write(dest.join("_map.md"), render(&map))?;

        finished.insert(dir.rel_path.clone(), map);
        regenerated.insert(dir.rel_path.clone());
        stats.generated += 1;
    }

    Ok((finished, stats))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::fake::FakeLlm;
    use crate::llm::DescribeResponse;
    use std::collections::HashMap;

    fn fixture() -> &'static Path {
        Path::new("tests/fixtures/demo")
    }

    /// A fake that answers every request with a generic description, so the
    /// orchestration itself is what's under test.
    struct EchoLlm;
    impl Llm for EchoLlm {
        fn describe(
            &self,
            reqs: Vec<crate::llm::DescribeRequest>,
        ) -> Result<Vec<DescribeResponse>> {
            Ok(reqs
                .iter()
                .map(|r| DescribeResponse {
                    custom_id: r.custom_id.clone(),
                    what: format!("Directory {}.", r.dir_path),
                    entry_symbol: r.symbol_names.first().cloned(),
                    key_symbols: r
                        .symbol_names
                        .iter()
                        .map(|n| (n.clone(), format!("{n} does things")))
                        .collect(),
                    look_here_for: vec![r.dir_path.clone()],
                    subdir_notes: r
                        .child_summaries
                        .iter()
                        .map(|(n, s)| (n.clone(), s.clone()))
                        .collect(),
                })
                .collect())
        }
    }

    /// Copy the fixture into a writable tempdir so a test can mutate a source file.
    fn copy_fixture(dest: &Path) {
        for entry in ignore::WalkBuilder::new(fixture())
            .hidden(false)
            .build()
            .flatten()
        {
            let rel = entry.path().strip_prefix(fixture()).unwrap();
            let target = dest.join(rel);
            if entry.file_type().is_some_and(|t| t.is_dir()) {
                std::fs::create_dir_all(&target).unwrap();
            } else if entry.file_type().is_some_and(|t| t.is_file()) {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }
                std::fs::copy(entry.path(), &target).unwrap();
            }
        }
    }

    #[test]
    fn generates_a_map_for_every_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let (maps, stats) =
            run(fixture(), tmp.path(), "abc123", &EchoLlm, &BTreeMap::new()).unwrap();

        assert!(maps.contains_key("src/settlement"));
        assert!(maps.contains_key("src"));
        assert!(maps.contains_key(""));
        assert_eq!(stats.generated, maps.len());
    }

    #[test]
    fn parents_receive_child_summaries() {
        let tmp = tempfile::tempdir().unwrap();
        let (maps, _) = run(fixture(), tmp.path(), "abc123", &EchoLlm, &BTreeMap::new()).unwrap();

        let src = &maps["src"];
        let names: Vec<&str> = src.subdirs.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"settlement"), "got {names:?}");
        assert!(names.contains(&"chain"), "got {names:?}");
    }

    #[test]
    fn every_generated_map_passes_anchor_validation() {
        use crate::anchor::validate;

        let tmp = tempfile::tempdir().unwrap();
        let (maps, _) = run(fixture(), tmp.path(), "abc123", &EchoLlm, &BTreeMap::new()).unwrap();

        for (rel, m) in &maps {
            let dir = fixture().join(rel);
            let mut symbols = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for e in entries.flatten() {
                    if e.path().is_file() {
                        let name = e.file_name().to_string_lossy().to_string();
                        if let Ok(src) = std::fs::read_to_string(e.path()) {
                            symbols.extend(extract(&name, &src));
                        }
                    }
                }
            }
            assert_eq!(validate(m, &symbols), vec![], "anchors broken in {rel}");
        }
    }

    #[test]
    fn unchanged_directories_are_skipped_on_a_second_run() {
        let tmp = tempfile::tempdir().unwrap();
        let (first, _) = run(fixture(), tmp.path(), "abc123", &EchoLlm, &BTreeMap::new()).unwrap();
        let (_, stats) = run(fixture(), tmp.path(), "abc123", &EchoLlm, &first).unwrap();

        assert_eq!(stats.generated, 0, "nothing changed, so nothing regenerates");
        assert_eq!(stats.skipped_unchanged, first.len());
    }

    #[test]
    fn incremental_regeneration_converges_with_full_regeneration() {
        let repo = tempfile::tempdir().unwrap();
        copy_fixture(repo.path());

        let out_a = tempfile::tempdir().unwrap();
        let (before, _) =
            run(repo.path(), out_a.path(), "sha1", &EchoLlm, &BTreeMap::new()).unwrap();

        // Change one file in one directory.
        std::fs::write(
            repo.path().join("src/settlement/batcher.rs"),
            "pub struct Batch {\n    pub items: Vec<u64>,\n}\n\npub fn submit_batch() {}\n\npub fn retry_batch() {}\n",
        )
        .unwrap();

        // Incremental: reuse the stale maps as `existing`.
        let out_inc = tempfile::tempdir().unwrap();
        let (incremental, stats) =
            run(repo.path(), out_inc.path(), "sha2", &EchoLlm, &before).unwrap();

        // Full: no prior maps at all.
        let out_full = tempfile::tempdir().unwrap();
        let (full, _) =
            run(repo.path(), out_full.path(), "sha2", &EchoLlm, &BTreeMap::new()).unwrap();

        assert_eq!(
            incremental["src/settlement"], full["src/settlement"],
            "incremental and full generation must agree on a touched directory"
        );
        assert!(
            stats.skipped_unchanged > 0,
            "untouched directories must be skipped, not regenerated"
        );
    }

    /// Counts how many directories are actually sent to the model, which is the
    /// cost driver. A one-file change must not trigger a full-repo rebuild.
    struct CountingLlm {
        calls: std::cell::RefCell<usize>,
    }
    impl Llm for CountingLlm {
        fn describe(
            &self,
            reqs: Vec<crate::llm::DescribeRequest>,
        ) -> Result<Vec<DescribeResponse>> {
            *self.calls.borrow_mut() += reqs.len();
            EchoLlm.describe(reqs)
        }
    }

    #[test]
    fn a_one_file_change_regenerates_only_the_touched_directory_and_its_ancestors() {
        let repo = tempfile::tempdir().unwrap();
        copy_fixture(repo.path());

        let out = tempfile::tempdir().unwrap();
        let (before, _) = run(repo.path(), out.path(), "sha1", &EchoLlm, &BTreeMap::new()).unwrap();

        std::fs::write(
            repo.path().join("src/settlement/batcher.rs"),
            "pub fn only_this_changed() {}\n",
        )
        .unwrap();

        let counting = CountingLlm {
            calls: std::cell::RefCell::new(0),
        };
        let out2 = tempfile::tempdir().unwrap();
        run(repo.path(), out2.path(), "sha2", &counting, &before).unwrap();

        // src/settlement changed, so src and the root must be re-rolled-up to
        // pick up its new summary. src/chain is untouched and must be skipped.
        assert_eq!(
            *counting.calls.borrow(),
            3,
            "expected src/settlement + src + root, and NOT src/chain"
        );
    }

    #[test]
    fn writes_map_files_to_disk() {
        let tmp = tempfile::tempdir().unwrap();
        run(fixture(), tmp.path(), "abc123", &EchoLlm, &BTreeMap::new()).unwrap();
        assert!(tmp.path().join("src/settlement/_map.md").exists());
        assert!(tmp.path().join("_map.md").exists());
    }

    #[test]
    fn dropped_responses_are_counted_and_omit_the_directory() {
        let tmp = tempfile::tempdir().unwrap();
        // FakeLlm with no canned responses returns nothing for every request.
        let empty = FakeLlm {
            responses: HashMap::new(),
        };
        let (maps, stats) = run(fixture(), tmp.path(), "abc123", &empty, &BTreeMap::new()).unwrap();

        assert!(maps.is_empty(), "no responses means no maps");
        assert!(stats.dropped > 0);
        assert_eq!(stats.generated, 0);
    }
}
