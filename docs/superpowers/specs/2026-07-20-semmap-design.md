# semmap — Design Spec

**Date:** 2026-07-20
**Status:** Approved, pre-implementation
**Name:** `semmap` is a placeholder and may change before release.

## Problem

A coding agent starting work on an unfamiliar repository spends most of its
tokens on orientation: grepping for where things live, reading files to learn
what they do, and reconstructing structure that every previous session already
reconstructed. The work is repeated on every session, by every developer, and
none of it is retained.

Hand-written per-directory READMEs solve this in principle. They fail in
practice because they rot: nobody updates them when the code changes, and a
stale description is worse than none because the reader trusts it.

`semmap` generates and maintains that per-directory documentation
automatically, regenerating only what a merged pull request touched.

## What this is not

Explicitly out of scope. Each was considered and cut:

- **Not a team knowledge network.** No capture of session findings, no
  cross-developer sharing, no broadcast or streaming of sessions. Value must be
  delivered to a single developer on a single repo with no colleagues
  participating.
- **Not a vector store or knowledge graph.** No embeddings, no graph database,
  no similarity search. The map is plain files in a directory tree.
- **Not a code review product.** No PR comments, no review verdicts, no
  human-facing rendering in v1.
- **Not a hosted service.** No source code reaches semmap-operated
  infrastructure, ever.
- **Not a replacement for reading code.** The map is an index that tells the
  agent where to look. The agent still reads the code it decides to change.

## Prior art, and the actual differentiator

The category is crowded and well funded:

- **codebase-memory-mcp** — persistent knowledge graph over 158 languages,
  intercepts the agent's Grep/Glob via a `PreToolUse` hook.
- **Augment** — $252M funded; unbundled its Context Engine into a standalone
  MCP server in February 2026; indexes 500k-file repositories.
- **Sourcegraph** — SCIP, a decade of cross-repository code intelligence.
- **Greptile** — semantic graph aimed at automated code review.
- **MCP Indexer** — post-merge incremental reindexing.

These beat semmap on raw retrieval capability and will continue to. semmap
does not compete on capability. It competes on three properties none of them
have:

1. **No new tools.** The map is a directory tree of markdown. The agent
   navigates it with `Read` and `Glob`, which it already uses reflexively.
   There is no MCP server to run, no tool the model has to remember to call,
   and no index to keep in sync with a separate process.
2. **No code egress.** Generation runs in the customer's own CI, on their own
   API key. There is no security review to pass. This is installable at a
   company that would categorically reject a hosted indexer.
3. **Human-readable and human-correctable.** When a description is wrong,
   someone edits the markdown and `git blame` records who fixed it. A wrong
   embedding in a competitor's vector store is invisible and uneditable.

## Architecture

Three components, each independently testable.

### 1. `mapgen` — the generator

A pure function: given a repository path and a diff range, produce a mirrored
tree of `_map.md` files. No network beyond model API calls, no persistent
state, no server. Runs identically in CI and on a developer's machine.

### 2. GitHub Action — the trigger

A thin wrapper invoked on merge to the default branch. It computes the changed
directories from the merge diff, runs `mapgen` over those directories, and
publishes the result. It runs in the customer's CI with the customer's model
API key supplied as a repository secret.

### 3. `semmap sync` — the client

Fetches the published map into `~/.semmap/<org>/<repo>/` and installs a
`SessionStart` hook into the developer's Claude Code configuration.

The fetch on session start is best-effort and bounded by a short timeout. On a
slow network, an unreachable remote, or an offline machine, the client uses the
last cached copy rather than blocking session start. A cached map is still
subject to the hash verification described under *Staleness and correctness*,
so an out-of-date cache degrades to silence rather than to wrong answers.

## Transport

The map is published to an **orphan branch** named `semmap` in the same
repository.

Rationale: it versions the map alongside the code, costs no hosting because it
rides the repository the customer already has, and keeps map churn out of every
pull request diff. Committing a `.semmap/` directory into the working tree was
rejected because it adds noise to every human code review.

Each publish is a single commit whose message records the source commit SHA the
map was derived from.

## Map format

Every directory in the mirrored tree gets exactly one `_map.md`:

```
# src/settlement
derived-from: a3f91c2

What: Off-chain settlement batching and on-chain submission.
Entry: submitBatch() — batcher.ts:142
Key symbols:
  BatchBuilder (builder.ts:30)  accumulates transfers to gas ceiling
  reconcile() (reconcile.ts:88) diffs on-chain events vs local ledger
Depends on: src/chain (RPC), src/ledger (balances)
Look here for: batch sizing, gas ceilings, settlement retries
Subdirs: ./adapters (per-chain RPC quirks), ./types
```

Field definitions:

| Field | Source | Purpose |
|---|---|---|
| `derived-from` | git | Source commit this map was generated at |
| `What` | model | One or two sentences on the directory's responsibility |
| `Entry` | parser + model | The symbol a reader should start from |
| `Key symbols` | parser for anchors, model for descriptions | The handful worth knowing |
| `Depends on` | parser for the import edges, model for the labels | Sibling and parent directories relied upon |
| `Look here for` | model | Query terms that should route a reader here |
| `Subdirs` | filesystem + child maps | One-line summary per child, lifted from the child's `What` |

The format is line-oriented so it diffs cleanly in git and can be grepped
without parsing.

## Data flow

```
PR merges to default branch
  → Action computes changed directories from the merge diff
  → mapgen regenerates _map.md for those directories only
  → rolls up ancestor summaries along the path to the root
  → commits atomically to orphan branch `semmap`, message carries source SHA
  → developer's SessionStart hook shallow-fetches the branch
  → hook injects the root _map.md plus one navigation instruction
```

A pull request regenerates each touched directory plus its ancestor chain to the
root — a parent embeds its children's summaries, so a regenerated child makes its
ancestors stale even when their own files are untouched. A PR touching three
files in two directories of a five-deep tree regenerates on the order of eight
map files, not the repository. This is what keeps the per-merge cost in cents.

## Generator strategy

**Bottom-up rollup with deterministic anchors.**

- **Leaf directories are generated from parsed symbols, not raw source.**
  Tree-sitter extracts exported symbols and their line numbers. The model
  receives the file list and the symbol table, and writes prose next to anchors
  the parser already produced. The model never emits a `file:line` itself,
  so it cannot hallucinate one. This is the central correctness mechanism.
- **Parent directories are generated from their children's `_map.md` files**,
  never from re-reading the children's code. Regenerating the root therefore
  never re-reads the repository, which makes both the initial build and every
  incremental update cheap.
- **Tiered models.** A small fast model (Haiku class) for leaf directories,
  which are high-volume and mechanical. A stronger model only for the top two
  or three levels, where summarization requires judgment.
- **Content-hash short-circuit.** A directory appearing in the diff is skipped
  if the content hashes of its files are unchanged, which covers
  whitespace-only and formatting commits.

## Retrieval

`SessionStart` injects **only the root `_map.md`** — a few hundred tokens — plus
a single instruction:

> A navigation map for this repository is at `~/.semmap/<org>/<repo>/`. Before
> searching the codebase, read the `_map.md` at the corresponding path.

The agent descends the tree on its own using its existing file tools. Token
cost of orientation is logarithmic in repository size rather than linear,
because the agent reads one map per level of descent instead of loading a
whole index.

No MCP server ships in v1. The tool-choice failure mode — the model not calling
an optional search tool at the moment it should — is the most common reason
this class of product goes unused, and injecting the root map sidesteps it
entirely.

## Staleness and correctness

Every `_map.md` records the source commit SHA and the content hashes of the
files it describes.

The client verifies hashes against the working tree before injecting. On any
mismatch — a force push, a failed Action, a local branch ahead of the published
map — the affected subtree is marked stale and **the hook injects nothing for
it**.

The governing principle is **fail closed**. An absent map costs the agent one
grep. A wrong map costs a wrong edit made confidently. These are not
comparable, and every ambiguous case resolves toward silence.

## Error handling

| Condition | Behavior |
|---|---|
| Action fails partway | Publish is one atomic commit or nothing; no partial maps |
| Model output unparseable for a directory | Retain the previous `_map.md`, mark the subtree degraded, continue |
| Token budget for the run exceeded | Publish what succeeded, mark the remainder stale |
| Model API rate limit or timeout | Retry with exponential backoff, bounded by the run budget |
| Vendored or generated directories | Excluded by a default ignore list plus `.semmapignore` |
| First install on a large repository | Full generation; a cost estimate is displayed and confirmed before the run starts |
| Map missing, stale, or hash-mismatched | Hook injects nothing |

The per-run token budget is a required setting in the Action configuration.
There is no unbounded generation path.

## Testing

- **Anchor validation.** Every `file:line` in every generated map must resolve
  to a real symbol at that line in the source at `derived-from`. Runs against a
  fixture repository. This is the load-bearing automated test and it catches
  malformed or drifted output mechanically.
- **Incremental convergence.** Full generation and incremental-from-diff
  generation must produce identical output for the touched directories.
- **Staleness detection.** Mutate a source file without regenerating; assert
  the client marks the subtree stale and injects nothing.
- **Cost regression.** Token spend for a simulated pull request stays under a
  fixed threshold on the fixture repository.
- **Eval harness.** A fixed question set against a real repository — "where is
  X", "what does Y do", "what would I change to add Z" — measuring
  tokens-to-correct-answer with the map versus without.

The eval harness is built in the first week of implementation, not at the end.
It is simultaneously the correctness signal, the tuning loop for generator
prompts, and the only number that will persuade anyone to install this.
Competitors quote large token-reduction figures; semmap needs its own, measured
honestly, including the cases where the map does not help.

## Milestones

1. `mapgen` over a fixture repository, leaf directories only, with anchor
   validation passing.
2. Bottom-up rollup to the root, plus the content-hash short-circuit.
3. Eval harness and the first honest token-reduction measurement.
4. `SessionStart` hook and `semmap sync`, validated end to end on one real
   repository.
5. GitHub Action with incremental regeneration and orphan-branch publish.
6. First install on a repository the author does not own.

The GitHub Action lands at step 5 rather than earlier despite being the
distribution mechanism, because steps 1 through 4 can all be validated by
running `mapgen` locally. Nothing is learned from wiring up CI before the
generated maps are known to be worth distributing.

## Deferred decisions

These are decided with a default so implementation is unblocked, and revisited
only if the eval harness shows they matter:

- **Language support at v1: TypeScript, Python, Go, Rust.** Tree-sitter
  grammars exist for all four and they cover the intended early users. Others
  fall back to a filename-and-imports-only map, which is degraded but not
  wrong.
- **Map lives at `~/.semmap/<org>/<repo>/`, outside the working tree.** Keeps
  it invisible to the repository's own tooling and to `git status`.
- **No multi-repository or cross-repository linking in v1.**
