#!/usr/bin/env python3
"""Generate the AutoCAD scripts, staged inputs and manifest for the trial session.

Nothing here runs AutoCAD. The run writes, under --work (default ~/Library/Caches/dwg-autocad):

    in/            the pinned R13 drawings copied to ASCII names, a two-page PDF and a 64 by 64 PNG
    lisp/          a copy of tools/autocad/lisp/*.lsp, so the scripts do not depend on the checkout
    scripts/       *.scr files for AutoCAD's SCRIPT command
    out/           created empty; AutoCAD writes every output here
    manifest.tsv   every expected output with its expected magic or $ACADVER, for check.py
    session.json   the options of this run, which --resume reads back

Scripts, in the order a session runs them (tools/autocad/README.md has the runbook):

    00-start.scr        records DWGCODEPAGE, SYSCODEPAGE and the build; checks the character mode
    s1-all.scr          section 1: OPEN each pinned R13 drawing, SAVEAS R2000 DWG, SAVEAS R2000 ASCII DXF
    build-all.scr       sections 2 to 4: NEW, load the AutoLISP builder, build, save a master DWG
    save-all.scr        sections 2 to 4: reopen each master and save every release and its ASCII DXF
    (and s1-<name>.scr, build-<sec>.scr, save-<sec>.scr, items/*.scr to rerun one piece)

save-*.scr are written only for masters that exist when the generator runs, so run it again after
the builders and the manual drawings have saved their masters. --resume leaves out whatever already
has all its outputs.

Usage:

    python3 -I tools/autocad/generate.py [--corpus DIR] [--pins FILE] [--work DIR] [--template NAME]
        [--sections 1,2,3,4] [--sources LIST] [--char-mode chr|escape] [--scripted-only]
        [--keyword-prefix _] [--resume] [--allow-missing]

--scripted-only leaves out the drawings built by hand. --keyword-prefix _ writes the DXF and Version
keywords of SAVEAS with the underscore that selects the English keyword, for a session in another
language. Every run saves its options in <work>/session.json; --resume reads them back for whatever
the command line does not give, and counts an output as done only when check.py would call it valid.

The SAVEAS and DXF prompt sequences follow Autodesk's documented command line; the DXF form
(SAVEAS DXF V <version> 16 <file>) and the R14 keyword LT98 also match the script that saved
the pinned ACadSharp sample_base twins (CLEANROOM.md, "Public corpus").
"""

import argparse
import csv
import hashlib
import importlib.util
import json
import shutil
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.dont_write_bytecode = True
_spec = importlib.util.spec_from_file_location('autocad_check', HERE / 'check.py')
CHECK = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(CHECK)
KEYWORD_PREFIX = ''
REPO = HERE.parents[1]
DEFAULT_PINS = REPO / 'tools' / 'corpus' / 'pins.tsv'
DEFAULT_CORPUS = '~/Library/Caches/dwg-corpus'
DEFAULT_WORK = '~/Library/Caches/dwg-autocad'

RELEASES = {
    'R14': ('LT98', 'AC1014', 'R12', 'AC1009', 'R12'),
    'R2000': ('2000', 'AC1015', '2000', 'AC1015', 'R2000'),
    'R2004': ('2004', 'AC1018', '2004', 'AC1018', 'R2004'),
    'R2007': ('2007', 'AC1021', '2007', 'AC1021', 'R2007'),
    'R2010': ('2010', 'AC1024', '2010', 'AC1024', 'R2010'),
    'R2013': ('2013', 'AC1027', '2013', 'AC1027', 'R2013'),
    'R2018': ('2018', 'AC1032', '2018', 'AC1032', 'R2018'),
}
ORDER = ['R2018', 'R2013', 'R2010', 'R2007', 'R2004', 'R2000', 'R14']
RANK = {r: i for i, r in enumerate(ORDER)}
ALL6 = ['R2018', 'R2013', 'R2010', 'R2007', 'R2004', 'R2000']
MODERN = ['R2018', 'R2013', 'R2010', 'R2007']
PDFREL = ['R2018', 'R2013', 'R2010']
TEXTREL = ['R2004', 'R2000', 'R14']

OPEN_NOTE = ('VERIFY: OPEN of a third-party R13 drawing may raise a recover or proxy prompt that stops the script; '
             'CLOSE should not ask to save after SAVEAS')
SAVE_NOTE = 'VERIFY: SAVEAS to R14 (LT98) may warn about unsupported objects; CLOSE should not ask to save after SAVEAS DXF'


class Item:
    def __init__(self, name, lisp, releases, types=(), note=''):
        self.name = name
        self.lisp = lisp
        self.releases = releases
        self.types = list(types)
        self.note = note

    @property
    def section(self):
        return self.name.split('-')[0]

    @property
    def origin(self):
        return 'lisp' if self.lisp else 'manual'


CATALOG = [
    Item('s2-material', 'dwgrs-objects', ALL6, ['MATERIAL:5@R2007'],
         'five materials; entmakex may refuse MATERIAL, then MANUAL-CHECKLIST.md'),
    Item('s2-material-maps', None, MODERN, ['MATERIAL:1@R2007'], 'MATERIAL map settings: manual'),
    Item('s2-wipeout-0', 'dwgrs-objects', ALL6, ['WIPEOUTVARIABLES:1@R2000'], 'WIPEOUTFRAME 0'),
    Item('s2-wipeout-1', 'dwgrs-objects', ALL6, ['WIPEOUTVARIABLES:1@R2000'], 'WIPEOUTFRAME 1'),
    Item('s2-wipeout-2', 'dwgrs-objects', ALL6, ['WIPEOUTVARIABLES:1@R2000'], 'WIPEOUTFRAME 2'),
    Item('s2-dbcolor', 'dwgrs-objects', ALL6, ['DBCOLOR:2@R2004'], 'two colour books'),
    Item('s2-pdf', 'dwgrs-objects', PDFREL, ['PDFDEFINITION:2@R2010', 'PDFUNDERLAY:2@R2010'],
         'page 1 and page 2 of the generated PDF'),
    Item('s2-acsh-primitives', 'dwgrs-solids', MODERN,
         ['ACSH_BOX_CLASS:3@R2007', 'ACSH_CYLINDER_CLASS:2@R2007', 'ACSH_SPHERE_CLASS:2@R2007'],
         'boxes, cylinders and spheres with history, one with a true colour'),
    Item('s2-acsh-extrusion', 'dwgrs-solids', MODERN, ['ACSH_EXTRUSION_CLASS:3@R2007'],
         'circle, tapered rectangle, circle along an arc path, with moves and a rotation'),
    Item('s2-dimassoc', 'dwgrs-annotation', ALL6, ['DIMASSOC:8@R2000'],
         'linear dimensions on END, MID, QUA, CEN, INT, NEA, a diameter and an angular with three references'),
    Item('s2-dyn-polar', None, ALL6, ['ACAD_EVALUATION_GRAPH:1@R2004'], 'dynamic block: polar parameter'),
    Item('s2-dyn-vis3', None, ALL6, ['ACAD_EVALUATION_GRAPH:1@R2004'], 'dynamic block: visibility, 3 states'),
    Item('s2-dyn-vis5', None, ALL6, ['ACAD_EVALUATION_GRAPH:1@R2004'], 'dynamic block: visibility, 5 states'),
    Item('s2-dyn-scale-b', None, ALL6, ['ACAD_EVALUATION_GRAPH:1@R2004'], 'dynamic block: scale action, other base point'),
    Item('s3-textctx', 'dwgrs-annotation', MODERN, ['ACDB_TEXTOBJECTCONTEXTDATA_CLASS:6@R2007'],
         '#337: annotative TEXT, rotations 30 and 75, offsets, flags, two scales'),
    Item('s3-pdfunderlay-a', 'dwgrs-objects', PDFREL, ['PDFUNDERLAY:1@R2010', 'PDFDEFINITION:1@R2010'],
         '#331: scale 2.5, rotation 30, rotated UCS, 5-vertex clip'),
    Item('s3-pdfunderlay-b', 'dwgrs-objects', PDFREL, ['PDFUNDERLAY:1@R2010', 'PDFDEFINITION:1@R2010'],
         '#331: rectangular clip'),
    Item('s3-sun-off', 'dwgrs-objects', MODERN, ['SUN:1@R2007'], '#332: status off, DST on, shadows off, type 1, map 512, softness 3'),
    Item('s3-sun-on', 'dwgrs-objects', MODERN, ['SUN:1@R2007'], '#332: status on, shadows on, type 2, map 1024, softness 5'),
    Item('s3-datatable', None, MODERN, ['DATATABLE:1@R2007'], '#335: no core command creates a DATATABLE; exploratory'),
    Item('s3-dyn-eg1', None, MODERN, ['ACAD_EVALUATION_GRAPH:1@R2007'], 'one linear parameter, stretch and scale'),
    Item('s3-dyn-eg2', None, MODERN, ['ACAD_EVALUATION_GRAPH:1@R2007'], 'two linear parameters, stretch and scale each'),
    Item('s3-dyn-eg3', None, MODERN, ['ACAD_EVALUATION_GRAPH:1@R2007'], 'three linear parameters, stretch and scale each'),
    Item('s3-pers-1', 'dwgrs-solids', ['R2013', 'R2010'],
         ['ACDBPERSSUBENTMANAGER:1@R2010', 'ACDBASSOCPERSSUBENTMANAGER:1@R2010'], 'empty drawing'),
    Item('s3-pers-2', 'dwgrs-solids', ['R2013', 'R2010'],
         ['ACDBPERSSUBENTMANAGER:1@R2010', 'ACDBASSOCPERSSUBENTMANAGER:1@R2010'], 'one box, SOLIDHIST on'),
    Item('s3-pers-3', 'dwgrs-solids', ['R2013', 'R2010'],
         ['ACDBPERSSUBENTMANAGER:1@R2010', 'ACDBASSOCPERSSUBENTMANAGER:1@R2010'], 'two boxes'),
    Item('s3-pers-4', 'dwgrs-solids', ['R2013', 'R2010'],
         ['ACDBPERSSUBENTMANAGER:1@R2010', 'ACDBASSOCPERSSUBENTMANAGER:1@R2010'], 'one box, one edge filleted'),
    Item('s3-pers-5', 'dwgrs-solids', ['R2013', 'R2010'],
         ['ACDBPERSSUBENTMANAGER:1@R2010', 'ACDBASSOCPERSSUBENTMANAGER:1@R2010'], 'one extruded profile'),
    Item('s3-pers-6', 'dwgrs-solids', ['R2013', 'R2010'],
         ['ACDBPERSSUBENTMANAGER:1@R2010', 'ACDBASSOCPERSSUBENTMANAGER:1@R2010'], 'one box, associative dimension on an edge'),
    Item('s3-pers-7', 'dwgrs-solids', ['R2013', 'R2010'],
         ['ACDBPERSSUBENTMANAGER:1@R2010', 'ACDBASSOCPERSSUBENTMANAGER:1@R2010'], 'two boxes erased, then PURGE'),
    Item('s3-plinegen-mesh', 'dwgrs-objects', ALL6, ['LWPOLYLINE:3@R2000', 'POLYLINE:1@R2000'],
         '#358: LWPOLYLINE with PLINEGEN on, on and closed, off; a 3x3 polygon mesh'),
    Item('s3-ucs', 'dwgrs-objects', ['R2004', 'R2000'], ['UCS:3@R2000'], '#285: saved UCS records and an active UCS'),
    Item('s3-xref-src', 'dwgrs-objects', [], [], 'xref source for s3-xref-host (master only)'),
    Item('s3-xref-host', 'dwgrs-objects', ALL6, [], '#290: xref-dependent layer, linetype, style and block'),
    Item('s4-latin1', 'dwgrs-codepage', TEXTREL, [], '#375: Latin-1 characters from chr'),
    Item('s4-latin1-esc', 'dwgrs-codepage', TEXTREL, [], '#375: the same text as U+XXXX escapes'),
    Item('s4-multi', 'dwgrs-codepage', TEXTREL, [], '#375: Latin-1 plus Cyrillic, Greek and Japanese as escapes'),
    Item('s4-multi-typed', None, TEXTREL, [], '#375: the same characters typed into AutoCAD'),
]
BY_NAME = {i.name: i for i in CATALOG}


def die(msg):
    print(f'generate.py: {msg}', file=sys.stderr)
    raise SystemExit(1)


def check_path(p):
    s = str(p)
    if not s.isascii() or any(c in s for c in '";\r\n\t'):
        die(f'{s}: AutoCAD script paths must be ASCII and hold no quote, semicolon or tab; pass --work with a plain path')
    return s


def scr_path(p):
    s = check_path(p)
    return f'"{s}"' if ' ' in s else s


def lisp_str(p):
    return '"' + check_path(p).replace('\\', '\\\\') + '"'


def sha256_of(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()


def read_pins(path):
    with open(path, encoding='utf-8', newline='') as f:
        lines = [ln for ln in f if ln.strip() and not ln.startswith('#')]
    rows = list(csv.reader(lines, delimiter='\t', quoting=csv.QUOTE_NONE))
    head = rows[0]
    return [dict(zip(head, r)) for r in rows[1:]]


def hosting(licence):
    low = licence.lower()
    if low.startswith(('apache', 'mit', 'bsd')):
        return 'pin-with-attribution'
    if low.startswith('public domain'):
        return 'pin'
    if low.startswith('none'):
        return 'local-only'
    return 'review'


def two_page_pdf():
    pages = [(612, 792, 'DWGRS PAGE 1'), (595, 842, 'DWGRS PAGE 2')]
    objs = [b'<< /Type /Catalog /Pages 2 0 R >>', b'<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>']
    for i, (w, h, label) in enumerate(pages):
        stream = f'2 w 30 30 {w - 60} {h - 60} re S BT /F1 36 Tf 60 {h // 2} Td ({label}) Tj ET'.encode()
        objs.append(f'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] /Contents {4 + 2 * i} 0 R '
                    f'/Resources << /Font << /F1 7 0 R >> >> >>'.encode())
        objs.append(b'<< /Length %d >>\nstream\n' % len(stream) + stream + b'\nendstream')
    objs.append(b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>')
    out = bytearray(b'%PDF-1.4\n')
    offsets = []
    for n, body in enumerate(objs, 1):
        offsets.append(len(out))
        out += f'{n} 0 obj\n'.encode() + body + b'\nendobj\n'
    xref = len(out)
    out += f'xref\n0 {len(objs) + 1}\n0000000000 65535 f \n'.encode()
    for off in offsets:
        out += f'{off:010d} 00000 n \n'.encode()
    out += f'trailer\n<< /Size {len(objs) + 1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n'.encode()
    return bytes(out)


def map_png():
    import struct
    import zlib

    def chunk(kind, body):
        return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', zlib.crc32(kind + body) & 0xffffffff)

    w = h = 64
    rows = b''.join(b'\x00' + b''.join(bytes((255, 255, 255) if (x // 8 + y // 8) % 2 else (200, 60, 10))
                                       for x in range(w)) for y in range(h))
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b''))


class Scr:
    def __init__(self, title):
        self.lines = [f'; {title}', '; generated by tools/autocad/generate.py; do not edit, rerun the generator',
                      '; VERIFY: prompt sequences are confirmed only against a published AutoCAD 2025 script; see README.md']

    def comment(self, text):
        self.lines.append(f'; {text}')

    def cmd(self, *tokens):
        self.lines.append(' '.join(tokens))

    def lisp(self, expr):
        self.lines.append(expr)

    def setup(self, expert):
        self.cmd('_.SETVAR', 'FILEDIA', '0')
        self.cmd('_.SETVAR', 'CMDDIA', '0')
        if expert:
            self.cmd('_.SETVAR', 'PROXYNOTICE', '0')
            self.cmd('_.SETVAR', 'EXPERT', '2')

    def teardown(self, expert):
        if expert:
            self.cmd('_.SETVAR', 'EXPERT', '0')
            self.cmd('_.SETVAR', 'PROXYNOTICE', '1')
        self.cmd('_.SETVAR', 'CMDDIA', '1')
        self.cmd('_.SETVAR', 'FILEDIA', '1')

    def write(self, path):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text('\n'.join(self.lines) + '\n', encoding='ascii')


def save_block(scr, base, rels, outdir):
    for rel in sorted(rels, key=RANK.get):
        ver, _magic, dxfver, _acad, dxftag = RELEASES[rel]
        scr.cmd('_.SAVEAS', ver, scr_path(outdir / f'{base}.{rel}.dwg'))
        scr.cmd('_.SAVEAS', KEYWORD_PREFIX + 'DXF', KEYWORD_PREFIX + 'V', dxfver, '16',
                scr_path(outdir / f'{base}.{dxftag}.dxf'))


def applicable_types(item, rel):
    out = []
    for spec in item.types:
        name, rest = spec.split(':')
        count, frm = rest.split('@')
        if RANK[rel] <= RANK[frm]:
            out.append(f'{name}:{count}')
    return ';'.join(out)


class Plan:
    def __init__(self, work, out, resume):
        self.work = work
        self.out = out
        self.resume = resume
        self.rows = []

    def add(self, **kw):
        self.rows.append(kw)

    def valid(self, rel_path):
        row = next((r for r in self.rows if r['path'] == rel_path), None)
        if row is None:
            return (self.out / rel_path).exists()
        return CHECK.inspect(row, self.out)['status'] in ('ok', 'TYPES')

    def item_done(self, rel_paths):
        return self.resume and all(self.valid(p) for p in rel_paths)


COLUMNS = ['section', 'item', 'kind', 'release', 'path', 'source', 'expect', 'types', 'flags',
           'input_sha256', 'input_name', 'licence', 'hosting', 'script', 'note']


def s1_plan(args, plan, pins):
    rows = [r for r in pins if r['release'] == 'AC1012' and r['kind'] == 'dwg']
    if args.sources:
        keep = set(args.sources.split(','))
        rows = [r for r in rows if r['source'] in keep]
    if not rows:
        print('section 1: no AC1012 DWG rows in the pins; until PR #323 lands on main, pass --pins with the '
              'pins.tsv of its branch', file=sys.stderr)
    counters = {}
    entries, missing = [], []
    for r in rows:
        n = counters.get(r['source'], 0) + 1
        counters[r['source']] = n
        stem = f"{r['source']}-{n:02d}-{r['sha256'][:8]}"
        src = Path(args.corpus).expanduser() / r['source'] / Path(r['path']).name
        if not src.is_file():
            missing.append(f'{src}: not in the corpus directory')
            continue
        if sha256_of(src) != r['sha256']:
            missing.append(f'{src}: sha256 differs from its pin')
            continue
        staged = plan.work / 'in' / f'{stem}.dwg'
        if not staged.exists() or sha256_of(staged) != r['sha256']:
            staged.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(src, staged)
        entries.append((stem, staged, r))
        rel = 'R2000'
        _ver, magic, _dv, acad, dxftag = RELEASES[rel]
        common = dict(section='s1', item=stem, source=f'in/{stem}.dwg', input_sha256=r['sha256'], input_name=Path(r['path']).name,
                      licence=r['licence'], hosting=hosting(r['licence']), script=f's1-{stem}.scr')
        plan.add(kind='dwg', release=rel, path=f's1/{stem}.{rel}.dwg', expect=magic, types='', flags='',
                 note='R13 re-save twin', **common)
        plan.add(kind='dxf', release=rel, path=f's1/{stem}.{dxftag}.dxf', expect=acad, types='', flags='',
                 note='R13 re-save twin', **common)
    todo = [e for e in entries if not plan.item_done([f's1/{e[0]}.R2000.dwg', f's1/{e[0]}.R2000.dxf'])]
    out = plan.out / 's1'
    out.mkdir(parents=True, exist_ok=True)
    for stem, staged, r in entries:
        scr = Scr(f'section 1: re-save {stem} ({r["licence"]})')
        scr.setup(True)
        scr.comment(OPEN_NOTE)
        scr.cmd('_.OPEN', scr_path(staged))
        save_block(scr, stem, ['R2000'], out)
        scr.teardown(True)
        scr.cmd('_.CLOSE')
        scr.write(plan.work / 'scripts' / f's1-{stem}.scr')
    if todo:
        scr = Scr(f'section 1: re-save {len(todo)} pinned R13 drawings to R2000 DWG and ASCII DXF')
        scr.setup(True)
        scr.comment(OPEN_NOTE)
        for k, (stem, staged, r) in enumerate(todo):
            scr.cmd('_.OPEN', scr_path(staged))
            save_block(scr, stem, ['R2000'], out)
            if k == len(todo) - 1:
                scr.teardown(True)
            scr.cmd('_.CLOSE')
        scr.write(plan.work / 'scripts' / 's1-all.scr')
    return missing, len(entries), len(todo)


def item_paths(item):
    paths = [f'masters/{item.name}.dwg']
    for rel in item.releases:
        _v, _m, _dv, _a, dxftag = RELEASES[rel]
        paths += [f'{item.section}/{item.name}.{rel}.dwg', f'{item.section}/{item.name}.{dxftag}.dxf']
    return paths


def items_plan(args, plan, sections, extra_s4):
    items = [i for i in CATALOG if i.section[1:] in sections and (i.lisp or not args.scripted_only)]
    items += extra_s4
    work, out = plan.work, plan.out
    for item in items:
        script = f'items/build-{item.name}.scr' if item.origin == 'lisp' else f'MANUAL-CHECKLIST.md#{item.name}'
        plan.add(section=item.section, item=item.name, kind='master', release='R2018',
                 path=f'masters/{item.name}.dwg', source='-', expect='AC1032', types='', flags='', input_sha256='-',
                 input_name='-', licence='Apache-2.0 (project-authored)', hosting='pin',
                 script=script, note=item.note)
        for rel in item.releases:
            ver, magic, dxfver, acad, dxftag = RELEASES[rel]
            common = dict(section=item.section, item=item.name, source=f'out/masters/{item.name}.dwg',
                          input_sha256='-', input_name='-',
                          licence='Apache-2.0 (project-authored)', hosting='pin',
                          script=f'items/save-{item.name}.scr', note=item.note)
            flags = 'text' if item.section == 's4' else ''
            plan.add(kind='dwg', release=rel, path=f'{item.section}/{item.name}.{rel}.dwg', expect=magic,
                     types='', flags=flags, **common)
            plan.add(kind='dxf', release=dxftag, path=f'{item.section}/{item.name}.{dxftag}.dxf', expect=acad,
                     types=applicable_types(item, rel), flags=flags, **common)
    lib = lisp_str(work / 'lisp' / 'dwgrs-lib.lsp')
    prelude = [f'(load {lib})', f'(setq *dwgrs-out* {lisp_str(out)})',
               f'(setq *dwgrs-pdf* {lisp_str(work / "in" / "dwgrs-two-page.pdf")})',
               f'(setq *dwgrs-char-mode* "{args.char_mode}")']
    builds = {}
    for sec in sections:
        builds[sec] = [i for i in items if i.section == f's{sec}' and i.origin == 'lisp'
                       and not plan.item_done([f'masters/{i.name}.dwg'])]
    for item in (i for i in items if i.origin == 'lisp'):
        scr = Scr(f'build {item.name}: {item.note}')
        scr.setup(False)
        build_one(scr, item, args, prelude, work)
        scr.teardown(False)
        scr.cmd('_.CLOSE')
        scr.write(work / 'scripts' / 'items' / f'build-{item.name}.scr')
    allbuild = []
    for sec in sections:
        todo = builds[sec]
        allbuild += todo
        if todo:
            write_build_group(work / 'scripts' / f'build-s{sec}.scr', f'build section {sec}', todo, args, prelude, work)
    if allbuild:
        write_build_group(work / 'scripts' / 'build-all.scr', 'build every AutoLISP item', allbuild, args, prelude, work)
    saves = {}
    for item in items:
        master = out / 'masters' / f'{item.name}.dwg'
        if not item.releases or not master.exists():
            continue
        paths = item_paths(item)[1:]
        scr = Scr(f'save {item.name} in {len(item.releases)} releases with ASCII DXF twins')
        scr.setup(True)
        scr.comment(SAVE_NOTE)
        scr.cmd('_.OPEN', scr_path(master))
        save_block(scr, item.name, item.releases, out / item.section)
        scr.teardown(True)
        scr.cmd('_.CLOSE')
        scr.write(work / 'scripts' / 'items' / f'save-{item.name}.scr')
        if not plan.item_done(paths):
            saves.setdefault(item.section, []).append(item)
    allsave = []
    for sec in sorted(saves):
        write_save_group(work / 'scripts' / f'save-{sec}.scr', f'save section {sec[1:]}', saves[sec], out)
        allsave += saves[sec]
    if allsave:
        write_save_group(work / 'scripts' / 'save-all.scr', 'save every master that exists', allsave, out)
    return items, len(allbuild), len(allsave)


def build_one(scr, item, args, prelude, work):
    scr.comment('VERIFY: the template name must resolve; pass --template with a full path if it does not')
    scr.cmd('_.NEW', scr_path(args.template))
    for line in prelude:
        scr.lisp(line)
    scr.lisp(f'(load {lisp_str(work / "lisp" / (item.lisp + ".lsp"))})')
    scr.lisp(f'(c:dwgrs-{item.name})')


def write_build_group(path, title, todo, args, prelude, work):
    scr = Scr(f'{title}: {", ".join(i.name for i in todo)}')
    scr.setup(False)
    for k, item in enumerate(todo):
        scr.comment(item.name)
        build_one(scr, item, args, prelude, work)
        if k == len(todo) - 1:
            scr.teardown(False)
        scr.cmd('_.CLOSE')
    scr.write(path)


def write_save_group(path, title, todo, out):
    scr = Scr(f'{title}: {", ".join(i.name for i in todo)}')
    scr.setup(True)
    scr.comment(SAVE_NOTE)
    for k, item in enumerate(todo):
        scr.comment(item.name)
        scr.cmd('_.OPEN', scr_path(out / 'masters' / f'{item.name}.dwg'))
        save_block(scr, item.name, item.releases, out / item.section)
        if k == len(todo) - 1:
            scr.teardown(True)
        scr.cmd('_.CLOSE')
    scr.write(path)


def write_start(args, work, out):
    scr = Scr('record the session: code pages, build, and which character mode AutoLISP strings need')
    lispdir = lisp_str(work / 'lisp')
    scr.comment('VERIFY: TRUSTEDPATHS exists, is writable and takes a semicolon list; otherwise set it by hand')
    scr.lisp(f'(if (and (getvar "TRUSTEDPATHS") (not (wcmatch (getvar "TRUSTEDPATHS") (strcat "*" {lispdir} "*")))) '
             f'(setvar "TRUSTEDPATHS" (strcat (getvar "TRUSTEDPATHS") ";" {lispdir})))')
    scr.cmd('_.SETVAR', 'FILEDIA', '0')
    scr.comment('VERIFY: saving the blank host drawing keeps NEW and OPEN from replacing it mid-run')
    scr.cmd('_.SETVAR', 'EXPERT', '2')
    scr.cmd('_.SAVEAS', '2018', scr_path(out / 'session.dwg'))
    scr.cmd('_.SETVAR', 'EXPERT', '0')
    scr.lisp(f'(load {lisp_str(work / "lisp" / "dwgrs-lib.lsp")})')
    scr.lisp(f'(setq *dwgrs-out* {lisp_str(out)})')
    scr.lisp('(dwgrs:info)')
    scr.lisp('(c:dwgrs-chartest)')
    scr.cmd('_.SETVAR', 'FILEDIA', '1')
    scr.write(work / 'scripts' / '00-start.scr')


def write_manifest(plan):
    path = plan.work / 'manifest.tsv'
    with open(path, 'w', encoding='utf-8', newline='') as f:
        w = csv.writer(f, delimiter='\t', lineterminator='\n', quoting=csv.QUOTE_NONE, escapechar='\\')
        w.writerow(COLUMNS)
        for row in plan.rows:
            w.writerow([row[c] for c in COLUMNS])
    return path


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('--corpus')
    ap.add_argument('--pins')
    ap.add_argument('--work', default=DEFAULT_WORK)
    ap.add_argument('--template')
    ap.add_argument('--sections')
    ap.add_argument('--sources')
    ap.add_argument('--char-mode', choices=('chr', 'escape'))
    ap.add_argument('--scripted-only', action='store_true', default=None)
    ap.add_argument('--keyword-prefix', choices=('', '_'))
    ap.add_argument('--resume', action='store_true')
    ap.add_argument('--allow-missing', action='store_true')
    args = ap.parse_args()
    work = Path(args.work).expanduser().resolve()
    saved = {}
    if args.resume and (work / 'session.json').is_file():
        saved = json.loads((work / 'session.json').read_text())
    fallback = {'corpus': DEFAULT_CORPUS, 'pins': str(DEFAULT_PINS), 'template': 'acadiso.dwt', 'sections': '1,2,3,4',
                'sources': '', 'char_mode': 'chr', 'scripted_only': False, 'keyword_prefix': ''}
    for key, default in fallback.items():
        if getattr(args, key) is None:
            setattr(args, key, saved.get(key, default))
    global KEYWORD_PREFIX
    KEYWORD_PREFIX = args.keyword_prefix
    sections = [s for s in args.sections.split(',') if s]
    if any(s not in ('1', '2', '3', '4') for s in sections):
        die('--sections takes 1, 2, 3 and 4')
    out = work / 'out'
    check_path(work)
    check_path(args.template)
    pins_path = Path(args.pins).expanduser()
    if not pins_path.is_file():
        die(f'{pins_path}: no such pins file')
    pins = read_pins(pins_path)
    for d in ('in', 'lisp', 'scripts/items', 'out/masters', 'out/s1', 'out/s2', 'out/s3', 'out/s4'):
        (work / d).mkdir(parents=True, exist_ok=True)
    for stale in list((work / 'scripts').glob('*.scr')) + list((work / 'scripts' / 'items').glob('*.scr')):
        stale.unlink()
    for lsp in sorted((HERE / 'lisp').glob('*.lsp')):
        shutil.copyfile(lsp, work / 'lisp' / lsp.name)
    (work / 'in' / 'dwgrs-two-page.pdf').write_bytes(two_page_pdf())
    (work / 'in' / 'dwgrs-map.png').write_bytes(map_png())
    plan = Plan(work, out, args.resume)
    missing, staged, s1_todo = [], 0, 0
    if '1' in sections:
        missing, staged, s1_todo = s1_plan(args, plan, pins)
    extra = []
    if '4' in sections:
        for m in sorted((out / 'masters').glob('s4-*.dwg')):
            if not m.name.endswith('.FAILED.dwg') and m.stem not in BY_NAME:
                extra.append(Item(m.stem, None, TEXTREL, [], 'extra code-page drawing found in masters'))
    items, n_build, n_save = items_plan(args, plan, [s for s in sections if s != '1'], extra)
    write_start(args, work, out)
    manifest = write_manifest(plan)
    print(f'work directory   {work}')
    print(f'manifest         {manifest} ({len(plan.rows)} expected files)')
    if '1' in sections:
        print(f'section 1        {staged} R13 drawings staged, {s1_todo} to run')
    print(f'sections 2 to 4  {n_build} AutoLISP items to build, {n_save} masters ready to save out')
    (work / 'session.json').write_text(json.dumps({k: getattr(args, k) for k in fallback}, indent=1) + '\n')
    manual = [i.name for i in items if i.origin == 'manual' and not (out / 'masters' / f'{i.name}.dwg').exists()]
    if manual:
        print(f'manual masters   {len(manual)} not saved yet (MANUAL-CHECKLIST.md): {", ".join(manual)}')
    for m in missing:
        print(f'missing input    {m}', file=sys.stderr)
    print('scripts')
    groups = [p for p in sorted((work / 'scripts').glob('*.scr')) if p.name == 's1-all.scr' or not p.name.startswith('s1-')]
    for p in groups:
        print(f'  {p}')
    per = len([p for p in (work / 'scripts').glob('s1-*.scr') if p.name != 's1-all.scr']) + len(list((work / 'scripts' / 'items').glob('*.scr')))
    print(f'  and {per} single-piece scripts under {work / "scripts"} and {work / "scripts" / "items"}')
    print()
    print('first three commands in AutoCAD, at the Command prompt:')
    for name in ('00-start.scr', 's1-all.scr', 'build-all.scr'):
        if (work / 'scripts' / name).exists():
            print(f'  -SCRIPT   {work / "scripts" / name}')
    return 0 if args.allow_missing or not missing else 2


if __name__ == '__main__':
    sys.exit(main())
