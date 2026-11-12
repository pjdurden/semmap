//! Runs the full mapgen pipeline over this repository with a deterministic
//! stand-in for the model, so the walk, parse, assemble, render and anchor
//! validation steps can be exercised without an API key.
//!
//! The prose is mechanical placeholder text. Everything else — the anchors,
//! the tree shape, the rollup order, the hashes — is exactly what a real run
//! produces.
//!
//! Usage: cargo run --example offline_demo

use anyhow::Result;
use semmap::anchor::validate;
use semmap::llm::{DescribeRequest, DescribeResponse, Llm};
use semmap::map::render;
use semmap::orchestrate::run;
use semmap::parse::extract;
use std::collections::BTreeMap;
use std::path::Path;

/// Stands in for the model. Produces deterministic prose from the symbol
/// names it is given, and deliberately also claims one symbol that does not
/// exist, to show that fabricated names are dropped rather than written out.
struct StubDescriber;

impl Llm for StubDescriber {
    fn describe(&self, reqs: Vec<DescribeRequest>) -> Result<Vec<DescribeResponse>> {
        Ok(reqs
            .iter()
            .map(|r| {
                let label = if r.dir_path.is_empty() {
                    "the repository root".to_string()
                } else {
                    format!("`{}`", r.dir_path)
                };

                let mut key_symbols: Vec<(String, String)> = r
                    .symbol_names
                    .iter()
                    .take(4)
                    .map(|n| (n.clone(), format!("[placeholder prose for {n}]")))
                    .collect();
                // A name the parser never saw. assemble() must drop this.
                key_symbols.push((
                    "totally_fabricated_symbol".to_string(),
                    "[this should never appear in the output]".to_string(),
                ));

                DescribeResponse {
                    custom_id: r.custom_id.clone(),
                    what: format!(
                        "[placeholder] Code in {label}: {} file(s), {} symbol(s).",
                        r.file_names.len(),
                        r.symbol_names.len()
                    ),
                    entry_symbol: r.symbol_names.first().cloned(),
                    key_symbols,
                    look_here_for: r
                        .symbol_names
                        .iter()
                        .take(3)
                        .map(|s| s.to_lowercase())
                        .collect(),
                    subdir_notes: r
                        .child_summaries
                        .iter()
                        .map(|(n, s)| (n.clone(), s.clone()))
                        .collect(),
                }
            })
            .collect())
    }
}

fn main() -> Result<()> {
    let repo = Path::new(".");
    let out = Path::new("/tmp/semmap-offline-demo");
    let _ = std::fs::remove_dir_all(out);

    let (maps, stats) = run(repo, out, "offline-demo", &StubDescriber, &BTreeMap::new())?;

    println!("=== run stats ===");
    println!(
        "{} generated, {} unchanged, {} dropped\n",
        stats.generated, stats.skipped_unchanged, stats.dropped
    );

    println!("=== generated tree ===");
    let mut paths: Vec<&String> = maps.keys().collect();
    paths.sort();
    for p in &paths {
        let shown = if p.is_empty() { "<root>" } else { p };
        println!("  {shown}/_map.md");
    }

    // Validate every anchor in every generated map against freshly parsed source.
    println!("\n=== anchor validation ===");
    let mut total_anchors = 0usize;
    let mut violations = 0usize;
    for (rel, m) in &maps {
        let dir = repo.join(rel);
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
        total_anchors += m.key_symbols.len() + usize::from(m.entry.is_some());
        let v = validate(m, &symbols);
        if !v.is_empty() {
            violations += v.len();
            for one in v {
                println!("  VIOLATION in {rel}: {}", one.reason);
            }
        }
    }
    println!("  {total_anchors} anchors checked, {violations} violations");

    // Show one real map in full.
    println!("\n=== src/llm/_map.md (verbatim) ===");
    if let Some(m) = maps.get("src/llm") {
        print!("{}", render(m));
    }

    println!("\n=== fabrication check ===");
    let fabricated = maps
        .values()
        .flat_map(|m| m.key_symbols.iter())
        .filter(|(a, _)| a.name == "totally_fabricated_symbol")
        .count();
    println!(
        "  the stub claimed `totally_fabricated_symbol` in all {} directories; \
         {fabricated} survived into the maps",
        maps.len()
    );

    Ok(())
}
