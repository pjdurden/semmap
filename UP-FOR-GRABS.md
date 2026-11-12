# Up for grabs: semmap

I started building this, then stopped. I'm not going to work on it any more,
so if you have the time, take it. It's MIT-licensed: fork it, rename it, ship
it, no need to ask.

## The idea in one line

Auto-maintained per-directory map files (`_map.md`) that tell a coding agent
where things live in a repo, regenerated only for the directories a merged PR
touched, so they never rot the way hand-written READMEs do.

## Why it might matter

An agent on an unfamiliar repo spends most of its tokens on orientation:
grepping, reading files to learn what they do, and rebuilding structure that
every earlier session already rebuilt. Hand-written per-directory docs would
fix that, but they go stale, and a stale description is worse than none.
semmap's angle against a crowded field (graph/vector context engines) is
plain files, CI-generated, with content-hash staleness detection and symbol
anchors validated against the real code.

## What exists (Rust)

- Directory walk with vendored-path filtering, processed deepest-first.
- Tree-sitter symbol extraction for Rust, Python, Go and TypeScript.
- `_map.md` render and parse with round-trip fidelity.
- Anchor validation and content-hash staleness detection.
- An LLM trait with an HTTP transport, and bottom-up orchestration that skips
  unchanged directories by hash, plus the `mapgen` CLI.
- An eval harness (`evals/questions.json`) that measures tokens-to-correct-answer
  with and without maps, and an offline demo that runs without an API key.

Spec: [`docs/superpowers/specs/2026-07-20-semmap-design.md`](docs/superpowers/specs/2026-07-20-semmap-design.md).
Plan: `docs/superpowers/plans/`.

## What's left

- Actually run the eval on real repos and publish the with-and-without numbers.
  This is the kill-or-continue gate.
- The CI integration from the spec: a GitHub Action that regenerates maps on
  merge with the repo owner's own key and commits them to an orphan branch.
- A SessionStart hook so agents load the relevant maps automatically.

## Where to start

`cargo test`, then run the offline demo test to see the pipeline end to end.
Run the eval next. If maps don't cut tokens-to-answer meaningfully, stop
there.
