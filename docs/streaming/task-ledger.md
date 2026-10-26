# Streaming reader work ledger

Owner: a Claude Code session since October 25, 2026, after #213 merged
(handoff recorded on #204). The foundation was built by a Codex chat on
`codex/streaming-core`, based on `ece6adf`; that branch was removed on
October 26, 2026 after its tree was confirmed equal to `ab1d42b`.
Exclusive edit scope: new `src/streaming/`, transport adapters, checkpoint codec,
CLI probe, streaming CI gate, and streaming documentation. Existing parser and
peer worktrees remain under their existing custody. Build artifacts go to CI;
verification receipts go to `docs/streaming/` or uploaded CI artifacts.

| Work | State | Evidence / next action |
| --- | --- | --- |
| Review startup drift, health queue, Git and active jobs | complete | Counts reviewed; no Rust build; existing clients preserved; isolated clean worktree |
| Audit current reader against requested invariants | complete | Whole-file reads in reader; whole-section buffers; WASM copies uploaded file |
| File ordered gaps in GitHub, link existing issues | complete | Epic #204 and ordered gaps #205–#212 |
| Hand-written byte-demand state machine | in progress | Header core passed remote E2E at 9a93736 (run 40234332022). R13–R2000 container walk (sections, object map, record framing, type, handle) matches an independent parse and the legacy reader on 3,546 records in 15 files; object bodies, R2004+ containers and decompression remain #205. §4.1 R2004-family header matches an independent parse and the legacy reader on 25 files with every documented check enforced; the §4.3–§4.5 page map and section map walk (checksum-verified, records committed) matches both on 25 files (371 pages, 306 sections, 321 section pages); §4.7 LZ77 decoder matches the legacy decoder and an independent parse on 266 pages of 25 R2004–R2013 files; the R2004+ walk that drives it remains #205 |
| Transport abstraction | in progress | File/slice adapters and real-file E2E passed at 422dc67 (run 40234514166); `AsyncSource` (host-driven asynchronous adapter) drives the walk over an HTTP/1.1 range worker with output equal to the file run on 3 files, 9,206 fetches each after a Pending, fresh-process resume with no refetch (including held bytes carried with the checkpoint), and 416/Content-Range/short-body/ignored-Range/oversized-206 faults typed; TLS, remote hosts and a browser worker (#209) remain |
| Portable checkpointing | in progress | Header state passed fresh-process restore at 98 byte positions, failed-state restore and malformed checkpoint rejection at a6dc6ba (run 40234765141). Walk state resumes at every feed of the canonical file and 66 per real file with no re-read or duplicate output; LZ77 decoder state resumes at every call of the canonical page and 45–46 sampled calls of three real pages; object-body state and a combined R2004+ walk checkpoint remain #207 |
| Spec compiler upgrade | pending | Only after a working hand-written reader; never patch generated output |
| WASM streaming API and native/WASM gate | pending | Existing full-buffer WASM is insufficient |
| Boundary fuzzing through real entry point | pending | proptest-driven executable, one byte and random splits |
| Golden real corpus, large-file benchmarks and comparisons | pending | No fabricated counts, fixtures or performance claims |
| Spec change watcher and automatic invariant walls | pending | Fail closed; no vacuous gate |
| Known prior prunable worktree | preserved peer artifact | Preserve; not owned by this work unit |

Discovered: canonical `synthetic_2000.dwg` had zero header CRC and zero sentinel, so the strict §3.2 core rejected it. Fixed under #214: the generator writes both, the fixture was regenerated, and the acceptance run reads it through the strict header entry point.

Real inputs acquired read-only outside Git: nextgis `line_R14.dwg` (122353 bytes, SHA-256 `4ae12afb4052f1be913003b84021054c0e07b08e6aef5e430bf523c6f85c0903`) and `line_2000.dwg` (116899 bytes, `f1936ccc5e26dd1aae3f6a26bf5803bdf53d8e0b24f8c18bf3fb96fc540918e0`). Independent prefix CRC/sentinel checks agree with §3.2.6. Native parser measurement pending.

Discovered live-vs-file ruleset drift: committed main-protection has an admin bypass and fewer required checks; live policy has no bypass, adds macOS/Windows/MSRV and a PR requirement. Track under existing #117; preserve all live restrictions when adding the requested streaming gate.

Ready for foundation delivery: remote acceptance at a6dc6ba passed (native/WASM no-allocator kernel, 256 proptest schedules across two real files, 98 fresh-process byte positions, malformed and failed-state restore). Saved artifacts retain binary SHA and exact schedules. This is header-only proof; compiler, object/decompression migration, browser runtime, real R13 and multi-gigabyte corpus remain open, in requested order.

Owned integration path added: `.github/rulesets/main-protection.json`. Export current live restrictions unchanged, add `Streaming E2E`, and apply/verify the live gate only against a checked candidate. Existing #117 tracks ruleset drift; #214 tracks invalid canonical header.

Foundation documentation and required-check integration are in progress for PR #213. The full architecture program is not complete; its ordered acceptance issues stay open. Later stages are deliberately not skipped while full container/object migration remains unresolved.
