#!/usr/bin/env python3
"""Regenerate and audit tests/fixtures/spec-examples/ from the spec PDF (#107).

The fixtures are the worked examples and printed tables of the ODA *Open
Design Specification for .dwg files* v5.4.1, the freely-redistributable PDF
named in CLEANROOM.md. `examples/spec_examples.rs` runs the crate's real
code paths on them. This script is how they were made and how anyone can
check them against the PDF:

    python3 tools/spec/extract_examples.py --pdf <spec.pdf> --check
    python3 tools/spec/extract_examples.py --pdf <spec.pdf> --write

Input: the PDF whose sha256 is SPEC_SHA256 below. The text is read with
`pdftotext -layout` (Poppler); page numbers are the PDF's own, which are the
numbers printed in its page headers.

Generated (--write rewrites them, keeping the hand-maintained `expect`
column of r14-objects.tsv):

- r14-objects.tsv  every "OBJECT:" dump of a §20.4.N.1 example
- r14-header.hex   the §9 "dump of a complete R14 header"
- crc8-table.txt, crc32-table.txt, magic-sequence.txt, r2007-copy-blocks.tsv

Hand-transcribed (bitcodes.tsv, lz77.tsv, sentinels.tsv, locator-xor.tsv):
--check confirms each row's `quote` column occurs on the page it cites.

Every dump is cross-checked before it is written: its row addresses must be
contiguous, each row's hex must equal its printed bit column where that
column is complete, its MS must equal the length in its header line, and
its printed CRC must equal CRC-8 (§2.14.1, seed 0xC0C1) over its MS and
data. A dump that fails any of these is an extraction error, not data.
"""

import argparse
import hashlib
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "tests" / "fixtures" / "spec-examples"
SPEC_SHA256 = "973628f353535b81a0be8a31f965bca4fb88b989127b2c1029d28f33be0f504b"
SOURCE = (
    "ODA Open Design Specification for .dwg files v5.4.1 "
    f"(sha256 {SPEC_SHA256})"
)

ROW = re.compile(r"(?:^|\s{2,})([0-9A-F]{5})((?: [0-9A-F]{2}){1,8})(?=\s{2,}|\s*$)")
NIBBLES = re.compile(r"\b[01]{4}\b")
OBJECT = re.compile(
    r"OBJECT:\s+(.+?)\s+\(([0-9A-F]+)H\),\s+len\s+([0-9A-F]+)H\s+\((\d+)\),"
    r"\s+handle:\s+([0-9A-F]{2}(?: [0-9A-F]{2})*)"
)
EXAMPLE = re.compile(r"^\s*(20\.4\.\d+)\.1\s+(?:R14 )?Example:")


def crc8_table():
    table = []
    for i in range(256):
        c = i
        for _ in range(8):
            c = (c >> 1) ^ 0xA001 if c & 1 else c >> 1
        table.append(c)
    return table


CRC8 = crc8_table()


def crc8(seed, data):
    for b in data:
        seed = (seed >> 8) ^ CRC8[(seed ^ b) & 0xFF]
    return seed


def pages_of(pdf):
    digest = hashlib.sha256(pdf.read_bytes()).hexdigest()
    if digest != SPEC_SHA256:
        sys.exit(f"{pdf}: sha256 {digest}, expected {SPEC_SHA256}")
    text = subprocess.run(
        ["pdftotext", "-layout", str(pdf), "-"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return text.split("\f")


def numbered_lines(pages, first, last):
    for number in range(first, last + 1):
        for line in pages[number - 1].splitlines():
            yield number, line


def rows_in(line):
    """Every (address, bytes, rest-of-row) a text line holds; pdftotext
    occasionally sets two dump rows on one line."""
    found = list(ROW.finditer(line))
    for i, m in enumerate(found):
        end = found[i + 1].start() if i + 1 < len(found) else len(line)
        yield int(m.group(1), 16), [int(x, 16) for x in m.group(2).split()], line[m.end() : end]


def check_bits(address, data, rest, problems):
    nibbles = NIBBLES.findall(rest)
    if len(nibbles) == 2 * len(data):
        printed = [int(nibbles[2 * i] + nibbles[2 * i + 1], 2) for i in range(len(data))]
        if printed != data:
            problems.append(f"row {address:05X}: hex and bit columns differ")


def extract_objects(pages):
    dumps, current, section = [], None, None
    for page, line in numbered_lines(pages, 100, len(pages)):
        heading = EXAMPLE.match(line)
        if heading:
            section = heading.group(1)
        header = OBJECT.search(line)
        if header:
            current = {
                "section": section,
                "pages": [page],
                "label": header.group(1),
                "type": header.group(2),
                "len": int(header.group(3), 16),
                "len_dec": int(header.group(4)),
                "handle": header.group(5).replace(" ", ""),
                "rows": [],
                "crc": None,
            }
            dumps.append(current)
            line = line[header.end() :]
        if current is None:
            continue
        if line.strip() == "ENDOBJECT":
            current = None
            continue
        for address, data, rest in rows_in(line):
            if page not in current["pages"]:
                current["pages"].append(page)
            if rest.strip() == "crc":
                current["crc"] = (address, data)
                current = None
                break
            current["rows"].append((address, data, rest))
    for d in dumps:
        problems, record = [], []
        expected = d["rows"][0][0]
        for address, data, rest in d["rows"]:
            if address != expected:
                problems.append(f"row {address:05X} follows {expected:05X}")
            check_bits(address, data, rest, problems)
            record.extend(data)
            expected = address + len(data)
        if d["crc"] is None or d["crc"][0] != expected or len(d["crc"][1]) != 2:
            problems.append("no CRC row directly after the data")
        ms = record[0] | record[1] << 8
        if ms & 0x8000:
            ms = (ms & 0x7FFF) | ((record[2] | record[3] << 8) & 0x7FFF) << 15
        if not (ms == d["len"] == d["len_dec"]):
            problems.append(f"MS {ms}, header len {d['len']} / {d['len_dec']}")
        if not problems:
            stored = d["crc"][1][0] | d["crc"][1][1] << 8
            if crc8(0xC0C1, bytes(record)) != stored:
                problems.append("printed CRC is not CRC-8(0xC0C1) of MS and data")
        if problems:
            sys.exit(f"§{d['section']} {d['label']}: " + "; ".join(problems))
        d["record"] = bytes(record + d["crc"][1])
    return dumps


def extract_header_dump(pages):
    record, expected = [], 0
    for page, line in numbered_lines(pages, 80, 83):
        for address, data, _ in rows_in(line):
            if address != expected:
                sys.exit(f"§9 dump: row {address:05X} follows {expected:05X}")
            record.extend(data)
            expected = address + len(data)
    return bytes(record)


def hex_words(pages, first, last, start, stop, width):
    words, inside = [], False
    for _, line in numbered_lines(pages, first, last):
        if start in line:
            inside = True
            line = line.split(start, 1)[1]
        if not inside:
            continue
        if stop and stop in line:
            words.extend(re.findall(rf"0x[0-9A-Fa-f]{{{width}}}\b", line.split(stop, 1)[0]))
            break
        words.extend(re.findall(rf"0x[0-9A-Fa-f]{{{width}}}\b", line))
    return [int(w, 16) for w in words]


def extract_magic(pages):
    out = []
    for _, line in numbered_lines(pages, 23, 23):
        if re.fullmatch(r"\s*(?:[0-9A-F]{2} ){3,15}[0-9A-F]{2}\s*", line):
            out.extend(int(x, 16) for x in line.split())
    return out


def extract_copy_blocks(pages):
    rows = {}
    for _, line in numbered_lines(pages, 54, 55):
        m = re.match(r"^\s*(\d{1,2})\s{3,}((?:\d+\s*\[\d+\],?\s*)+)$", line)
        if m:
            blocks = re.findall(r"(\d+)\s*\[(\d+)\]", m.group(2))
            rows[int(m.group(1))] = " ".join(f"{s}[{i}]" for s, i in blocks)
    return rows


def header(what, pages):
    return [f"# {what}", f"# Source: {SOURCE}, {pages}.", "# Regenerate: tools/spec/extract_examples.py"]


def render(pages, expects):
    files = {}
    dumps = extract_objects(pages)
    lines = header(
        "§20.4.N.1 R14 examples: every OBJECT dump the spec prints, as the on-disk record "
        "(MS, data, the two printed CRC bytes)",
        "pages 109-240",
    ) + [
        "# The spec prints type, length and handle in each dump's header line and the CRC after it;",
        "# it prints no decoded field values. `expect` is hand-maintained: closes | unhandled | walk-only | error #N.",
        "section\tpages\tlabel\ttype\tlen\thandle\tcrc\texpect\trecord",
    ]
    for d in dumps:
        key = (d["section"], d["handle"])
        pages_cell = "-".join(str(p) for p in (d["pages"][0], d["pages"][-1])) if len(d["pages"]) > 1 else str(d["pages"][0])
        lines.append(
            "\t".join(
                [
                    d["section"],
                    pages_cell,
                    d["label"],
                    d["type"],
                    str(d["len"]),
                    d["handle"],
                    d["record"][-2:].hex().upper(),
                    expects.get(key, "closes"),
                    d["record"].hex().upper(),
                ]
            )
        )
    files["r14-objects.tsv"] = lines

    dump = extract_header_dump(pages)
    lines = header('§9 "Here is a dump of a complete R14 header" (empty14.dwg)', "pages 80-83")
    for i in range(0, len(dump), 8):
        lines.append(f"{i:05X} " + " ".join(f"{b:02X}" for b in dump[i : i + 8]))
    files["r14-header.hex"] = lines

    table = hex_words(pages, 15, 16, "int crctable[256]= {", "};", 4)
    if len(table) != 256:
        sys.exit(f"§2.14.1 table: {len(table)} values")
    files["crc8-table.txt"] = header("§2.14.1 crctable[256]", "pages 15-16") + [
        " ".join(f"0x{v:04X}" for v in table[i : i + 8]) for i in range(0, 256, 8)
    ]
    table = hex_words(pages, 16, 18, "OdUInt32 crc32Table[] =", "};", 8)
    if len(table) != 256:
        sys.exit(f"§2.14.2 table: {len(table)} values")
    files["crc32-table.txt"] = header("§2.14.2 crc32Table[]", "pages 16-18") + [
        " ".join(f"0x{v:08x}" for v in table[i : i + 4]) for i in range(0, 256, 4)
    ]
    magic = extract_magic(pages)
    if len(magic) != 92:
        sys.exit(f"§4.1 magic sequence: {len(magic)} bytes printed, expected the known 92")
    files["magic-sequence.txt"] = header("§4.1 the magic number sequence as printed", "page 23") + [
        "# The text says it XORs 0x6C bytes but prints only the first 92; the generator printed",
        "# under it yields all 0x6C. examples/spec_examples.rs checks the printed prefix.",
    ] + [" ".join(f"{b:02X}" for b in magic[i : i + 16]) for i in range(0, len(magic), 16)]
    blocks = extract_copy_blocks(pages)
    if sorted(blocks) != list(range(1, 33)):
        sys.exit(f"§5.10.1 table: rows {sorted(blocks)}")
    files["r2007-copy-blocks.tsv"] = header(
        "§5.10.1 literal copy table: byte count, then (block size)[source index] in output order",
        "pages 54-55",
    ) + ["count\tblocks"] + [f"{n}\t{blocks[n]}" for n in range(1, 33)]
    return {name: "\n".join(lines) + "\n" for name, lines in files.items()}


def current_expects():
    path = OUT / "r14-objects.tsv"
    if not path.exists():
        return {}
    expects = {}
    for line in path.read_text().splitlines():
        if line.startswith("#") or line.startswith("section\t"):
            continue
        cells = line.split("\t")
        expects[(cells[0], cells[5])] = cells[7]
    return expects


QUOTED = ("bitcodes.tsv", "lz77.tsv", "sentinels.tsv", "locator-xor.tsv")


def squash(text):
    return re.sub(r"\s+", "", text)


def check_quotes(pages):
    problems = []
    for name in QUOTED:
        path = OUT / name
        lines = [l for l in path.read_text().splitlines() if l and not l.startswith("#")]
        columns = lines[0].split("\t")
        for line in lines[1:]:
            row = dict(zip(columns, line.split("\t")))
            first, _, last = row["page"].partition("-")
            text = squash("".join(pages[int(p) - 1] for p in range(int(first), int(last or first) + 1)))
            for quote in row["quote"].split("|"):
                if squash(quote) not in text:
                    problems.append(f"{name} {row['id']}: {quote!r} is not on page {row['page']}")
    return problems


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--pdf", required=True, type=Path)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--write", action="store_true")
    args = parser.parse_args()
    pages = pages_of(args.pdf)
    files = render(pages, current_expects())
    if args.write:
        OUT.mkdir(parents=True, exist_ok=True)
        for name, text in files.items():
            (OUT / name).write_text(text)
        print(f"wrote {len(files)} files to {OUT.relative_to(ROOT)}")
        return 0
    problems = [f"{name} differs from the PDF" for name, text in files.items() if (OUT / name).read_text() != text]
    problems += check_quotes(pages)
    for p in problems:
        print(p)
    print("ok" if not problems else f"{len(problems)} problem(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
