#!/usr/bin/env python3
"""Check what an AutoCAD trial session produced against the generator's manifest.

Walks <work>/out for every file in <work>/manifest.tsv and reports, per file: whether it exists and is
not empty, a DWG's six-byte magic, a DXF's $ACADVER and $DWGCODEPAGE (both compared with the manifest),
the DXF record types the item is meant to produce (a count of 0 means AutoCAD silently skipped it), and
for the code-page drawings the number of bytes above 0x7F and of \\U+XXXX escapes in the DXF. Each file's
byte count and sha256 are listed, so a corpus pins row is a copy of one line. Anything missing, empty or
wrong is listed afterwards as the next step to run. An export older than the master or staged input it was
saved from is STALE: the master changed after it. DXFs are read as bytes, never decoded: a pre-R2007
DXF holds code-page bytes. A `.FAILED.dwg` left by a builder error counts only while it is newer than the
item's valid master and still a drawing.

Usage:

    python3 -I tools/autocad/check.py [--work DIR] [--tsv FILE] [--quiet]

Exit status 0 when every expected file is present and right, 1 otherwise.
"""

import argparse
import csv
import hashlib
import re
import shlex
import sys
from collections import Counter
from pathlib import Path

DEFAULT_WORK = '~/Library/Caches/dwg-autocad'
ESCAPE = re.compile(rb'\\[Uu]\+[0-9A-Fa-f]{4}')


def read_manifest(path):
    with open(path, encoding='utf-8', newline='') as f:
        rows = list(csv.reader(f, delimiter='\t', quoting=csv.QUOTE_NONE, escapechar='\\'))
    head = rows[0]
    return [dict(zip(head, r)) for r in rows[1:]]


def scan_dxf(data):
    if data.startswith(b'AutoCAD Binary DXF'):
        return {'binary': True}
    lines = data.split(b'\n')
    header, types = {}, Counter()
    section, cur = None, None
    i = 0
    while i + 1 < len(lines):
        code, val = lines[i].strip(), lines[i + 1].strip()
        i += 2
        if code == b'0':
            types[val.decode('latin-1')] += 1
            if val == b'ENDSEC':
                section = None
            elif val == b'SECTION' and i + 1 < len(lines) and lines[i].strip() == b'2':
                section = lines[i + 1].strip()
        elif section == b'HEADER':
            if code == b'9':
                cur = val
            elif cur in (b'$ACADVER', b'$DWGCODEPAGE') and code in (b'1', b'3') and cur not in header:
                header[cur] = val.decode('latin-1')
    return {'binary': False, 'acadver': header.get(b'$ACADVER', '-'),
            'codepage': header.get(b'$DWGCODEPAGE', '-'), 'types': types}


def _inspect(row, out):
    path = out / row['path']
    res = {'status': 'ok', 'bytes': 0, 'sha': '-', 'version': '-', 'codepage': '-', 'info': ''}
    if not path.is_file():
        res['status'] = 'MISSING'
        return res
    data = path.read_bytes()
    res['bytes'] = len(data)
    res['sha'] = hashlib.sha256(data).hexdigest()
    if not data:
        res['status'] = 'EMPTY'
        return res
    if row['kind'] in ('dwg', 'master'):
        res['version'] = data[:6].decode('latin-1')
        if res['version'] != row['expect']:
            res['status'] = 'BAD-MAGIC'
        return res
    scan = scan_dxf(data)
    if scan['binary']:
        res['status'] = 'BINARY'
        return res
    res['version'], res['codepage'] = scan['acadver'], scan['codepage']
    if scan['acadver'] != row['expect']:
        res['status'] = 'BAD-ACADVER'
    info = []
    for spec in filter(None, row['types'].split(';')):
        name, need = spec.split(':')
        have = scan['types'].get(name, 0)
        info.append(f'{name}={have}')
        if have < int(need) and res['status'] == 'ok':
            res['status'] = 'TYPES'
    if row['flags'] == 'text':
        high = len(data.translate(None, bytes(range(128))))
        info.append(f'high={high} esc={len(ESCAPE.findall(data))}')
    res['info'] = ' '.join(info)
    return res


def inspect(row, out):
    res = _inspect(row, out)
    source = row.get('source', '-')
    if res['status'] in ('ok', 'TYPES') and source not in ('', '-'):
        origin = out.parent / source
        if origin.is_file() and (out / row['path']).stat().st_mtime < origin.stat().st_mtime:
            res['status'] = 'STALE'
    return res


def failure_artifacts(out, master_ok):
    live = []
    for p in sorted((out / 'masters').glob('*.FAILED.dwg')):
        item = p.name[:-len('.FAILED.dwg')]
        if p.read_bytes()[:4] != b'AC10':
            continue
        master = out / 'masters' / f'{item}.dwg'
        if master_ok.get(item, False) and master.stat().st_mtime >= p.stat().st_mtime:
            continue
        live.append(p)
    return live


def next_steps(rows, results, work, out):
    master_ok = {r['item']: res['status'] == 'ok' for r, res in zip(rows, results) if r['kind'] == 'master'}
    s1, build, manual, save, types = {}, {}, {}, {}, {}
    for r, res in zip(rows, results):
        if res['status'] == 'ok':
            continue
        item = r['item']
        if r['kind'] == 'master':
            (manual if r['script'].startswith('MANUAL') else build)[item] = r['script']
        elif r['section'] == 's1':
            s1[item] = r['script']
        elif master_ok.get(item, False):
            if res['status'] == 'TYPES':
                types[item] = r['types']
            else:
                save[item] = r['script']
    steps = []
    resume = f'python3 -I tools/autocad/generate.py --work {shlex.quote(str(work))} --resume'

    def group(found, whole, what):
        if not found:
            return
        if len(found) <= 3:
            for item, script in sorted(found.items()):
                steps.append(f'-SCRIPT {work / "scripts" / script}   ({item})')
        else:
            steps.append(f'{resume}, then -SCRIPT {work / "scripts" / whole}   ({len(found)} {what})')

    group(s1, 's1-all.scr', 'R13 drawings')
    group(build, 'build-all.scr', 'items to build')
    if manual:
        steps.append('build by hand and save each as <out>/masters/<item>.dwg (MANUAL-CHECKLIST.md): '
                     + ', '.join(sorted(manual)))
    group(save, 'save-all.scr', 'items to save out')
    for item, spec in sorted(types.items()):
        steps.append(f'{item}: AutoCAD wrote too few of {spec}; read the builder text in the command line, '
                     f'then MANUAL-CHECKLIST.md#{item}')
    for p in failure_artifacts(out, master_ok):
        steps.append(f'builder error: {p.name} was saved by the error handler; scroll back to its dwgrs: message')
    return steps


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('--work', default=DEFAULT_WORK)
    ap.add_argument('--tsv', help='also write the results as TSV')
    ap.add_argument('--quiet', action='store_true', help='print the summary and next steps only')
    args = ap.parse_args()
    work = Path(args.work).expanduser().resolve()
    out = work / 'out'
    manifest = work / 'manifest.tsv'
    if not manifest.is_file():
        print(f'check.py: {manifest} does not exist; run tools/autocad/generate.py first', file=sys.stderr)
        return 2
    rows = read_manifest(manifest)
    results = [inspect(r, out) for r in rows]
    info = out / 'session-info.txt'
    if info.is_file():
        print('session-info.txt')
        for line in info.read_text(encoding='utf-8', errors='replace').splitlines():
            print(f'  {line}')
        print()
    if not args.quiet:
        cols = ('status', 'item', 'kind', 'release', 'bytes', 'version', 'codepage', 'sha', 'info')
        table = [(res['status'], r['item'], r['kind'], r['release'], str(res['bytes']), res['version'],
                  res['codepage'], res['sha'][:12], res['info'])
                 for r, res in zip(rows, results) if res['status'] != 'MISSING']
        if table:
            widths = [max(len(cols[c]), *(len(t[c]) for t in table)) for c in range(len(cols))]
            print('  '.join(c.upper().ljust(w) for c, w in zip(cols, widths)).rstrip())
            for t in table:
                print('  '.join(v.ljust(w) for v, w in zip(t, widths)).rstrip())
            print()
    tally = Counter(res['status'] for res in results)
    hosting = Counter(r['hosting'] for r, res in zip(rows, results) if res['status'] == 'ok')
    print(f'{len(rows)} expected files: ' + ', '.join(f'{n} {s}' for s, n in sorted(tally.items())))
    if hosting:
        print('present by hosting: ' + ', '.join(f'{n} {h}' for h, n in sorted(hosting.items())))
    steps = next_steps(rows, results, work, out)
    if steps:
        print('\nnext steps')
        for k, s in enumerate(steps, 1):
            print(f'  {k}. {s}')
    if args.tsv:
        with open(args.tsv, 'w', encoding='utf-8', newline='') as f:
            w = csv.writer(f, delimiter='\t', lineterminator='\n')
            w.writerow(['item', 'kind', 'release', 'path', 'status', 'bytes', 'sha256', 'version',
                        'codepage', 'licence', 'hosting', 'input_sha256', 'input_name'])
            for r, res in zip(rows, results):
                w.writerow([r['item'], r['kind'], r['release'], r['path'], res['status'], res['bytes'],
                            res['sha'], res['version'], res['codepage'], r['licence'], r['hosting'],
                            r['input_sha256'], r['input_name']])
    return 0 if all(res['status'] == 'ok' for res in results) and not steps else 1


if __name__ == '__main__':
    sys.exit(main())
