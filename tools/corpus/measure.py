#!/usr/bin/env python3
"""Measure the reader on the fetched public corpus and ratchet it (#97, #98).

Three built examples run over every DWG pinned in tools/corpus/pins.tsv:

    coverage_report --tsv      decoded / skipped / errored per file, per type
    probe_class_census         classes the walk reaches fewer of than declared
    probe_reference_closure    inter-record bytes; decoded, unanswered and
                               hard handle references

Subcommands:

    run       --corpus DIR --bin DIR --out results.json [--logs DIR]
    check     --results results.json [--baseline FILE] [--summary FILE] [--proposed FILE]
    baseline  --results results.json [--baseline FILE]
    diff      --base base.json --head head.json [--summary FILE]
    report    --results results.json [--summary FILE]

--corpus is fetch.py's --dest; --bin holds the three built examples
(target/release/examples). `check` compares every file's row with the
committed baseline (tools/corpus/baseline.tsv) and fails on any difference:
a regression is a defect to fix, and an improvement is recorded by running
`baseline` and committing the result in the same pull request. `diff` writes
a report of how one build's results differ from another's and never fails;
`report` writes one build's tables alone.

Usage, from a checkout:

    python3 -I tools/corpus/fetch.py --dest ../dwg-corpus
    cargo build --release --example coverage_report \\
        --example probe_class_census --example probe_reference_closure
    python3 -I tools/corpus/measure.py run --corpus ../dwg-corpus \\
        --bin target/release/examples --out corpus-results.json
    python3 -I tools/corpus/measure.py check --results corpus-results.json
"""

import argparse
import json
import re
import subprocess
import sys
import tempfile
from collections import defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent
PINS = HERE / 'pins.tsv'
BASELINE = HERE / 'baseline.tsv'
RELEASES = {
    'AC1012': 'R13', 'AC1014': 'R14', 'AC1015': 'R2000', 'AC1018': 'R2004',
    'AC1021': 'R2007', 'AC1024': 'R2010', 'AC1027': 'R2013', 'AC1032': 'R2018',
}
COLUMNS = ['file', 'source', 'release', 'decoded', 'skipped', 'errored', 'walk_skips',
           'handle_mismatches', 'census', 'between', 'refs', 'unanswered', 'hard', 'no_boundary']
HIGHER_IS_BETTER = {'decoded', 'refs'}
CENSUS_ROW = re.compile(r'^(\S+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s+([+-]\d+)\s*(ALLOWED)?\s*$')
TILING = re.compile(r'^stream tiling\s*:.*?(\d+) between records')
CLOSURE = re.compile(r'^reference closure\s*:\s*(\d+) references decoded, (\d+) unanswered target\(s\), '
                     r'(\d+) of them hard \((\d+) record')


def read_tsv(path):
    rows, header = [], None
    for line in Path(path).read_text(encoding='utf-8').splitlines():
        if not line or line.startswith('#'):
            continue
        fields = line.split('\t')
        if header is None:
            header = fields
        else:
            rows.append(dict(zip(header, fields)))
    return rows


def pinned_drawings():
    return [row for row in read_tsv(PINS) if row['kind'] == 'dwg']


def execute(argv, corpus, log):
    proc = subprocess.run(argv, capture_output=True, text=True)
    text = (proc.stdout + proc.stderr).replace(str(corpus), '<corpus>')
    if log:
        log.write_text(text, encoding='utf-8')
    return proc.returncode, text


def census(text):
    rows, done = [], False
    for line in text.splitlines():
        match = CENSUS_ROW.match(line)
        if match and int(match.group(6)) < 0:
            name, declared, walked, fixed = match.group(1), *map(int, match.group(3, 4, 5))
            rows.append([name, declared, walked + fixed, bool(match.group(7))])
        if line.startswith('RESULT:') or 'nothing to census' in line or 'no handle-map-driven walk' in line:
            done = True
    return sorted(rows), done


def closure(text):
    found = {}
    for line in text.splitlines():
        if match := TILING.match(line):
            found['between'] = int(match.group(1))
        if match := CLOSURE.match(line):
            found.update(zip(('refs', 'unanswered', 'hard', 'no_boundary'), map(int, match.groups())))
    return found


def run(args):
    corpus, bins = args.corpus.resolve(), args.bin.resolve()
    logs = args.logs
    if logs:
        logs.mkdir(parents=True, exist_ok=True)
    files = {}
    pins = pinned_drawings()
    for source in sorted({row['source'] for row in pins}):
        with tempfile.TemporaryDirectory() as tmp:
            tsv = Path(tmp) / 'coverage.tsv'
            code, _ = execute([str(bins / 'coverage_report'), str(corpus / source), '--tsv', str(tsv)], corpus,
                              logs / f'coverage-{source}.txt' if logs else None)
            lines = tsv.read_text(encoding='utf-8').splitlines() if tsv.exists() else []
        for line in lines:
            kind, name, *rest = line.split('\t')
            entry = files.setdefault(name, {'errors': {}, 'unhandled': {}, 'failed': []})
            if kind == 'file':
                entry['magic'] = rest[0]
                for key, value in zip(('decoded', 'skipped', 'errored', 'walk_skips', 'handle_mismatches'), rest[1:]):
                    entry[key] = int(value)
            elif kind in ('error', 'unhandled'):
                entry['errors' if kind == 'error' else 'unhandled'][rest[0]] = int(rest[1])
            elif kind == 'failed':
                entry['failed'].append(f'coverage_report {rest[0]}: {rest[1]}')
        if code != 0:
            print(f'::error::coverage_report exited {code} on {source}', flush=True)

    results = {}
    for row in pins:
        name = Path(row['path']).name
        entry = files.get(name, {'errors': {}, 'unhandled': {}, 'failed': []})
        entry.update(source=row['source'], release=row['release'], tests=row['tests'] == 'yes')
        if 'decoded' not in entry and not entry['failed']:
            entry['failed'].append('coverage_report did not measure it')
        if entry.get('magic', row['release']) != row['release']:
            entry['failed'].append(f'coverage_report reads {entry["magic"]}, pinned {row["release"]}')
        path = corpus / row['source'] / name
        code, text = execute([str(bins / 'probe_class_census'), str(path)], corpus,
                             logs / f'{name}.census.txt' if logs else None)
        entry['census'], done = census(text)
        entry['census_exit'] = code
        if not done:
            entry['failed'].append(f'probe_class_census exited {code} without a result')
        code, text = execute([str(bins / 'probe_reference_closure'), str(path)], corpus,
                             logs / f'{name}.closure.txt' if logs else None)
        found = closure(text)
        entry.update(found, closure_exit=code)
        if 'RESULT:' not in text:
            entry['failed'].append(f'probe_reference_closure exited {code} without a result')
        results[name] = entry
    args.out.write_text(json.dumps({'files': results}, indent=1, sort_keys=True) + '\n', encoding='utf-8')
    failed = [f'{name}: {msg}' for name, entry in results.items() for msg in entry['failed']]
    for line in failed:
        print(f'::error::{line}', flush=True)
    print(f'measured {len(results)} pinned drawings; {len(failed)} measurement failure(s)', flush=True)
    return 1 if failed else 0


def census_cell(entry):
    return ';'.join(f'{name} {declared}>{reached}' for name, declared, reached, _ in entry.get('census', [])) or '-'


def baseline_rows(results):
    rows = []
    for name, entry in sorted(results['files'].items()):
        row = {'file': name, 'source': entry['source'], 'release': entry['release'], 'census': census_cell(entry)}
        for key in COLUMNS[3:]:
            if key != 'census':
                row[key] = str(entry.get(key, '?'))
        rows.append(row)
    return rows


def write_baseline(rows, path):
    lines = [
        '# Per-file corpus ratchet (#97): the Corpus job in .github/workflows/ci.yml',
        '# fails when a measurement differs from its row here, in either direction.',
        '# Regenerate with tools/corpus/measure.py baseline and commit it in the PR',
        '# that moves it. census: class declared>reached for every class the walk',
        '# reaches fewer of than AcDb:Classes declares; between: bytes between walked',
        '# records; refs/unanswered/hard: handle references probe_reference_closure',
        '# decodes, leaves unresolved, and of those hard; no_boundary: records with no',
        '# handle-stream boundary, whose references go unchecked.',
        '\t'.join(COLUMNS),
    ]
    lines += ['\t'.join(row[key] for key in COLUMNS) for row in rows]
    Path(path).write_text('\n'.join(lines) + '\n', encoding='utf-8')


def ratchet(old, new):
    findings = []
    for key in COLUMNS[3:]:
        if old[key] == new[key]:
            continue
        if key == 'census':
            before = {c.rsplit(' ', 1)[0]: int(c.rsplit('>', 1)[1]) for c in old[key].split(';') if c != '-'}
            after = {c.rsplit(' ', 1)[0]: int(c.rsplit('>', 1)[1]) for c in new[key].split(';') if c != '-'}
            worse = any(k not in before or after[k] < before[k] for k in after)
        elif not (old[key].isdigit() and new[key].isdigit()):
            worse = True
        else:
            delta = int(new[key]) - int(old[key])
            worse = delta < 0 if key in HIGHER_IS_BETTER else delta > 0
        findings.append((worse, f'{new["file"]}: {key} {old[key]} -> {new[key]}'))
    return findings


def ratio(decoded, skipped, errored):
    total = decoded + skipped + errored
    return f'{100.0 * decoded / total:.1f} %' if total else 'n/a'


def totals(entries):
    sums = [sum(e.get(k, 0) for e in entries) for k in ('decoded', 'skipped', 'errored')]
    return [len(entries), *sums, ratio(*sums)]


def table(header, rows):
    out = ['| ' + ' | '.join(header) + ' |', '|' + '|'.join('---' for _ in header) + '|']
    out += ['| ' + ' | '.join(str(c) for c in row) + ' |' for row in rows]
    return out


def by_type(files, key):
    releases = sorted({e['release'] for e in files.values()})
    counts = defaultdict(lambda: defaultdict(int))
    for entry in files.values():
        for label, n in entry.get(key, {}).items():
            counts[label][entry['release']] += n
    order = sorted(counts, key=lambda label: (-sum(counts[label].values()), label))
    rows = [[label, *[counts[label].get(r, '') for r in releases], sum(counts[label].values())] for label in order]
    return table(['Type', *[RELEASES.get(r, r) for r in releases], 'Total'], rows)


def report(results):
    files = results['files']
    out = ['## Corpus measurement', '',
           f'{len(files)} public drawings pinned by sha256 in `tools/corpus/pins.tsv` '
           f'({", ".join(sorted({e["source"] for e in files.values()}))}).', '', '### By release', '']
    groups = defaultdict(list)
    for entry in files.values():
        groups[entry['release']].append(entry)
    rows = [[f'{RELEASES.get(r, r)} ({r})', *totals(groups[r])] for r in sorted(groups)]
    rows.append(['**All**', *totals(list(files.values()))])
    legacy = [e for e in files.values() if e['tests']]
    rows.append(['Legacy 19-file corpus (`tests` column)', *totals(legacy)])
    out += table(['Release', 'Files', 'Decoded', 'Skipped', 'Errored', 'Decoded %'], rows)
    out += ['', '### By source', '']
    sources = defaultdict(list)
    for entry in files.values():
        sources[entry['source']].append(entry)
    out += table(['Source', 'Files', 'Decoded', 'Skipped', 'Errored', 'Decoded %'],
                 [[s, *totals(sources[s])] for s in sorted(sources)])
    out += ['', '### Errored records by type and release', '']
    out += by_type(files, 'errors') if any(e['errors'] for e in files.values()) else ['No errored record.']
    out += ['', '### Skipped (no decoder) records by type and release', '']
    out += by_type(files, 'unhandled')
    out += ['', '### Walk checks', '']
    short = [[n, census_cell(e)] for n, e in sorted(files.items()) if e.get('census')]
    out.append(f'Class census: {len(files) - len(short)} of {len(files)} drawings reach every class '
               'AcDb:Classes declares. Shortfalls (declared>reached), each pinned in the baseline:')
    out += [''] + table(['File', 'Shortfall'], short)
    vacuous = sorted(n for n, e in files.items() if e.get('refs') == 0 and e.get('no_boundary'))
    open_walk = sorted(n for n, e in files.items() if e.get('between') or e.get('hard'))
    out += ['', f'Reference closure: {len(files) - len(set(vacuous) | set(open_walk))} drawings closed '
            '(no bytes between records, no unanswered hard reference); '
            f'{len(open_walk)} open{": " + ", ".join(open_walk) if open_walk else ""}; '
            f'NOT MEASURED on {len(vacuous)} whose records expose no handle-stream boundary '
            f'({", ".join(vacuous) if vacuous else "none"}).', '', '### Per file', '']
    out += table(['File', 'Release', 'Decoded', 'Skipped', 'Errored', 'Decoded %', 'Refs (unanswered)'],
                 [[n, RELEASES.get(e['release'], e['release']), e.get('decoded', '?'), e.get('skipped', '?'),
                   e.get('errored', '?'), ratio(e.get('decoded', 0), e.get('skipped', 0), e.get('errored', 0)),
                   f'{e.get("refs", "?")} ({e.get("unanswered", "?")})'] for n, e in sorted(files.items())])
    return out


def check(args):
    results = json.loads(args.results.read_text(encoding='utf-8'))
    new = {row['file']: row for row in baseline_rows(results)}
    old = {row['file']: row for row in read_tsv(args.baseline)} if args.baseline.exists() else {}
    if args.proposed:
        write_baseline(list(new.values()), args.proposed)
    findings = []
    for name in sorted(old.keys() - new.keys()):
        findings.append((True, f'{name}: in the baseline but not measured'))
    for name in sorted(new.keys() - old.keys()):
        findings.append((False, f'{name}: measured but not in the baseline'))
    for name in sorted(old.keys() & new.keys()):
        findings += ratchet(old[name], new[name])
    failed = [f'{n}: {m}' for n, e in sorted(results['files'].items()) for m in e['failed']]
    lines = report(results) + ['', '### Ratchet against `tools/corpus/baseline.tsv`', '']
    if failed:
        lines += [f'- MEASUREMENT FAILED {f}' for f in failed]
    if findings:
        lines += [f'- {"REGRESSION" if worse else "IMPROVED OR NEW (commit the new baseline)"}: {text}'
                  for worse, text in findings]
        lines += ['', 'Every row must match. Record an improvement with '
                  '`python3 -I tools/corpus/measure.py baseline --results <results.json>` '
                  '(the job uploads the proposed `baseline.tsv` as an artifact) and commit it in this PR.']
    if not failed and not findings:
        lines.append(f'Every one of {len(new)} rows matches.')
    text = '\n'.join(lines) + '\n'
    if args.summary:
        with open(args.summary, 'a', encoding='utf-8') as fh:
            fh.write(text)
    print(text)
    for worse, finding in findings:
        print(f'::error file=tools/corpus/baseline.tsv::{"regression" if worse else "improvement not recorded"}: '
              f'{finding}', flush=True)
    return 1 if findings or failed else 0


def baseline(args):
    results = json.loads(args.results.read_text(encoding='utf-8'))
    write_baseline(baseline_rows(results), args.baseline)
    print(f'wrote {args.baseline}')
    return 0


def report_only(args):
    results = json.loads(args.results.read_text(encoding='utf-8'))
    text = '\n'.join(report(results)) + '\n'
    if args.summary:
        with open(args.summary, 'a', encoding='utf-8') as fh:
            fh.write(text)
    print(text)
    return 0


def diff(args):
    base = json.loads(args.base.read_text(encoding='utf-8'))['files']
    head = json.loads(args.head.read_text(encoding='utf-8'))['files']
    lines = ['## Measure: head against base', '']
    releases = sorted({e['release'] for e in list(base.values()) + list(head.values())})
    rows = []
    for r in releases + ['All']:
        b = [e for e in base.values() if r == 'All' or e['release'] == r]
        h = [e for e in head.values() if r == 'All' or e['release'] == r]
        tb, th = totals(b), totals(h)
        rows.append([RELEASES.get(r, r), *[f'{tb[i]} -> {th[i]} ({th[i] - tb[i]:+d})' for i in (1, 2, 3)],
                     f'{tb[4]} -> {th[4]}'])
    lines += table(['Release', 'Decoded', 'Skipped', 'Errored', 'Decoded %'], rows)
    old = {row['file']: row for row in baseline_rows({'files': base})}
    new = {row['file']: row for row in baseline_rows({'files': head})}
    changes = [text for name in sorted(old.keys() & new.keys()) for _, text in ratchet(old[name], new[name])]
    changes += [f'{n}: only in base' for n in sorted(old.keys() - new.keys())]
    changes += [f'{n}: only in head' for n in sorted(new.keys() - old.keys())]
    lines += ['', '### Per-file changes', ''] + ([f'- {c}' for c in changes] or ['None.'])
    type_rows = []
    for key in ('errors', 'unhandled'):
        b, h = defaultdict(int), defaultdict(int)
        for e in base.values():
            for label, n in e.get(key, {}).items():
                b[label] += n
        for e in head.values():
            for label, n in e.get(key, {}).items():
                h[label] += n
        type_rows += [[key, label, b[label], h[label], f'{h[label] - b[label]:+d}']
                      for label in sorted(b.keys() | h.keys()) if b[label] != h[label]]
    lines += ['', '### Per-type changes', ''] + (table(['Kind', 'Type', 'Base', 'Head', 'Delta'], type_rows)
                                                 if type_rows else ['None.'])
    text = '\n'.join(lines) + '\n'
    if args.summary:
        with open(args.summary, 'a', encoding='utf-8') as fh:
            fh.write(text)
    print(text)
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    sub = parser.add_subparsers(dest='command', required=True)
    p = sub.add_parser('run')
    p.add_argument('--corpus', required=True, type=Path)
    p.add_argument('--bin', required=True, type=Path)
    p.add_argument('--out', required=True, type=Path)
    p.add_argument('--logs', type=Path)
    p = sub.add_parser('check')
    p.add_argument('--results', required=True, type=Path)
    p.add_argument('--baseline', type=Path, default=BASELINE)
    p.add_argument('--summary', type=Path)
    p.add_argument('--proposed', type=Path)
    p = sub.add_parser('baseline')
    p.add_argument('--results', required=True, type=Path)
    p.add_argument('--baseline', type=Path, default=BASELINE)
    p = sub.add_parser('diff')
    p.add_argument('--base', required=True, type=Path)
    p.add_argument('--head', required=True, type=Path)
    p.add_argument('--summary', type=Path)
    p = sub.add_parser('report')
    p.add_argument('--results', required=True, type=Path)
    p.add_argument('--summary', type=Path)
    args = parser.parse_args()
    return {'run': run, 'check': check, 'baseline': baseline, 'diff': diff, 'report': report_only}[args.command](args)


if __name__ == '__main__':
    sys.exit(main())
