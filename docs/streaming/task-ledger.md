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
| Hand-written byte-demand state machine | in progress | Header core passed remote E2E at 9a93736 (run 40234332022); full container/object migration #205 remains open |
| Transport abstraction | in progress | File/slice adapters and real-file E2E passed at 422dc67 (run 40234514166); network/worker adapters remain #206 |
| Portable checkpointing | in progress | Header state passed fresh-process restore at 98 byte positions, failed-state restore and malformed checkpoint rejection at a6dc6ba (run 40234765141); decompression/object state remains #207 |
| Spec compiler upgrade | pending | Only after a working hand-written reader; never patch generated output |
| WASM streaming API and native/WASM gate | pending | Existing full-buffer WASM is insufficient |
| Boundary fuzzing through real entry point | pending | proptest-driven executable, one byte and random splits |
| Golden real corpus, large-file benchmarks and comparisons | pending | No fabricated counts, fixtures or performance claims |
| Spec change watcher and automatic invariant walls | pending | Fail closed; no vacuous gate |
| Known prior prunable worktree | preserved peer artifact | Preserve; not owned by this work unit |

Discovered: canonical `synthetic_2000.dwg` has zero header CRC and zero sentinel; the strict §3.2 core must reject it. Track under #205/#211; preserve the fixture until its writer is corrected and the corpus regenerated with proof.

Real inputs acquired read-only outside Git: nextgis `line_R14.dwg` (122353 bytes, SHA-256 `4ae12afb4052f1be913003b84021054c0e07b08e6aef5e430bf523c6f85c0903`) and `line_2000.dwg` (116899 bytes, `f1936ccc5e26dd1aae3f6a26bf5803bdf53d8e0b24f8c18bf3fb96fc540918e0`). Independent prefix CRC/sentinel checks agree with §3.2.6. Native parser measurement pending.

Discovered live-vs-file ruleset drift: committed main-protection has an admin bypass and fewer required checks; live policy has no bypass, adds macOS/Windows/MSRV and a PR requirement. Track under existing #117; preserve all live restrictions when adding the requested streaming gate.

Ready for foundation delivery: remote acceptance at a6dc6ba passed (native/WASM no-allocator kernel, 256 proptest schedules across two real files, 98 fresh-process byte positions, malformed and failed-state restore). Saved artifacts retain binary SHA and exact schedules. This is header-only proof; compiler, object/decompression migration, browser runtime, real R13 and multi-gigabyte corpus remain open, in requested order.

Owned integration path added: `.github/rulesets/main-protection.json`. Export current live restrictions unchanged, add `Streaming E2E`, and apply/verify the live gate only against a checked candidate. Existing #117 tracks ruleset drift; #214 tracks invalid canonical header.

Foundation documentation and required-check integration are in progress for PR #213. The full architecture program is not complete; its ordered acceptance issues stay open. Later stages are deliberately not skipped while full container/object migration remains unresolved.
