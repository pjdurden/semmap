use anyhow::{anyhow, Context, Result};
use clap::Parser;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
struct Question {
    id: String,
    question: String,
    expect_substrings: Vec<String>,
}

#[derive(Debug, Default, Clone, PartialEq)]
struct Score {
    correct: bool,
    input_tokens: u64,
    output_tokens: u64,
}

fn score_answer(q: &Question, answer: &str) -> bool {
    let hay = answer.to_lowercase();
    q.expect_substrings
        .iter()
        .all(|needle| hay.contains(&needle.to_lowercase()))
}

fn summarize(with_map: &[Score], without_map: &[Score]) -> String {
    let acc = |s: &[Score]| s.iter().filter(|x| x.correct).count();
    let tokens = |s: &[Score]| -> u64 { s.iter().map(|x| x.input_tokens + x.output_tokens).sum() };

    let with_tokens = tokens(with_map);
    let without_tokens = tokens(without_map);

    let delta_pct = if without_tokens == 0 {
        0.0
    } else {
        (without_tokens as f64 - with_tokens as f64) / without_tokens as f64 * 100.0
    };

    let direction = if delta_pct >= 0.0 {
        format!("{delta_pct:.0}% fewer tokens with the map")
    } else {
        format!("{:.0}% MORE tokens with the map", delta_pct.abs())
    };

    format!(
        "accuracy with map:    {}/{}\n\
         accuracy without map: {}/{}\n\
         tokens with map:      {with_tokens}\n\
         tokens without map:   {without_tokens}\n\
         result:               {direction}\n",
        acc(with_map),
        with_map.len(),
        acc(without_map),
        without_map.len(),
    )
}

#[derive(Parser)]
#[command(
    name = "eval",
    about = "Measure tokens-to-correct-answer with and without maps"
)]
struct Args {
    /// Repository the questions are about
    #[arg(long, default_value = ".")]
    repo: PathBuf,

    /// Map tree produced by mapgen
    #[arg(long)]
    maps: PathBuf,

    /// Question set
    #[arg(long, default_value = "evals/questions.json")]
    questions: PathBuf,

    /// Model used to answer questions
    #[arg(long, default_value = "claude-opus-4-8")]
    model: String,
}

/// Concatenate every `_map.md` in the tree. This is the context an agent gets
/// in the with-map arm; the without-map arm gets a bare file listing instead.
fn collect_maps(dir: &PathBuf) -> Result<String> {
    let mut out = String::new();
    for entry in ignore::WalkBuilder::new(dir).hidden(false).build().flatten() {
        if entry.file_name() == "_map.md" {
            out.push_str(&std::fs::read_to_string(entry.path())?);
            out.push('\n');
        }
    }
    Ok(out)
}

fn collect_file_listing(repo: &PathBuf) -> Result<String> {
    let mut out = String::new();
    for entry in ignore::WalkBuilder::new(repo).hidden(false).build().flatten() {
        if entry.file_type().is_some_and(|t| t.is_file()) {
            let rel = entry.path().strip_prefix(repo).unwrap_or(entry.path());
            out.push_str(&rel.to_string_lossy());
            out.push('\n');
        }
    }
    Ok(out)
}

/// Returns the score (with `correct` left false for the caller to fill in,
/// since only the caller holds the question) alongside the answer text.
fn ask(api_key: &str, model: &str, context: &str, question: &str) -> Result<(Score, String)> {
    let client = reqwest::blocking::Client::new();
    let prompt = format!(
        "Answer using only the context below. Name the specific file and symbol.\n\n\
         CONTEXT:\n{context}\n\nQUESTION: {question}"
    );

    let resp: Value = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&json!({
            "model": model,
            "max_tokens": 1024,
            "messages": [{ "role": "user", "content": prompt }]
        }))
        .send()?
        .error_for_status()?
        .json()?;

    let text = resp["content"]
        .as_array()
        .and_then(|b| b.iter().find(|x| x["type"] == "text"))
        .and_then(|x| x["text"].as_str())
        .ok_or_else(|| anyhow!("no text block in eval response"))?
        .to_string();

    Ok((
        Score {
            correct: false,
            input_tokens: resp["usage"]["input_tokens"].as_u64().unwrap_or(0),
            output_tokens: resp["usage"]["output_tokens"].as_u64().unwrap_or(0),
        },
        text,
    ))
}

fn main() -> Result<()> {
    let args = Args::parse();
    let api_key = std::env::var("ANTHROPIC_API_KEY").context("ANTHROPIC_API_KEY is not set")?;

    let questions: Vec<Question> =
        serde_json::from_str(&std::fs::read_to_string(&args.questions)?)?;

    let map_context = collect_maps(&args.maps)?;
    let listing_context = collect_file_listing(&args.repo)?;

    let mut with_map = Vec::new();
    let mut without_map = Vec::new();

    for q in &questions {
        let (mut s, answer) = ask(&api_key, &args.model, &map_context, &q.question)?;
        s.correct = score_answer(q, &answer);
        println!("[with map]    {} correct={}", q.id, s.correct);
        with_map.push(s);

        let (mut s, answer) = ask(&api_key, &args.model, &listing_context, &q.question)?;
        s.correct = score_answer(q, &answer);
        println!("[without map] {} correct={}", q.id, s.correct);
        without_map.push(s);
    }

    print!("\n{}", summarize(&with_map, &without_map));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q() -> Question {
        Question {
            id: "q1".into(),
            question: "where?".into(),
            expect_substrings: vec!["parse.rs".into(), "extract".into()],
        }
    }

    #[test]
    fn answer_is_correct_only_when_all_substrings_present() {
        assert!(score_answer(&q(), "Look at src/parse.rs, the extract function."));
        assert!(!score_answer(&q(), "Look at src/parse.rs."));
        assert!(!score_answer(&q(), "No idea."));
    }

    #[test]
    fn scoring_is_case_insensitive() {
        assert!(score_answer(&q(), "SRC/PARSE.RS defines EXTRACT"));
    }

    #[test]
    fn summary_reports_accuracy_and_token_delta_in_both_directions() {
        let with = vec![
            Score {
                correct: true,
                input_tokens: 1000,
                output_tokens: 100,
            },
            Score {
                correct: true,
                input_tokens: 1000,
                output_tokens: 100,
            },
        ];
        let without = vec![
            Score {
                correct: true,
                input_tokens: 5000,
                output_tokens: 200,
            },
            Score {
                correct: false,
                input_tokens: 5000,
                output_tokens: 200,
            },
        ];
        let s = summarize(&with, &without);
        assert!(s.contains("2/2"), "{s}");
        assert!(s.contains("1/2"), "{s}");
        // 8200 saved of 10400 total = 78.8%, rendered at zero decimals as 79%.
        assert!(s.contains("79"), "expected a ~79% token reduction figure: {s}");
    }

    #[test]
    fn summary_reports_a_regression_honestly() {
        let with = vec![Score {
            correct: true,
            input_tokens: 9000,
            output_tokens: 100,
        }];
        let without = vec![Score {
            correct: true,
            input_tokens: 3000,
            output_tokens: 100,
        }];
        let s = summarize(&with, &without);
        assert!(
            s.contains('-') || s.to_lowercase().contains("more"),
            "a token increase must be reported, not hidden: {s}"
        );
    }
}
