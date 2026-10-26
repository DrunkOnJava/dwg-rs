Two hand-written pure state machines are implemented for R13, R14 and
R2000 (AC1012/AC1014/AC1015), ODA v5.4.1: the complete **file header** (§3.2),
and the **container walk** that continues from it through the header-variable
and class sections, the object map and every mapped record's framing, type and
handle (below). Neither decodes object bodies.
For the R2004 family there is the **file header** (§4.1), a bounded,
resumable **LZ77 decoder** (§4.7), the **page map and section map walk**
(§4.3–§4.5), the **data pages** that assemble each section (§4.6), the
**whole-file walk** that joins them behind one checkpoint, and the **object
records** (§20.1) framed out of that walk, all below. R2007 (AC1021) has its
own **whole-file walk** (§5.1–§5.4, §5.10–§5.13): its own Reed-Solomon pages,
LZ dialect, page map, section map and checksums, behind one checkpoint. An
object's fields are not decoded, and R2007's object records are not framed.
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
big-endian size counting its own two bytes, 2 to 2034 (the spec's 2,032 bytes of entries and the size field), entries, and a
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

Two 1.3 MB drawings from ACadSharp (`sample_AC1014.dwg` and
`sample_AC1015.dwg`, `walk-acadsharp-corpus.json`) were added with the object
stream below: 1,242 and 1,047 records, each equal to the independent parse and
the legacy reader. They found that the walker refused the first object map
chunk of both, 2,033 and 2,034 bytes: a chunk's size counts its own two bytes,
so the spec's 2,032 bytes of entries make 2,034 (ARCHITECTURE.md §7ac), and the
limit is now that.

Not measured: a real R13 file and any throughput. The companion DXFs in the
sample repository renumber handles on save, so they are not used as a handle
oracle.

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
used to label three of its fields wrongly (now fixed), and its `page_size` is
the page's size on disk, not the decompressed size. A compressed page decodes with a
ceiling of the section's page size; it may end anywhere from
`min(page size, bytes the section has left)` up to a whole page and no
shorter (`DataPageLength`), and a stream that outgrows the page size is
`Lz77Overrun`. Bytes of the section that no page covers (before a page that
starts further on, after the last page, or all of a section that lists
none) are handed out as `Hole` and read as zeros; a page that starts at or
past the section's size (a zero page, §4.5) is walked and checked but gives
nothing. An encrypted section (§4.5, flag 1) is refused (`SectionEncrypted`):
its stored bytes are not its content. The legacy reader refuses it the
same way (`Error::SectionEncrypted`), as it does an R2007 section whose
encryption flag is set.

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

Not covered here: object bodies, R2007 (its own walk, below), and throughput. The
section walker needs the caller's page index; the whole-file walker (next
section) does not.

## The whole R2004-family file (§4.1, §4.3–§4.6)

`streaming::R2004Walker` is the map walker and the section walker behind
one state and one checkpoint, for a caller that has no page index and wants
none. A walker with fixed state cannot hold the page index (page number to
address, unbounded), so it does not walk in section order. Pass A is the map
walk: the header, the page map and the section map. Each section description
goes into a fixed table of at most 64 slots (`MAX_SECTIONS`; real drawings
have 14 or fewer; one more is `SectionTableFull`) and is also handed to the
caller as a `Section` step; each section's page references are counted and
folded into a digest, not kept. Pass B reads the page map again, entry by
entry, and walks each data page it lists, in the order the page map gives:
the page's checksummed header names its section and its place in it, which
is all the section walker needs, so no page index is passed in, and the page
is then read, checked and decoded exactly as in the section walker. A page
of a section the section map does not describe is `SectionPageUnknown`.
Bytes come out tagged with their section and offset (`Data { section_id,
offset, len }`) in page-map order, and the consumer scatters them; holes
are whatever it never writes.

At the end, for every section, the count and the digest of the page
references the section map listed must equal the count and the digest of the
pages the page map held (`SectionDigest`). The digest is the wrapping sum
over pages of a splitmix64 mix of (page number, start offset, data size), so
a listed page that is missing, one walked twice, one that is not listed and
one in the wrong place each change it, whatever order the page map holds
them in. The legacy reader accepts any order, and so does this walk. The
digest is an integrity check on a file whose page headers are already
checksummed, not a security boundary. A page's data size is the length of its
stored bytes (a whole page for a stored section, the compressed length for
an LZ77 one), so it cannot be bounded by the section's size and is not; what
bounds a page is its start offset, a multiple of the section's page size, and
the section's size: a padded last page's bytes past the end of the section are
decoded, to reach the stream's end and its checksums, and dropped.

The page map is read twice (passes A and B) and each data page three times:
its 32-byte header to start the walk, then the checksum pass and the decoding
pass of the section walker. State is the two walkers (two decoder windows)
and the table, bounded at `2 * WINDOW + 8192` in the `no_std` kernel.
`R2004WalkCheckpoint` (`DWGF`) holds the table, the map walker's checkpoint,
and while a page is walked the section walker's; at most about 100 KB. It is
self-contained: restore needs the source identity and nothing else, and what
the caller keeps are the cursors over the bytes it was handed. Restore
requires a state the walk can reach: both embedded walkers validated by their
own restore, then the table (indexes in order, ids unique, references no more
than the pages declared, pages seen no more than listed, a finished walk's
digests equal), the stage against the map walker's mode and refs left, and
the page in hand against the table.

`examples/stream_r2004_file.rs` is the entry point. It prints a `section`
line per section, a `page` and `page_end` line per page (the page's CRC-32)
and the end of the walk, none of which depends on the chunking; with `--out`
it writes each section sparsely to `<dir>/<index>.bin` and prints a
`section_done` line (size and CRC-32 read back from the file) per section;
`--legacy` assembles the same sections with `read_section_payload`.
`--stop-after` and `--resume` take the checkpoint alone and keep the output
cursors in `<checkpoint>.caller`. `tools/streaming/verify_file.py` runs it on
the four canonical R2004-family fixtures, the 21 pinned nextgis files and a
file it builds three ways (pages in section order, reversed and shuffled
in the file and the page map): every line and every assembled byte equals an
independent Python assembly and the legacy reader (321 pages, 306
sections); five fixed and 128 proptest schedules; a fresh process resumes at
every step of the R2018 fixture and about 80 stops of each of three real
files and the built ones, with all six stages checkpointed and source reads and
output concatenating exactly. Bit flips at every byte and every truncation of a
small checkpoint, 100 sampled of each for a walking one, a wrong identity and
50 resealed unreachable states are rejected. On the canonical R2004
fixture and one real file per release band six corruptions end in their named
error (both checksums, a page type, a section the map does not describe, a start
offset the map does not list, a data size); on the built file a listed page the
page map lacks, one it lists twice, an unlisted page of a known section, a page
of an unknown section, 65 sections and an encrypted section.

Not covered: throughput, and R13 (no AC1012 drawing is available to walk). The
object records are the next section, and R2007 has its own walk after it.

## The object records of an R2004-family file (§20.1)

`streaming::R2004ObjectWalker` is the whole-file walker with one section
singled out. The bytes of `AcDb:AcDbObjects` do not leave it as section data
but are framed, in section order, into records; every other section's data
passes through as the file walker reports it (`ObjectStep::Walk`). A record is
the `MS` size, on R2010 and later the `MC` handle-stream size that the `MS` does
not count, `MS` bytes of payload and a CRC over the bytes from the `MS`
through the last payload byte (seed 0xC0C1), after a four-byte `RL 0x0DCA`
prologue. The type and handle are read from the payload's first bits (a `BS`
type and the `RL` size on R2004, a two-bit-tagged type on R2010 and later), and
the record is committed only after the CRC and those fields pass. Its payload
(the `MC`, if any, and the data, string and handle streams: the bytes the
legacy `RawObject::raw` holds) is handed out as it arrives in `Payload` steps
and is provisional until the `Object` step that commits it. The records are
emitted in stream order, each with the handle it carries; the `AcDb:Handles`
map is not read and no index of handles or offsets is kept. The framer demands
exact byte counts at every stage (the prologue, each `MS` word, an `MC` byte at
a time, the payload remaining, the CRC), takes a slice of any size and consumes
no more than its stage needs. A zero or three-word `MS`, a record that would
pass the section's end, a wrong CRC, an `MC` of more than ten bytes or of more
bits than the record holds, an unreadable type or handle, a missing prologue,
bytes that stop short of a record boundary at the end of the section and a
file with no object section or two are each a typed terminal error
(`RecordSize`, `RecordRange`, `RecordCrc`, `RecordHeader`, `ObjectPrologue`,
`ObjectsIncomplete`, `ObjectsMissing`, `SectionEntry`), and the walk must end on
a record boundary exactly at the section's size (`Objects`).

The whole-file walker visits data pages in the order the page map lists them,
and the framer needs the section's bytes in order. A page of the object section
that the page map lists out of order, or a hole in it, is refused with
`ObjectsOrder` rather than buffered or reordered, since the state is fixed and
no page index is kept. All 28 R2004-family drawings of the pinned corpus list
the object section's pages in order and contiguously (ARCHITECTURE.md §7ac),
while pages of other sections interleave freely with them (the verifier checks
that on built files).

State is the file walker, a 256-byte stage for the section bytes a `Data`
step handed over and the framer's few dozen bytes, bounded at `2 * WINDOW +
8192 + 1024` in the `no_std` kernel (native and wasm32). `R2004ObjectCheckpoint`
(`DWGO`) holds the file walker's checkpoint (restored by its own restore), the
object section's id, the framer's cursors and the section bytes not yet
framed (at most 256); at most about 100 KB. It is self-contained: restore needs
the source identity and nothing else, and what the caller keeps is the digest
and length of the open record's payload so far. Restore requires a state the
walk can reach: the framer's cursors against the record bytes it holds (its CRC
is recomputed wherever those bytes are still held, and its `MC` is re-read from
them), the section against the table, the unframed bytes against the section
walker's cursor. The file walker's table keeps no
section names, so a checkpoint taken before the object section is described
cannot be checked for a table that already holds it; a forged one restores and
ends in `ObjectsMissing` or `SectionEntry` rather than `InvalidCheckpoint`. A forged
CRC in a record whose payload is past the 32 bytes the framer holds ends in
`RecordCrc` at the record's end for the same reason.

`examples/stream_r2004_objects.rs` is the entry point. It prints one line per
record (handle, section offset, `MS` size, type, payload length and the payload's
CRC-32), then `objects` (records and the section's bytes) and `finished`, none
of which depends on how the source was chunked; `--legacy` prints the same
record lines from `DwgFile::all_objects_lossy`. `--stop-after` and `--resume`
count steps and keep the payload cursors in `<checkpoint>.caller`.
`tools/streaming/verify_objects.py` runs it on the four canonical fixtures and
the 28 pinned R2004, R2010, R2013 and R2018 drawings (21 from nextgis,
`lz77-corpus.json`; 7 from ACadSharp, `objects-corpus.json`): 8,027 records,
8,007 of them in the drawings. Every printed line equals an independent Python
framing of the independently assembled section (its own `MS`, `MC`, CRC and bit
reader), the (handle, offset) pairs equal an independent parse of
`AcDb:Handles`, and the records equal the legacy reader's on handle, offset,
size, type, payload length and payload CRC-32. Reads of 1, 3, 64 and 256 bytes
and a mixed schedule with Pending polls (13 and 256 and the mixed one for the
four drawings over 300 KB) print the same output, and so do 128 proptest
schedules on three files. Real object sections are repacked in files the
verifier builds, at page sizes from 4 bytes up so that records, `MS` words and
`MC` fields straddle pages, and with the other sections' pages interleaved with
the object section's; they print the same records. A fresh process resumes at
every step of the R2018 fixture (396), at about 110 stops of each of three
small real files and 32 of two large ones, and at about 190 stops of built files
whose page size puts an `MS` or an `MC` across a page boundary; the 1,374
resumes' source reads and output concatenate to the uninterrupted run's, and
the checkpoints were taken in every framer stage, 636 of them holding unframed
bytes, 18 inside an `MC` and 37 inside an `MS`. 750 bit flips and 750
truncations of three checkpoints (the fixture before its first record and inside
one, a real drawing inside one), a wrong identity and 158 resealed unreachable
states are rejected. 49 errors above are reached on real object sections edited
and repacked (a record's CRC or payload flipped, a prologue byte, an `MS` of
zero or of three words, a record past the end, a section cut inside the last
record, bytes after it, an unreadable handle with a valid CRC, a record too short
for its header, an `MC` of eleven bytes or one claiming too many bits), and the
output before each is exactly the records before the failing one; swapped and
dropped pages of the object section end in `ObjectsOrder`, and a file without the
object section or with two in `ObjectsMissing` and `SectionEntry`.

`tools/streaming/verify_wasm_objects.py` is the cross-runtime run: native `DWGO`
checkpoints (every step of the R2018 fixture, 56 to 58 stops of each small real
file, 21 and 22 of the large ones) restored in the kernel finish with the lines
the native resume prints, at kernel read sizes of 1, 7, 64 and 256 bytes; the
kernel's own checkpoint at the same step is byte-identical to the native one in
all 611, with equal cursors, and the native build resumes it to the
uninterrupted run's output; 82 flipped bits and a wrong identity are refused by
the kernel.

Not covered: an object's fields (the records are bytes), R2007's object
records, object pages the page map lists out of order, and throughput.

## The whole R2007 file (§5.1–§5.4, §5.10–§5.13)

`streaming::R2007Walker` walks an AC1021 file behind one fixed-size state and
one checkpoint: the file header, the page map, the section map and every data
page of every section. R2007 shares nothing with the R2004 family's container
(a different compression, Reed-Solomon pages, 64-bit CRCs, bare data pages), so
it has its own walker, and the design differs where the format forces it.

**The section map is the outer loop.** A data page has no header: its section,
its place in it, its sizes, its checksum and its CRC are in its reference in
the section map, and its address only in the page map. A walker with fixed state
cannot hold a page index, so for each page reference it scans the page map from
its start for the entry (one pass over a small system page per reference, and
the scan stops at the entry) and then walks the page. Pages therefore come out
in the order each section lists them, which is the order of its bytes, tagged
with their offset, with a gap before a page handed out as a `Hole` of zeros
(§5.2: a page of zeros is not written) and a section that ends short of its
size ending short, both exactly as `r2007::read_section` returns them. No
section table is needed, so there is no section limit. A section that is
encrypted (§5.2, encryption other than 0 or 2) is refused when its description
is read (`SectionEncrypted`), as the R2004 walk does.

**No page is held, and none is bounded.** Every page, system or data, is a run of
Reed-Solomon codewords interleaved across the page (§5.13.2): byte `i` of
codeword `j` of `n` is at `n * i + j`, so a codeword's 255 bytes are not
adjacent. The walker demands one codeword at a time with a `Gather` (the offset
of its first byte, the stride `n`, the bytes still owed; `byte_source` has a
`poll_gather`, and `FileSource` serves it from a 1 MiB window, which a read that
would block keeps and the next gather continues); a short feed is kept like any
other. Each codeword's syndromes are checked as it arrives and its
message bytes feed the §5.10 decoder, `streaming::r2007_lz`, which pauses between any two
bytes of input or output, copies literal runs in §5.10.1's permuted blocks of 32
(one block of input and one of output are its only buffers) and keeps a 64 KiB
window, the largest distance §5.10 encodes. This matters: the pages of
`sample_AC1021.dwg` run to 70,912 bytes and 273 codewords although every section
names `0xF800` as its maximum, so a buffer sized to the maximum refuses real
drawings (ARCHITECTURE.md §7ai).

**Every byte is checked before it is handed out.** Each page is walked twice.
The first pass reads every codeword of the page (the repeated copies of a system
page included), checks every syndrome, the CRC-64 of the stored bytes (mirrored
flavour, `UpdateSeed1`, seeded from the header's CRC seed), and decodes the whole
page to its declared size, checking the CRC-64 of the decompressed bytes for a
system page and the §5.4.1 checksum for a data page; it hands out nothing. The
second pass decodes again and hands out bytes. The file header page gets the
same treatment in one buffer (its three codewords, the two CRC-64s and the
checking sequence of §5.2.1, the five check values at `0x3D8`, the header's own
CRC and its fixed fields), and the page map's entries must account for the
header's counts and name the page map and the section map. Every rule was
measured on the pinned drawings before it was enforced, and the probe that
reproduces the table is `tools/streaming/probe_r2007_checks.py` (§7ai). Where the
legacy reader stops quietly on a stream that ends before its size, this walk
reports `Lz77Truncated`; where it ignores a section name that is empty or an
unnamed descriptor with pages, this walk accepts the first and refuses the
second.

State is two readers (the section map's, kept while the pages it lists are
walked, and the page scan's or the data page's), each a 64 KiB window, a
codeword and a 256-byte output stage, and a few hundred bytes of cursors,
bounded at `2 * R2007_WINDOW + 8192` in the `no_std` kernel (native and wasm32).
`R2007Checkpoint` (`DWGR`) holds the cursors, the verified 0x110-byte header
(re-checked on restore) and each reader's progress; the geometry of every page
(where it is, how many codewords, what its checks must be) is derived from them
on restore and never stored, and a retained window is the last
`min(produced, 64 KiB)` bytes. At most `2 * R2007_WINDOW + 8192` bytes. Restore
requires a reachable state: each reader's counters against its page, the
decoder's mode against its fields and window, the walk's stage against which
readers are in use, and the page reference against the section.

`examples/stream_r2007.rs` is the entry point. It prints a `page_entry` line per
page map entry, a `section` line, then a `page`, a `hole` and a `page_end` line
(with the CRC-32 of the page's emitted bytes) per page and a `section_end`, and
the end of the walk, none of which depends on the chunking; with `--out` it
writes each section sparsely to `<dir>/<index>.bin` and prints a `section_done`
line (size and CRC-32, read back). `--legacy` walks the same file with
`r2007::Container` and prints the same lines. `--trace` records every read or
gather (`offset`, `stride`, `bytes`); `--stop-after` and `--resume` take the
checkpoint alone and keep the output cursors in `<checkpoint>.caller`.
`tools/streaming/verify_r2007.py` runs it on the eight pinned drawings (seven
nextgis saves and ACadSharp's `sample_AC1021.dwg`; the local subset at
`../../samples` holds three of them) and on three files `r2007_ref.build` writes
from the spec's text, with shapes the corpus lacks (a page of 70,912 bytes and
273 codewords, holes, a zero-length page, a page past its section, a section
short of its size, a page map out of file order with free pages, a non-zero CRC
seed, system pages and header stored, repeat factor 1, a compressed bare page, a
bare page of zero bytes in a zero-sized page map slot, a 126-byte section name
that straddles a codeword, so a checkpoint holds 107 bytes of a name that
`restore` must accept).
`tools/streaming/r2007_ref.py` is an independent reader and writer written from
the spec's text and sharing no code with the crate (its Reed-Solomon encoder
reproduces AutoCAD's parity on every codeword it is given, and its compressor
emits every instruction class).

Measured, by `verify_r2007.py` (11 files, 115 sections, 153 pages, 4,966 data
codewords, 3,067,696 section bytes): on each of them every output line and every
assembled byte equals the independent Python parse and the legacy reader's, four
to seven fixed chunk schedules (down to one byte at a time on files under 400 KB,
with `Pending` polls, and one through a reader under `FileSource` that fails every
other read with `WouldBlock` and returns at most 512 bytes) and 128 proptest
schedules on two files (`arc_2007.dwg`, `constructed_a.dwg`) reproduce it, and
`stream_r2007_timing`'s two walks agree with the assembled sections on every file,
holes counted as the zeros they stand for. A fresh process resumes at 794 stops
(every step of the tiny built file at 256-byte reads and every thirteenth at
3-byte reads; 60 to 100 stops of each other file at 256 and at 7-byte reads,
around every section, page, hole and the end of the walk): the source reads and
output of the two processes concatenate to the uninterrupted run's, the bytes written
equal the section, all six stages were checkpointed, both passes of the page map,
the section map and a data page, a page scan, and nine of the decoder's
fourteen modes. Bit flips at every byte and every truncation of a 395-byte
checkpoint, 100 sampled of each for a walking one, a wrong identity, 129
resealed unreachable states (a stage that does not follow, an odd name length, an
encrypted section, codewords read past the page, bytes produced past it, a
corrupted header) are rejected, and 180 resealed single-byte mutations of the
stage checkpoints never panic. On `arc_2007.dwg` and `sample_AC1021.dwg` 41
corruptions end in their named error with exactly the walk before them printed
(a flipped byte in each of the header's codewords, check data, the page map, the
section map and in a page's message, parity and padding; a bare page's stored
CRC; another release's magic); on a built file 44 rules broken alone with every
codeword intact each end in theirs: each of the header's five CRCs and seven
fixed fields, the system pages' four CRCs, a data page's CRC and checksum, an
encrypted section, an unknown encoding, a page the page map lacks or lists too
small, an overlap, bad section names, page map and section map entries that end
inside a record or break a rule, and a stream that reaches before its start,
overruns its page or ends early; and a source cut inside the head, the section
map or a data page ends in `Truncated`. The header's random padding, which the
format leaves unprotected, is not read: a flipped byte there changes nothing.

Not measured: a stored file header (negative `ComprLen`) and a non-zero CRC seed
on a drawing AutoCAD wrote (only built files have them, so the seed plumbing is
checked against the spec's text, not against AutoCAD); an R2007 section with
encryption 1; a drawing larger than 1.1 MB (a page map of a few thousand pages
costs one pass over it per page reference); a browser runtime.

**Throughput** (not a gate; `cargo run --release --example stream_r2007_timing --
<file> 15` in memory, best of 15, `SliceSource`, 256-byte scratch): the walk
takes about 20 ms for `sample_AC1021.dwg` (1.06 MB file, 1.40 MB of section
bytes) against 4.3 ms for `r2007::Container::parse` and `read_section` of every
section, that is 53.6 MB/s of the file and 70.7 MB/s of section bytes against
247 and 327 MB/s; on `arc_2007.dwg` (66 KB) 63 and 179 MB/s against 410 and 1,160
MB/s. The legacy reader checks no CRC, checksum or syndrome and decodes each page
once; the walk checks all of them, decodes each page twice (a page's checksum is
of its decompressed bytes, so the first pass must decode it before the second
may hand any of it out), reads each codeword byte by byte through a gather and
scans the page map once per page reference. Measured on one Apple-silicon Mac
under other load.

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
`lz77-corpus.json` is a follow-up. R2007 uses a different compression (§5.10),
decoded by its own bounded decoder in the R2007 walk below.

## Cross-runtime restore (R2004 and R2007 walks)

`tools/streaming/wasm_resume.rs` is a `#![no_std]` WebAssembly module built
from the identical grammar files (`rustc --target wasm32-unknown-unknown`,
no allocator). A host copies the source bytes, a checkpoint and the caller's
cursors into its scratch memory, calls an export and reads back the JSON
lines the native examples print: `resume_maps` restores a `DWGM`
checkpoint and finishes the page map and section map walk, `resume_section`
restores a `DWGS` checkpoint (given the section's rows of the caller's page
index and the saved cursors) and finishes that section, and `run_maps` walks
for a number of steps and leaves a `DWGM` checkpoint for the native build.
`resume_file` restores a `DWGF` checkpoint of the whole-file walker, given
only the saved page CRC-32, length and number, and finishes the whole walk;
`run_file` walks for a number of steps and leaves a `DWGF` checkpoint.
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

The whole-file walker crosses both ways, from its checkpoint alone: about 60
native `DWGF` checkpoints per file (the fixture, the three real files and a
built file whose pages are shuffled in the file and the page map), around every
section, page and the end of the walk, restored in the kernel with no page
index and no section list, finish with the lines the native resume prints, at
kernel read sizes of 1, 7, 64 and 256 bytes; the kernel's own `DWGF`
checkpoint at the same step is byte-identical to the native one in every case
(55, 61, 60, 61, 62 and 62 of as many), with equal output cursors, and the
native build resumes it to the uninterrupted run's output. A sampled bit flip of
a native checkpoint of each of the three walks, and a wrong identity, are
refused by the kernel.

The R2007 walker crosses both ways from its checkpoint alone too: `run_r2007`
walks for a number of steps and leaves a `DWGR` checkpoint, `resume_r2007`
restores one given the saved page CRC-32, page length, section index and page id
and finishes the whole walk, printing the lines `stream_r2007` prints. The
grammar compiles unchanged to `wasm32-unknown-unknown` with no allocator; the
kernel reads a gather one byte at a time from its copy of the file.
`tools/streaming/verify_wasm_r2007.py` is its acceptance run: on `tiny.dwg`, `constructed_a.dwg` and
`constructed_b.dwg` (files `r2007_ref.build` writes), `arc_2007.dwg` and
`sample_AC1021.dwg`, 349 native `DWGR` checkpoints (every thirteenth step of the
smallest, and 32 to 102 steps of each of the others, around every section, page,
hole and the end of the walk, covering all six stages) restored in the kernel
finish with exactly the lines the native resume prints, at kernel read sizes of
1, 7, 64 and 256 bytes; the kernel's own checkpoint at the same step is
byte-identical to the native one in all 349 and the native build resumes it to
the uninterrupted run's output; and the kernel refuses 80 sampled bit flips of a
native checkpoint, its first and last byte and a wrong identity (83 refusals), as
the native build does.

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
cargo build --locked --example stream_walk --example stream_lz77 --example stream_r2004 --example stream_r2004_maps --example stream_r2004_sections --example stream_r2004_file --example stream_r2004_objects --example stream_r2007 --example stream_r2007_timing --example streaming_schedules
python3 tools/streaming/verify_walk.py target/debug/examples/stream_walk target/debug/examples/streaming_schedules
python3 tools/streaming/verify_lz77.py target/debug/examples/stream_lz77 target/debug/examples/streaming_schedules
python3 tools/streaming/verify_source.py target/debug/examples/stream_walk
python3 tools/streaming/verify_r2004.py target/debug/examples/stream_r2004 target/debug/examples/streaming_schedules
python3 tools/streaming/verify_maps.py target/debug/examples/stream_r2004_maps target/debug/examples/streaming_schedules
python3 tools/streaming/verify_sections.py target/debug/examples/stream_r2004_sections target/debug/examples/streaming_schedules
python3 tools/streaming/verify_file.py target/debug/examples/stream_r2004_file target/debug/examples/streaming_schedules
python3 tools/streaming/verify_wasm.py target/debug/examples/stream_r2004_maps target/debug/examples/stream_r2004_sections target/debug/examples/stream_r2004_file
python3 tools/streaming/verify_objects.py target/debug/examples/stream_r2004_objects target/debug/examples/streaming_schedules
python3 tools/streaming/verify_wasm_objects.py target/debug/examples/stream_r2004_objects
python3 tools/streaming/verify_r2007.py target/debug/examples/stream_r2007 target/debug/examples/streaming_schedules --timing target/debug/examples/stream_r2007_timing
python3 tools/streaming/verify_wasm_r2007.py target/debug/examples/stream_r2007
python3 tools/streaming/probe_r2007_checks.py <the eight pinned R2007 drawings>
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
cross-runtime checkpoint acceptance is established only for the R2004 walks,
the R2004 object stream and the R2007 walk (above).

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
