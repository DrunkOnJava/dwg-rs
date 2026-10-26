# Streaming core

Two hand-written pure state machines are implemented for R13, R14 and
R2000 (AC1012/AC1014/AC1015), ODA v5.4.1: the complete **file header**
(§3.2), and the **container walk** that continues from it through the
header-variable and class sections, the object map and every mapped
record's framing, type and handle (below). Neither decodes object bodies.
For the R2004 family there is the **file header** (§4.1), a bounded,
resumable **LZ77 decoder** (§4.7), the **page map and section map walk**
(§4.3–§4.5) and the **data pages** that assemble each section (§4.6), all
below; object bodies and R2007 are not implemented.
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
end; `SliceSource` borrows immutable bytes. `AsyncSource` is the adapter
for a transport that answers later (a worker thread, a browser worker, an
HTTP range fetcher, an event loop): it owns no thread, runtime or
connection. A poll with nothing in flight posts one request of at most one
chunk and returns `Pending`; the host takes it with `take_request()`,
fetches it however it likes and hands the answer back with `complete()`,
`complete_eof()` or `fail()`, then polls again. An answer for another
offset, an empty one or one longer than the request is rejected and the
request stays in flight. Bytes the caller's next scratch cannot take are
held (at most one chunk) for the following polls, so nothing is fetched
twice; across a checkpoint they are the caller's to save (`held()`) and
restore (`with_held()`). A forward-only transport for a complete DWG needs a spool/cache
because indexes can follow objects; `AsyncSource` posts absolute offsets,
so its transport must be range-capable.

`stream_walk --url http://host:port/path --length <bytes>` drives the
container walk through `AsyncSource` and an HTTP/1.1 range worker thread
(`examples/support/http_range.rs`, plain `http`, one keep-alive
connection; an example, not a library client). `tools/streaming/verify_source.py`
serves the canonical R2000 fixture and the pinned `line_R14.dwg` and
`line_2000.dwg` from a local range server: under one fixed and one mixed
chunk schedule the output equals the file-backed run, every poll that
posts is answered on a later poll (9,206 fetches over the two schedules,
each after a `Pending`),
the bytes the server sent equal the bytes delivered to the grammar, in
order and once, and with a fixed schedule every fetch is exactly the
grammar's demand. A walk stopped and resumed in a fresh process over HTTP
fetches no byte twice and delivers none twice, at six stops under a fixed
schedule and fifteen under a mixed one; at five of those the adapter held
fetched but undelivered bytes, which `stream_walk` saves in
`<checkpoint>.source`. Discarding them instead makes the resumed run fetch
them again, which the check catches. A 416 past a short file ends in the
walk's typed `Truncated`; a `Content-Range` for another offset, a body cut
short, a 200 that ignores `Range` and a 206 longer than the request are
source errors (the last two refused before any body is read); each leaves
only a valid prefix of the output. Not measured: TLS, a remote host, latency or throughput, and a
browser worker (#209).

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

## R2004-family file header (§4.1)

`streaming::R2004HeaderParser` reads the first 0x100 bytes of an AC1018,
AC1024, AC1027 or AC1032 file with the same contract as the flat header:
exact byte demands, short feeds kept, wrong offsets and oversized feeds
rejected without mutation, terminal grammar errors, and no output until
every check passes. It checks the long `0x80` at 0x28; the decrypted
block's file ID, its CRC-32 (computed with the CRC field zeroed), and its
fixed fields `0`, `0x6C`, `4`, `0x20`, `0x80` and `0x40` at 0x0C, 0x10,
0x14, 0x44, 0x48 and 0x4C; the 0x14 trailing bytes against the magic
sequence; a page map address on a 0x20 boundary whose §4.3 header lies
inside the source; and a second-header address, when non-zero, inside
the source. A failing field is `R2004Field(offset)` and a CRC mismatch is
`R2004HeaderCrc`. Flat and R2007 magics are `UnsupportedLayout`; the
caller chooses the parser by release, and a front door that does not
re-read the magic belongs to the R2004+ walk (#205). Plain bytes the spec
documents as zero but real files fill (0x15–0x17, 0x2C–0x7F) and the
decrypted "unknown" long at 0x24 are not enforced. The parser is
0x100 retained bytes plus bookkeeping, bounded at 512 in the `no_std`
kernel; its `R2004Checkpoint` is the flat checkpoint's layout with a
`DWG4` marker and room for the whole header, at most 316 bytes (bounded at
336), restored by replaying the bytes inside it.

`examples/stream_r2004.rs` is the entry point; `--legacy` prints the
legacy `header::R2004Header` fields under the same keys.
`tools/streaming/verify_r2004.py` runs it on the four canonical
R2004-family fixtures and the 21 pinned nextgis R2004, R2010 and R2013
files: every header equals an independent Python §4.1 parse and the
legacy reader's fields; chunk sizes 1 to 32 and a mixed schedule with
Pending polls reproduce it; 128 proptest schedules run on three files;
a file cut at each of the 256 header bytes ends in `Truncated`. On one
file per release band and the canonical R2004 fixture, twelve corruptions
(three magics, 0x28, the file ID, the CRC, two fixed fields, the trailer,
a page map past the end or off its boundary, a second header past the
end) each end in their named error with nothing printed, and a fresh
process resumes at every one of the 257 positions reading each byte once.
Bit flips at every byte, every truncation and a wrong source identity
are rejected for a mid-header checkpoint, and a failure snapshot restores
the same failure. The local R2018 file (#96) was checked by hand. The
canonical fixtures pass only since #233 made the writer emit the fixed
fields.

## R2004-family page map and section map (§4.3–§4.5)

`streaming::R2004MapWalker` continues from the §4.1 header into the two
system pages: the section page map at the header's address, then the
section map at the address its entry in the page map gives. Each system
page is read twice, as the flat walk reads an object-map chunk twice: a
pass that checks the §4.3 checksum (the §4.2 sum of the compressed bytes,
seeded with the sum of the header with the field zeroed) and keeps
nothing, then a pass that decodes it with the §4.7 decoder and hands each
record to the caller as a committed step. Nothing is derived from a page
before its checksum passes. `demand()` says whether the next step needs
source bytes (`Read`, at most 64 ahead of the decoder) or only a `Pump`
of output already decoded, so one compressed byte that expands to many
records still yields one record per step; `poll_r2004_maps` drives both.

Checked, because every real file and the canonical fixtures agree: the
system page type and compression type 2, its extent inside the source
(and the section map's inside its page); every page size a non-zero
multiple of 0x20; the page map's own entry at exactly the header's page
map address and large enough for the page; a page-map entry for the
header's section map id; the section map header `(count, 2, 0x7400, 0,
…)`; each description's compression flag (1 or 2), encryption flag (0 to
2), page size and printable name; each section's page start offsets
rising multiples of its page size; and both pages decoding to exactly
their declared size and ending between records (a section count that
understates the records is caught as soon as the last record is read). Not checked, because real
files do not follow them: sequential page numbers, the "unknown" longs,
and page extents inside the file (real last pages run past its end). The
walker holds no map: page numbers, addresses and the section list are the
caller's, so whether a section's pages exist in the page map is the
caller's check.

State is the header parser, the decoder and a few hundred bytes of
cursors, bounded at `WINDOW + 1024` in the `no_std` kernel.
`R2004MapCheckpoint` (`DWGM`) embeds the header checkpoint (replayed), the
decoder checkpoint while a page is being decoded, and the cursors, which
must form a state the walk can reach; at most about 50 KB.

`examples/stream_r2004_maps.rs` is the entry point; `--legacy` prints the
legacy `section_map` page map and section info in the same shape.
`tools/streaming/verify_maps.py` runs it on the four canonical
R2004-family fixtures and the 21 pinned nextgis R2004, R2010 and R2013
files: 371 page entries, 306 section descriptions and 321 section page
references, every line equal to an independent Python parse and every
record equal to the legacy reader; five fixed schedules (one byte up,
with Pending polls) and 128 proptest schedules on three files reproduce
it. A fresh process resumes at every step of the canonical R2018 fixture
(173) and of `line_2004.dwg` (316) and at sampled steps of
`line_2010.dwg` and `line_2013.dwg`: the source reads and output of the
two processes concatenate to the uninterrupted run's. Bit flips at every
byte, every truncation and a wrong identity are rejected for a mid-walk
checkpoint, as are ten resealed unreachable states. On the canonical R2004
fixture and one file per release band, ten corruptions end in their
named error: both page checksums, the page map type, a page size off
0x20, a page map moved off its own entry, a section map header constant,
a section count one short and zero, a section description field, and a
file cut inside the page map. The
record corruptions get past the checksum by rewriting a literal byte of
the LZ77 stream and resealing the page. The local R2018 file (#96) was
checked by hand. The fixtures pass only since #265 made the writer emit
real page checksums.

Not covered here: the data pages (next section), R2007 (§5), and
throughput.

## R2004-family data pages and sections (§4.5–§4.6)

`streaming::R2004SectionWalker` takes one section description (from the map
walk) and then one page at a time from its caller: `demand()` answers
`NextPage` between pages, and `begin_page(page_entry, reference)` hands it
the page map's entry and the section map's reference for the next page of
that section. The caller keeps the page index (the walker holds no map), so
which page comes next and where it lives are the caller's, as the map
walker's doc says. A page that the entry, the reference and the section do
not agree on (page number, section id, an address that leaves no room for
its header in the source, a start offset off a page boundary or not above
the last) is refused with `SectionPageRef`, leaving the walker as it was.

Each page is read twice, as the system pages are. The first pass reads the
0x20-byte masked header (§4.6) and checks its own checksum, then every
field against what the caller said (`DataPage`): type, section number,
start offset, data size and page size, and that the stored bytes lie inside
the page and the source. It then reads the stored bytes in chunks of at most
256 and checks the data checksum, keeping nothing. Only then does the
second pass run them through the §4.7 decoder (or copy them, for a stored
section) and hand out section bytes. `demand()` gives source reads (at most
64 bytes ahead of the decoder, at most 256 in the checksum pass), pumps of
output already decoded, `NextPage` and `Done`. Steps are `Page` (header and
both checksums verified), `Hole`, `Data { offset, len }` (up to 256 bytes,
in `data()` until the next call), `PageEnd` and `Finished`. A page's bytes
are handed out only after its checksums have passed, in section order; a
byte past the section's size is decoded and dropped.

The header layout and both checksum rules are a measured finding
([ARCHITECTURE.md §7n](../../ARCHITECTURE.md)): `section_map::DataPageHeader`
labels three of its fields wrongly, and its `page_size` is the page's size
on disk, not the decompressed size. A compressed page decodes with a
ceiling of the section's page size; it may end anywhere from
`min(page size, bytes the section has left)` up to a whole page and no
shorter (`DataPageLength`), and a stream that outgrows the page size is
`Lz77Overrun`. Bytes of the section that no page covers (before a page that
starts further on, after the last page, or all of a section that lists
none) are handed out as `Hole` and read as zeros; a page that starts at or
past the section's size (a zero page, §4.5) is walked and checked but gives
nothing. An encrypted section (§4.5, flag 1) is refused (`SectionEncrypted`):
its stored bytes are not its content. The legacy reader ignores the flag.

State is the decoder, a 256-byte output stage and a few hundred bytes of
cursors, bounded at `WINDOW + 1024` in the `no_std` kernel (native and
wasm32). `R2004SectionCheckpoint` (`DWGS`) holds the section's fields, the
page in hand, the cursors and, while an LZ77 page is being decoded, the
decoder checkpoint; at most about 50 KB. Restore requires a state the walk
can reach: the stage, the header (checksum included), the page's job against
the section, the input position against the decoder's and the carried bytes,
and the cursor against what has been handed out. Which section and which page
of it come next, and the page index, are the caller's to save beside it.

`examples/stream_r2004_sections.rs` is the entry point: it walks the maps,
then every section, and prints one line per page, hole and section that does
not depend on how the source was chunked (`page`, `hole`, `page_end` with
the page's CRC-32, `section_done` with the whole section's CRC-32). `--out`
writes each assembled section; `--legacy` assembles the same sections with
`section_map::read_section_payload` and prints the same `section_done` lines
and files. `--stop-after` and `--resume` count steps of the data-page walk
and save the page index in `<checkpoint>.caller`. `tools/streaming/verify_sections.py`
runs it on the four canonical R2004-family fixtures, the 21 pinned nextgis
files and a file the verifier builds (holes, a zero page, a section that
lists no page, padded and exact last pages, stored and compressed sections):
every output line and every assembled byte equals an independent Python
assembly (its own §4.7 decoder and checksums), and every section equals the
legacy reader's; five fixed schedules and 128 proptest schedules on three
files reproduce it. A fresh process resumes at every step of the canonical
R2018 fixture and at about 80 stops of each of three real files and the built
one, chosen around every page, hole and section end: the source reads and
output of the two processes concatenate to the uninterrupted run's, the
bytes written equal the section, and all six checkpoint kinds (idle, header,
checksum pass, decoding LZ77, decoding stored, done) were among them. Bit
flips at every byte and every truncation of a checksum-pass checkpoint are
rejected, as are 120 sampled of each for a decoding one, a wrong identity
and twenty resealed unreachable states; and pages the sidecar index lies
about are refused by the walker. On the canonical R2004 fixture and one real
file per release band nine corruptions end in their named error: both checksums, five
header fields with the header resealed, a stream shorter than its place and
one that outgrows the page size; on the built file, a short page, an
encrypted section, a stored page's checksum and type and an overrun. One
real file with a page moved past the section's end (rewriting a literal byte
of the section map and resealing both pages) agrees with both oracles.

Not covered: object bodies, R2007, one checkpoint across the whole R2004
walk (#207), and throughput.

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

## Cross-runtime restore (R2004 walks)

`tools/streaming/wasm_resume.rs` is a `#![no_std]` WebAssembly module built
from the identical grammar files (`rustc --target wasm32-unknown-unknown`,
no allocator). A host copies the source bytes, a checkpoint and the caller's
cursors into its scratch memory, calls an export and reads back the JSON
lines the native examples print: `resume_maps` restores a `DWGM`
checkpoint and finishes the page map and section map walk, `resume_section`
restores a `DWGS` checkpoint (given the section's rows of the caller's page
index and the saved cursors) and finishes that section, and `run_maps` walks
for a number of steps and leaves a `DWGM` checkpoint for the native build.
`tools/streaming/wasm_resume.mjs` drives it under node, a fresh instance per
job, and `tools/streaming/verify_wasm.py` is the acceptance run.

On the canonical R2018 fixture and `line_2004.dwg`, `line_2010.dwg` and
`line_2013.dwg` (a file the verifier builds for the section walk): every
native map-walk checkpoint (every step of the fixture, about 40 sampled of
each real file) restored in the kernel finishes with exactly the lines the
native resume prints, with the kernel reading at 1, 7, 64 or 256 bytes at a
time; the kernel's own checkpoint at the same step is byte-identical to the
native one in all of them, and the native build resumes it to the
uninterrupted run's output. About 90 native data-page checkpoints of each
file, around every page, hole and section end, restored in the kernel finish
their section with the lines the native resume prints, and the section's
CRC-32 equals the independent assembly of `verify_sections.py`. A bit flip
anywhere sampled in a native checkpoint of either walk, and a wrong
identity, are refused by the kernel as by the native build. The kernel runs
under node (V8), not in a browser, and the section walk crosses runtimes in
one direction only.

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
cargo build --locked --example stream_walk --example stream_lz77 --example stream_r2004 --example stream_r2004_maps --example stream_r2004_sections --example streaming_schedules
python3 tools/streaming/verify_walk.py target/debug/examples/stream_walk target/debug/examples/streaming_schedules
python3 tools/streaming/verify_lz77.py target/debug/examples/stream_lz77 target/debug/examples/streaming_schedules
python3 tools/streaming/verify_source.py target/debug/examples/stream_walk
python3 tools/streaming/verify_r2004.py target/debug/examples/stream_r2004 target/debug/examples/streaming_schedules
python3 tools/streaming/verify_maps.py target/debug/examples/stream_r2004_maps target/debug/examples/streaming_schedules
python3 tools/streaming/verify_sections.py target/debug/examples/stream_r2004_sections target/debug/examples/streaming_schedules
python3 tools/streaming/verify_wasm.py target/debug/examples/stream_r2004_maps target/debug/examples/stream_r2004_sections
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
this reader. The kernel WASM compile does not establish browser runtime;
cross-runtime checkpoint acceptance is established only for the R2004 walks
(below).

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
