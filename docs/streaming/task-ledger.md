# Streaming reader work ledger

Owner: this Codex chat. Branch: `codex/streaming-core`, based on `ece6adf`.
Exclusive edit scope: new `src/streaming/`, transport adapters, checkpoint codec,
CLI probe, streaming CI gate, and streaming documentation. Existing parser and
peer worktrees remain under their existing custody. Build artifacts go to CI;
verification receipts go to `docs/streaming/` or uploaded CI artifacts.

| Work | State | Evidence / next action |
| --- | --- | --- |
| Review startup drift, health queue, Git and active jobs | complete | Counts reviewed; no Rust build; existing clients preserved; isolated clean worktree |
| Audit current reader against requested invariants | complete | Whole-file reads in reader; whole-section buffers; WASM copies uploaded file |
| File ordered gaps in GitHub, link existing issues | complete | Epic #204 and ordered gaps #205–#212 |
| Hand-written byte-demand state machine | in progress | Explicit suspension, bounded storage, fail closed |
| Transport abstraction | pending | Grammar must contain no I/O |
| Portable checkpointing | pending | Fresh-process resume at exact next byte |
| Spec compiler upgrade | pending | Only after a working hand-written reader; never patch generated output |
| WASM streaming API and native/WASM gate | pending | Existing full-buffer WASM is insufficient |
| Boundary fuzzing through real entry point | pending | proptest-driven executable, one byte and random splits |
| Golden real corpus, large-file benchmarks and comparisons | pending | No fabricated counts, fixtures or performance claims |
| Spec change watcher and automatic invariant walls | pending | Fail closed; no vacuous gate |
| Known prior prunable worktree | existing peer artifact | Preserve; not owned by this work unit |

Discovered: canonical `synthetic_2000.dwg` has zero header CRC and zero sentinel; the strict §3.2 core must reject it. Track under #205/#211; preserve the fixture until its writer is corrected and the corpus regenerated with proof.
