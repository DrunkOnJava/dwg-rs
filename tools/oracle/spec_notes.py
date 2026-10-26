"""Write what the DXF value oracle measured into docs/data/spec-coverage.tsv.

Reads the text report of `examples/dxf_oracle` (its field matrices and its
summary by spec section) and replaces each measured section's
"DXF oracle (examples/dxf_oracle): ..." note, leaving the rest of the note
and the state alone. Run tools/ci/spec_coverage.py --write afterwards.

    cargo run --release --features dxf-reader --example dxf_oracle -- <corpus> > oracle.txt
    python3 tools/oracle/spec_notes.py oracle.txt docs/data/spec-coverage.tsv
    python3 tools/ci/spec_coverage.py --write

Each note keeps its other text: the oracle's sentence replaces the previous
one in place, before the "The spec's own example ..." sentence that
`cargo run --release --example spec_examples -- --update` writes last, so
the two tools can run in either order.

`issues` names the issue each disagreeing section's disagreements are filed
under; `gaps` the filed gap that keeps an otherwise agreeing section short
of `complete`. Update both when an issue is filed or closed.
"""
import re, sys, collections
report, tsv = sys.argv[1], sys.argv[2]
text = open(report).read()
# per-type releases with records, from the field tables
rels = {}
order = None
for m in re.finditer(r'^(\S+) \((§[\d.]+|-)\) records per release: ([\d ]+)$', text, re.M):
    rels[m.group(1)] = [int(x) for x in m.group(3).split()]
hdr = re.search(r'^  field\s+group\s+(.*?)\s+total$', text, re.M).group(1).split()
summary = text[text.index('== Summary by spec section'):]
rows = collections.OrderedDict()
for line in summary.splitlines():
    p = line.split()
    if not p or not p[0].startswith('§'):
        continue
    sec, ty = p[0], p[1]
    n = list(map(int, p[2:9]))
    rows.setdefault(sec, []).append((ty, n))
issues = {'§20.4.58': '#242', '§20.4.59': '#241', '§20.4.65': '#241', '§20.4.6': '#243', '§20.4.64': '#244',
          '§20.4.60': '#256'}
# Filed gaps that keep an otherwise agreeing section from `complete`.
gaps = {'§20.4.54': 'the true colour is not surfaced (#245)'}
notes = {}
for sec, items in rows.items():
    parts = []
    for ty, (records, eq, de, di, dw, nd, un) in items:
        if records == 0:
            parts.append((f"{ty}: " if len(items) > 1 else "") + f"{un} paired records not decoded or not scored")
            continue
        if eq + de + di + dw == 0:
            per = rels.get(ty, [])
            parts.append(f"{records} records paired on every release that holds them; the decoder surfaces no field to compare; {nd} DXF groups not decoded")
            continue
        per = rels.get(ty, [])
        bands = [hdr[i] for i, k in enumerate(per) if k > 0]
        span = bands[0] if len(bands) == 1 else f"{bands[0]}-{bands[-1]}"
        if 'R14' in bands:
            span += " (R14 against its R12 export)"
        agree = "every decoded value equals AutoCAD's" if di == 0 else f"{di} values disagree ({issues.get(sec, '#99')})"
        s = f"{records} records {span}, {agree} ({eq} equal, {de} by the DXF default, {dw} with no DXF group); {nd} DXF groups not decoded"
        if un:
            s += f"; {un} more paired records not decoded"
        if sec in gaps:
            s += f"; {gaps[sec]}"
        parts.append(s if len(items) == 1 else f"{ty}: {s}")
    notes[sec.lstrip('§')] = "DXF oracle (examples/dxf_oracle): " + "; ".join(parts) + "."
# The oracle's sentence alone: it ends at the note's end or where the
# sentence `spec_examples -- --update` keeps last begins.
SPEC_EXAMPLE = "The spec's own example"
ORACLE = re.compile(r' ?DXF oracle \(examples/dxf_oracle\):.*?\.(?= The spec\'s own example|$)')
out = []
for line in open(tsv).read().splitlines():
    cells = line.split('\t')
    if not line.startswith('#') and len(cells) == 7 and cells[0] in notes:
        note = ORACLE.sub('', cells[6]).strip()
        at = note.find(SPEC_EXAMPLE)
        head, tail = (note[:at].rstrip(), note[at:]) if at >= 0 else (note, '')
        cells[6] = ' '.join(x for x in (head, notes[cells[0]], tail) if x)
        line = '\t'.join(cells)
    out.append(line)
open(tsv, 'w').write('\n'.join(out) + '\n')
print(len(notes), 'sections annotated')
