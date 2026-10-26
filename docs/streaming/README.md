# Streaming core

Two hand-written pure state machines are implemented for R13, R14 and
R2000 (AC1012/AC1014/AC1015), ODA v5.4.1: the complete **file header**
(§3.2), and the **container walk** that continues from it through the
header-variable and class sections, the object map and every mapped
record's framing, type and handle (below). Neither decodes object bodies.
For the R2004 family there is a bounded, resumable **LZ77 decoder** (§4.7,
below); the R2004+ container walk that would drive it is not implemented.
Those remain open in [#205](https://github.com/DrunkOnJava/dwg-rs/issues/205). The spec
compiler is an upgrade after the hand-written baseline, tracked in
[#208](https://github.com/DrunkOnJava/dwg-rs/issues/208).

`streaming::FlatHeaderParser::request()` declares the exact remaining byte
count and absolute offset for the current transition. `feed()` accepts a
nonempty prefix of that demand. A short feed saves its partial field;
wrong offsets, empty input and oversized delivery are rejected without
mutation. Grammar failures are terminal values. CRC, locator identities,
source ranges and the end sentinel must all pass before `header()` returns
output. Record 5 is kept as an opaque numbered locator; its meaning is not
guessed. R2004+ yields `UnsupportedLayout`, rather than a successful header
with missing sections.

The grammar retains a fixed 256-byte buffer and has no heap allocation,
source handles or I/O. CI compiles the **same source files** independently
with `no_std`, no allocator and `forbid(unsafe_code)`, on native and
`wasm32-unknown-unknown`. Compile-time bounds cap parser state at 512 bytes
and the checkpoint holder at 272 bytes, including bookkeeping. This proof
covers `src/streaming/`; the legacy reader still loads complete files and
sections. It is not a full-crate streaming certification.

`byte_source::ByteSource` is the pausable delivery contract. A poll returns
`Ready(n)`, `Pending`, EOF or an adapter error. `poll_header()` performs at
most one read into caller-owned bounded scratch. `FileSource` handles short
reads, interrupted calls and absolute positioning without seeking to the
end; `SliceSource` borrows immutable bytes. HTTP and worker implementations
can satisfy the same requests without changing grammar. Their actual
network/worker adapters are still open in #206. A forward-only transport
for a complete DWG needs a spool/cache because indexes can follow objects.

## Container walk (R13–R2000)

`streaming::FlatWalker` starts at byte 0, runs the header grammar above,
then visits, in order: the §9 header-variable section (locator 0), the
§10.1 class section (locator 1) and the §23.1 object map (locator 2),
following each map entry to its §20.1 record. Locators 3–5, the second
header (§26) and image data are not walked. Each `feed` returns a `Step`
naming what its bytes were: `Framing`, provisional `Payload` of a section
or record, or a committed `Header`, `Section` or `Object`, then
`Finished` after the map's final chunk.

The framing it enforces was read from the spec and confirmed on 14 real
files before coding (audit.md): sections are start sentinel, `RL` size,
data, `RS` CRC seeded 0xC0C1 over size and data, end sentinel, and the
locator size must equal 38 plus the data size; map chunks are a
big-endian size counting its own two bytes, 2 to 2032, entries, and a
big-endian CRC over size and entries, ending with a size-2 chunk exactly
at the locator's end; records are `MS` size (at most two words, so under
2^30 bytes), data and a little-endian CRC over the `MS` bytes and data.
After a record's CRC passes, its type (`BS`), the R2000 `RL` and its
handle are read from the first 16 data bytes, and the handle must equal
the map's. Every mismatch is a typed terminal `StreamError`; a zero-size
class locator, an entry crossing its chunk, a record overlapping the
file header or the map are rejected rather than guessed around.

Commit contract (audit.md, decision 7): index bytes are verified before
anything is derived from them, so each map chunk is read twice, once to
check its CRC with no storage and once to decode entries and visit their
records. Payload is handed to the caller as it arrives and is committed
only by the `Section` or `Object` step after its checks; on failure the
caller discards uncommitted payload. A step stays committed even when
the next unit is malformed; that failure is returned by the next request.

State is fixed-size: the walker holds the header parser, a 16-byte
record prefix and a few counters, under 512 bytes, with no allocation;
the `no_std`/wasm32 kernel build includes it. Requests jump between
absolute offsets, so the source must be range-capable or spooled. The
walk does not hold the map: handle-to-offset lookup after the walk is
the caller's.

`WalkCheckpoint` (at most 256 bytes) is the header checkpoint, replayed
on restore, followed by the walk cursors, which must form a state the
walk can reach over the verified locators. Corrupt, truncated, resealed
but inconsistent, wrong-version and wrong-identity checkpoints are
rejected. A failed walk has no checkpoint. The digest of uncommitted
payload and anything else derived from emitted bytes is caller state:
`stream_walk` saves its own beside the checkpoint.

`examples/stream_walk.rs` is the entry point; `--legacy` prints the
legacy reader's handle-map walk in the same shape, and `--length` takes
the source length from a trusted manifest. `tools/streaming/verify_walk.py`
drives it on the canonical R2000 fixture and every R14 and R2000 file in
the pinned `nextgis/dwg_samples` commit (`walk-corpus.json`, fetched by
hash, never committed): 3,546 records. For each file the walk's entire
output equals an independent Python framing parse, its records equal the
legacy reader's (handle, offset, size, type, payload CRC-32), and seven
fixed chunk schedules reproduce it; 128 proptest schedules run on three
files. Fresh-process resume is checked at every feed of the canonical
file and 66 sampled feeds of two real files: the two processes' source
reads and outputs concatenate to the uninterrupted run's. Twenty-five
corruptions per file (flipped, resealed, truncated and short-transport)
must end in the named error with only the valid prefix committed.

Not measured: a real R13 file, files larger than about 160 KB, and any
throughput. The companion DXFs in the sample repository renumber handles
on save, so they are not used as a handle oracle.

## LZ77 decoder (R2004 family, §4.7)

`streaming::Lz77Decoder` decodes one compressed page. `decode(input, out)`
takes any prefix of the remaining compressed bytes and an output slice of
any length, and returns how many of each it used; it stops when the input
is used up, the output is full or the `0x11` terminator is read, so it can
pause between any two bytes of input or output. `end_of_input()` turns a
stream that ends before the terminator into `Lz77Truncated`. The declared
decompressed size is a hard ceiling: a literal run or copy that would pass
it is `Lz77Overrun`, before anything is written. A reserved opcode
(`0x00`–`0x0F`) is `Lz77Opcode` and a back-reference before the first output
byte is `Lz77Offset`, each with its input position. Errors are terminal.

Opcode semantics are the crate's own `lz77::decompress`, measured against
AutoCAD-written files: distances are one more than §4.7's text gives, and
bit 3 of an `0x12`–`0x1F` opcode is bit 14 of the distance. The largest
distance is `0xBFFF`, so the decoder keeps a fixed `0xC000`-byte window of
recent output and nothing else of the page. Its state is that window plus
80 bytes on aarch64 (49,232 bytes; the checkpoint holder is 49,224), and
both are compile-time bounded at `WINDOW + 128` in the `no_std` kernel
build. Neither grows with the page or the stream.

`Lz77Checkpoint` serializes the decoder between calls: the opcode cursor,
the pending copy or literal run, the totals, the limit and the retained
window (the last `min(produced, 0xC000)` output bytes, oldest first),
under a CRC-32. It is `54 + kept + 4` bytes, so at most 49,210 bytes, and
29,754 for a full `0x7400` page. Restore checks length, version and CRC
before copying anything, then requires a state `decode` can reach: fields
a mode does not use are zero, runs fit the limit, a pending copy points
inside the window. `produced` can never pass the limit, since every
pending run is bounded by `limit - produced`; `consumed` has no bound a
checkpoint can prove, so it is accumulated with a check, and a forged
count at the top of its range ends in `CounterOverflow`, never a panic or
a wrapped position. Where the compressed bytes come from, how many the
caller has read but not yet fed, and the output already handed out are
the caller's state; `stream_lz77` saves them beside the checkpoint.

`examples/stream_lz77.rs` decodes every compressed page of an R2004-family
file: the §4.3 section page map and section map, then every page of every
LZ77 section (§4.5). The page list comes from the legacy section map;
only the decoding is under test. `--legacy` decodes the same pages with
`lz77::decompress`. `tools/streaming/verify_lz77.py` runs it on the four
canonical R2004-family fixtures and every R2004, R2010 and R2013 file in
the pinned `nextgis/dwg_samples` commit (`lz77-corpus.json`, fetched by
hash with bounded retry, never committed): 25 files, 266 pages, 6,097,392
decompressed bytes. For every page the output length, CRC-32 and the bytes
consumed through the terminator equal an independent Python §4.7 parse,
and the output equals `lz77::decompress`. Four fixed input/output
schedules (down to one byte in and one byte out, with Pending polls)
reproduce every file's output, and 128 proptest input schedules run on
three files. Fresh-process resume inside the largest data page is checked
at every call of the canonical page and 45 or 46 sampled calls of three
real pages: the two processes' source reads and output concatenate to the
uninterrupted run's. Checkpoints were taken in seven of the ten modes; bit
flips at every byte, every truncation and nine resealed forgeries are
rejected. A CRC-valid checkpoint with `consumed = 2^64 - 1`, in a literal
run and at an opcode, fed directly to the restored decoder
(`--decode-checkpoint`), ends in `CounterOverflow`; a literal run ending
exactly at a limit of `2^64 - 1` decodes. A reserved opcode, a distance before the first byte, an output
past the declared size and a stream cut before the terminator each end in
their named error.

Measured on that corpus: every AutoCAD-written page decompresses to exactly
its declared size (the section's page size for data pages, which includes
the final page's padding), and every compressed stream ends `11 00 00`, two
bytes after the terminator. The crate's writer does neither: its data
pages decompress to their content length and its streams end at `11`. The
decoder accepts both; the writer divergence is recorded on #163.

Not measured: R2018 pages in this acceptance run, pages larger than
`0x7400`, and throughput. #225 pins R2018 drawings from DomCR/ACadSharp
in `tools/corpus/pins.tsv`; `sample_AC1032.dwg`, byte-identical to one of
them, was checked by hand (53 pages equal), and adding them to
`lz77-corpus.json` is a follow-up. R2007 uses a different compression (§5.10) and is not
covered.

## Checkpoint contract

A checkpoint contains the source identity and length, phase, byte position,
current target, locator count/index/mask, failure marker and all retained
partial header bytes. The wire form is canonical, versioned and little
endian. Its current maximum is 157 bytes (97 consumed header bytes plus 60
bytes of metadata/checksum). The holder and every scratch buffer fit within
one declared 256-byte chunk.

| Byte offset | Encoding |
| --- | --- |
| 0 | Four-byte `DWGH` marker |
| 4 | Format version 1 |
| 5–9 | Phase, locator count, locator index, seen mask, failure flag |
| 10 | u16 consumed byte count |
| 12 | u16 current transition target |
| 14 | u16 chunk limit |
| 16 | u64 source length |
| 24 | 32-byte source identity |
| 56 | Consumed prefix, including any partial field |
| Last 4 | CRC32 of all preceding checkpoint bytes |

Restore checks the complete length before copying, rejects incompatible
versions and source identities, checks CRC, then validates phase and counters
by replaying only the retained bytes **inside the checkpoint**. No source
bytes are read to reconstruct state. Failure snapshots restore the same
failure. The next request starts at the first unconsumed source byte.

The caller supplies identity from a trusted content manifest or immutable
object version; the parser does not reread a multi-gigabyte source to hash
it. CRC detects accidental checkpoint corruption and provides no authenticity
guarantee. The caller owns atomic custody of saved state and any already
committed output. `header()` is an inspection API, not an exactly-once event
queue. Full object/decompression checkpoints remain open in #207.

## Repeatable acceptance

The real executable is `examples/stream_header.rs`. The Python driver uses
it in separate OS processes; the proptest executable generates and shrinks
chunk schedules for that same entry point. No new unit tests are involved.

```sh
cargo build --locked --example stream_header --example streaming_schedules
python3 tools/streaming/verify.py target/debug/examples/stream_header target/debug/examples/streaming_schedules
python3 tools/streaming/check_invariants.py
mkdir -p streaming-build
rustc --edition=2024 --crate-type rlib tools/streaming/no_std.rs --out-dir streaming-build
rustc --edition=2024 --crate-type rlib --target wasm32-unknown-unknown tools/streaming/no_std.rs --out-dir streaming-build
```

The R13–R2000 walk and the LZ77 decoder have their own drivers:

```sh
cargo build --locked --example stream_walk --example stream_lz77 --example streaming_schedules
python3 tools/streaming/verify_walk.py target/debug/examples/stream_walk target/debug/examples/streaming_schedules
python3 tools/streaming/verify_lz77.py target/debug/examples/stream_lz77 target/debug/examples/streaming_schedules
```

Run these on the project's build host. The workflow uploads
`streaming-evidence.json` and the generated schedule JSONL files. It checks
12 synthetic version/count combinations, the canonical R2000 fixture,
bytewise and short delivery,
truncation at every header byte, malformed input, two hash-pinned real
R14/R2000 headers, 128 generated schedules per real file, 98 fresh-process
resume positions, actual source-read continuity, pending delivery, corrupt
and inconsistent checkpoints, source mismatch and failure-state restore.
The real-file manifest records header fields, not object counts. No real R13
file, multi-gigabyte model or independent object-count oracle has passed
this reader. The kernel WASM compile does not establish browser runtime or
cross-runtime checkpoint acceptance.

## Current evidence

[Run 40234765141](https://github.com/DrunkOnJava/dwg-rs/actions/runs/40234765141)
passed on `a6dc6baa2d449e3f33db3bba0e3998803cc87834`: native executable
acceptance, proptest schedules, fresh-process resume and the native/WASM
no-allocator kernel builds. The evidence artifact identifies the built
binary by SHA-256. The same commit also passed the existing three WASM
wrapper build targets. Repository-wide checks are reported separately by the
PR; these claims do not establish a complete model parse or release readiness.

The canonical R2000 fixture's writer used to omit the file-header CRC and
sentinel, so the strict reader rejected it ([#214](https://github.com/DrunkOnJava/dwg-rs/issues/214)).
The generator now writes both and the acceptance run checks that file
under every chunk size from 1 to 32 bytes. Its other sections remain
placeholders. The audit and complete ordered work
are in [audit.md](audit.md) and [#204](https://github.com/DrunkOnJava/dwg-rs/issues/204).
