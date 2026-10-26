# Streaming architecture audit

Audited base: `ece6adf`, October 25, 2026. Tracking epic: [#204](https://github.com/DrunkOnJava/dwg-rs/issues/204).

| Priority | Requirement | Current evidence | Issue |
| --- | --- | --- | --- |
| 1 | Pure byte-demand state machine | `reader.rs` uses `fs::read`, retains all input, materializes sections; decoder calls are synchronous full slices | #205 |
| 2 | Pausable transport independent of grammar | No byte source trait; native and WASM supply entire owned files | #206 |
| 3 | Portable exact-position checkpoints | No versioned parser-state encoding or fresh-process resume | #207 |
| 4 | Reviewed grammar compiler | Rust field lists handwritten; spec coverage is a Markdown matrix | #208 |
| 5 | Streaming WASM | Existing native/WASM build CI; WASM open copies whole upload; browser uses `arrayBuffer()` | #209 |
| 6 | Boundary equivalence and automatic walls | Existing proptest/fuzz targets exercise whole slices; no chunk equivalence or parser allocation gate | #210 |
| 7 | Real golden and multi-gigabyte evidence | Five committed synthetic fixtures; no independent large-model oracle; public corpus CI tracked in #95/#97 | #211 |
| 8 | Spec change watcher | No pinned public-document hash or revision notification | #212 |

The existing `Version` rejects R13/AC1012 (#119). `OpenLimits` documents
several downstream caps as advisory; limits alone do not establish bounded
streaming. The four viewer samples are identical generated skeletons, not
four independently authored drawings. All remain explicit migration gaps.

## Decisions

1. A hand-written state machine comes first. A compiler may replace measured
   transitions after parity; generated files are never manually patched.
2. Grammar declares absolute byte demands. Source I/O, pending callbacks,
   scheduling, caching and file descriptors stay in adapters. An indexed
   DWG on a forward-only stream requires a spool/cache or a range-capable
   source; transport independence cannot remove this physical requirement.
3. The first core milestone is a bounded file-header state machine. Its
   completion means a header completed, never a model or object walk.
   Container and object migration remains open under #205.
4. Explicit failed/unsupported outcomes are part of the output contract.
   Unknown spec fields are preserved or surfaced; never silently guessed.
5. E2E acceptance invokes a built executable and saves receipts. Proptest
   generates chunk schedules for that executable. No new unit tests.
6. No full-parser memory or performance claim until it is measured on real
   files with independent object counts. Missing large models remain
   NOT MEASURED. Synthetic fixtures and build success cannot substitute.
7. Commit contract for the R13–R2000 container walk (October 25, 2026).
   **Index bytes are verified before anything is derived from them;
   payload bytes are delivered provisionally and committed by a verdict.**
   The file header fits in one chunk and is verified whole. An object-map
   chunk (up to 2032 bytes) does not, so the walker reads it twice: a
   CRC-only pass with no storage, then a decoding pass that visits
   objects. No object is visited from an unverified chunk; the second
   pass costs one extra read of the map, which is small next to the
   objects it indexes. Header-variable, class and object data are handed
   to the caller as they arrive, in slices no larger than the request,
   and become committed only at the `Section` or `Object` step emitted
   after their CRC, sentinel and handle checks pass. A failed check is
   a terminal error; the caller discards uncommitted payload.

8. LZ77 decoder state (October 26, 2026). **The window is bounded by the
   format, not by the page.** §4.7 distances reach at most `0xBFFF` with
   the crate's measured distance rules, so a `0xC000`-byte window decodes
   any page, including system pages whose declared size has no fixed cap.
   Output is produced into caller slices of any size and is provisional
   until the caller's own commit point, as in decision 7. A mid-page
   checkpoint must carry the window; it is bounded by
   `min(produced, 0xC000)` and never re-reads compressed bytes.

## Core failure cases to exercise before implementation

- EOF at every byte of a field, locator, CRC and sentinel.
- Wrong magic, unsupported release, unrecognized locator count/number,
  duplicate locators, overflow/out-of-source section ranges and bad CRC.
- Exact-demand, one-byte and random short reads produce identical results.
- Oversized, empty or out-of-position feed rejected without mutation.
- Pending transport leaves parser unchanged; zero-byte EOF is distinct.
- Mid-field checkpoint, terminal checkpoint, corrupted/truncated/oversized
  checkpoint, incompatible schema, wrong source identity/length or limits.
- Resume in a fresh process starts at the first unconsumed source byte.
- Parser emits no success event until integrity checks pass.
- Oversized chunk configuration never induces an allocation.

Spec authority: ODA public *Open Design Specification for .dwg files*
v5.4.1, chapters 3–8 and §2.14. Checked against the official PDF on
October 25, 2026. Only specification and this repository's own implementation
were consulted; no third-party DWG implementation source.

## Container walk failure cases (R13–R2000, written before the walker)

Spec framing, confirmed on `line_R14.dwg` and `line_2000.dwg` before
coding: §9 and §10.1 sections are start sentinel, `RL` size, data, `RS`
CRC seeded 0xC0C1 over size and data, end sentinel, and the locator size
equals 38 plus the data size. §23.1 map chunks are a big-endian size
that counts its own two bytes, then entries, then a big-endian CRC
seeded 0xC0C1 over size and entries; a size-2 chunk ends the map at the
locator's end. §20.1 records are `MS` size, data, then an `RS` CRC
seeded 0xC0C1 over the `MS` bytes and data.

- EOF inside every fixed field, payload run, map entry and record.
- Wrong start or end sentinel; locator size not 38 plus the data size;
  zero-size class locator; section CRC mismatch.
- Map chunk size below 2 or above 2032, running past the map locator,
  final chunk not ending exactly at the locator end, chunk CRC mismatch.
- Map entry crossing its chunk end, `MC` longer than a `u64` holds,
  handle or offset overflow, negative absolute offset.
- Record outside the source, overlapping the file header or the map,
  `MS` of zero or of more than two words, record CRC mismatch, prologue
  too short for type and handle, handle counter above 8, record handle
  different from the map's handle.
- Every chunk schedule (1 byte, odd sizes, random) produces the same
  steps and committed records.
- Checkpoint at every step boundary, restored in a fresh process: the
  concatenated source reads and outputs of the two processes equal the
  uninterrupted run's, so no byte is read twice and no record is
  delivered twice.
- Corrupt, truncated, oversized, wrong-version, wrong-identity and
  internally inconsistent walk checkpoints are rejected before use.
- Committed records equal the legacy reader's handle-map walk (count,
  handle, offset, size, type, payload CRC-32) and an independent
  Python framing parse on every real file in the pinned manifest.

## LZ77 decoder failure cases (§4.7, written before the decoder)

- Reserved opcode `0x00`–`0x0F` where an opcode must stand (a byte in
  that range where a literal length may stand is a literal length).
- Back-reference distance of zero or beyond the output produced so far.
- Literal run, copy or `0x00` extension run passing the declared size.
- Input ending inside a literal run, an extension, a two-byte offset, an
  `0x40`–`0xFF` opcode's second byte, or before the `0x11` terminator.
- Copies longer than their distance (the run repeats its own output).
- Every input and output slice schedule (one byte, odd sizes, random,
  with Pending polls) produces the same output and consumption.
- Checkpoint between any two calls, in every mode, restored in a fresh
  process: source reads and output of the two processes concatenate to
  the uninterrupted run's.
- Corrupt, truncated, oversized, wrong-version and resealed but
  unreachable decoder checkpoints are rejected before use.
- A resealed checkpoint whose counters sit at the top of their range:
  the next byte is a typed error, never an overflow; a run ending exactly
  at the largest limit still decodes.
- Output equals the legacy whole-buffer decoder and an independent parse
  on every compressed page of every real file in the pinned manifest.

## Asynchronous source failure cases (#206, written before the adapter)

- A poll with a read in flight returns `Pending` and changes nothing; a
  different request while one is in flight is an error, never a second
  fetch.
- An answer for another offset, an empty answer or one longer than the
  request is rejected and leaves the request in flight.
- More bytes than the caller's next scratch takes are held and delivered
  on the following polls; a request that does not continue them is an
  error.
- An answer of "no byte here" becomes `Eof` at exactly that offset, and
  the grammar turns it into its typed truncation.
- A transport failure surfaces as the source error on the next poll.
- Output through the adapter equals the file-backed run; every fetched
  byte is delivered once, in order; a fresh-process resume fetches no
  byte twice, including bytes the adapter held at the checkpoint.
- A transport answer that ignores the range or is longer than the request
  is refused before its body is read.

## R2004-family header failure cases (§4.1, written before the parser)

- EOF at every byte of the 0x100-byte header.
- Unknown magic; flat (AC1012/AC1014/AC1015) or R2007 (AC1021) magic.
- Plain 0x28 not 0x80; decrypted file ID wrong; CRC-32 of the decrypted
  block (CRC field zeroed) wrong; a fixed decrypted field (0x0C, 0x10,
  0x14, 0x44, 0x48, 0x4C) wrong after resealing the CRC.
- The 0x14 bytes at 0xEC not the magic sequence continued.
- Page map address off a 0x20 boundary or its header past the source;
  a non-zero second-header address past the source.
- Every chunk schedule produces the same header; checkpoint at every
  byte resumes in a fresh process reading each byte once; corrupt,
  truncated and wrong-identity checkpoints are rejected; a failure
  snapshot restores the same failure.

## R2004 page map and section map failure cases (§4.3–§4.5, written before the walker)

- System page: wrong type tag, compression type not 2, zero sizes, data
  past the source, the section map larger than its page; checksum wrong.
- Page map: entry number 0, size zero or off 0x20, address overflow, no
  entry of its own at the header's address or one too small for it, no
  entry for the section map id, a gap entry cut short.
- Section map: header constants other than (2, 0x7400, 0); a description
  with a compression flag other than 1 or 2, an encryption flag above 2,
  a zero page size or a non-printable name; page start offsets that do
  not rise or are not whole pages.
- Either page decoding to more or less than declared, or ending inside a
  record.
- Every chunk schedule produces the same steps; a checkpoint at every
  step resumes in a fresh process with reads and output concatenating to
  the uninterrupted run's; corrupt, truncated, wrong-identity and resealed
  unreachable checkpoints are rejected.
