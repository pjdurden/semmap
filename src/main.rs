use anyhow::{Context, Result};
use clap::Parser;
use semmap::llm::http::HttpLlm;
use semmap::map::parse as parse_map;
use semmap::orchestrate::run;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "mapgen", about = "Generate navigation maps for a code repository")]
struct Args {
    /// Repository root to map
    #[arg(long, default_value = ".")]
    root: PathBuf,

    /// Directory to write the mirrored map tree into
    #[arg(long)]
    out: PathBuf,

    /// Commit SHA these maps are derived from
    #[arg(long)]
    derived_from: String,

    /// Model for directory descriptions
    #[arg(long, default_value = "claude-haiku-4-5")]
    model: String,

    /// Effort level. Leave unset for claude-haiku-4-5, which rejects it.
    #[arg(long)]
    effort: Option<String>,

    /// Use the Batch API (50% cheaper, minutes of latency instead of seconds)
    #[arg(long)]
    batch: bool,
}

/// Load any maps already present in the output tree so unchanged directories
/// can be skipped instead of regenerated.
fn load_existing(out: &PathBuf) -> BTreeMap<String, semmap::map::DirMap> {
    let mut found = BTreeMap::new();
    let walker = ignore::WalkBuilder::new(out).hidden(false).build();
    for entry in walker.flatten() {
        if entry.file_name() != "_map.md" {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        if let Ok(map) = parse_map(&text) {
            found.insert(map.path.clone(), map);
        }
    }
    found
}

fn main() -> Result<()> {
    let args = Args::parse();
    let api_key = std::env::var("ANTHROPIC_API_KEY").context("ANTHROPIC_API_KEY is not set")?;

    let llm = HttpLlm {
        api_key,
        model: args.model.clone(),
        effort: args.effort.clone(),
        use_batch: args.batch,
    };

    let existing = load_existing(&args.out);
    let (maps, stats) = run(&args.root, &args.out, &args.derived_from, &llm, &existing)?;

    println!(
        "{} generated, {} unchanged, {} dropped, {} maps total",
        stats.generated,
        stats.skipped_unchanged,
        stats.dropped,
        maps.len()
    );
    if stats.dropped > 0 {
        eprintln!("warning: {} directories have no map this run", stats.dropped);
    }
    Ok(())
}
