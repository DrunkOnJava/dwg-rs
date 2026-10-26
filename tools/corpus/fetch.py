#!/usr/bin/env python3
"""Fetch the public DWG corpus by pinned hash (#95).

Every row of tools/corpus/pins.tsv is downloaded from its repository at its
pinned commit and checked against its pinned size and sha256 before it is
kept. Nothing fetched is ever committed to this repository.

Layout under --dest (stable; the DXF value oracle reads it too):

    <dest>/nextgis/arc_2000.dwg            nextgis/dwg_samples, AutoCAD 2016
    <dest>/nextgis/arc_2000.dxf            AutoCAD's DXF export of that drawing
    <dest>/acadsharp/sample_AC1032.dwg     DomCR/ACadSharp samples/, AutoCAD 2025
    <dest>/acadsharp/sample_AC1032_ascii.dxf

A DWG's DXF twin is its `twin` column and sits in the same directory.
--legacy-samples DIR also copies the 19 rows marked `tests` into DIR, flat:
the layout tests/samples.rs, class_census.rs, block_names.rs,
corpus_roundtrip.rs and r2013_entity_values.rs read at ../../samples.

Files already present with the pinned hash are not downloaded again, so a
restored cache needs no network. Each file comes from raw.githubusercontent.com
at its pinned commit, or failing that from the git blobs API by its pinned
blob id; each route is retried before the other is tried, and GITHUB_TOKEN
(or GH_TOKEN) is sent when set. The contents API is deliberately not a route:
its raw media type re-encodes non-UTF-8 text, which turned the 0xD8 of an
ANSI_1252 DXF into two UTF-8 bytes.

pins.tsv is the one list of corpus files. --cross-check takes other JSON
manifests that name files of a pinned commit (tools/streaming/*-corpus.json)
and fails unless each file they name is pinned here with the same size and
sha256, so a second copy of a pin cannot drift.

Exit status: 0 every pinned file is present and verified; 1 a file does not
match its pin, its pinned commit no longer serves it, or a cross-checked
manifest disagrees (fail closed); 69 the host was unreachable after every
retry (the caller reports NOT MEASURED).

Usage:

    python3 -I tools/corpus/fetch.py --dest DIR [--legacy-samples DIR] [--cross-check FILE...]
"""

import argparse
import base64
import hashlib
import json
import os
import shutil
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

PINS = Path(__file__).resolve().with_name('pins.tsv')
UNREACHABLE = 69
ATTEMPTS = 3


def read_pins(path=PINS):
    rows = []
    header = None
    for line in path.read_text(encoding='utf-8').splitlines():
        if not line or line.startswith('#'):
            continue
        fields = line.split('\t')
        if header is None:
            header = fields
            continue
        rows.append(dict(zip(header, fields)))
    return rows


def local_name(row):
    return Path(row['source']) / Path(row['path']).name


def verify(path, row):
    if not path.is_file():
        return 'missing'
    data = path.read_bytes()
    if len(data) != int(row['bytes']):
        return f'{len(data)} bytes, pinned {row["bytes"]}'
    digest = hashlib.sha256(data).hexdigest()
    if digest != row['sha256']:
        return f'sha256 {digest}, pinned {row["sha256"]}'
    return None


def routes(row):
    token = os.environ.get('GITHUB_TOKEN') or os.environ.get('GH_TOKEN')
    auth = {'Authorization': f'Bearer {token}'} if token else {}
    raw = (f'https://raw.githubusercontent.com/{row["repo"]}/{row["commit"]}/{row["path"]}', auth, False)
    blob = (
        f'https://api.github.com/repos/{row["repo"]}/git/blobs/{row["blob"]}',
        {'Accept': 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28', **auth},
        True,
    )
    return [raw, blob]


class Gone(Exception):
    pass


class Unreachable(Exception):
    pass


def download(row):
    failures = []
    gone = 0
    for url, headers, encoded in routes(row):
        for attempt in range(ATTEMPTS):
            request = urllib.request.Request(url, headers={'User-Agent': 'dwg-rs-corpus-fetch', **headers})
            try:
                with urllib.request.urlopen(request, timeout=60) as response:
                    body = response.read()
                return base64.b64decode(json.loads(body)['content']) if encoded else body
            except urllib.error.HTTPError as e:
                if e.code == 404:
                    gone += 1
                    failures.append(f'{url}: 404')
                    break
                failures.append(f'{url}: HTTP {e.code}')
            except (urllib.error.URLError, TimeoutError, ConnectionError, OSError, ValueError, KeyError) as e:
                failures.append(f'{url}: {e!r}')
            if attempt + 1 < ATTEMPTS:
                time.sleep(2 ** (attempt + 1))
    if gone == len(routes(row)):
        raise Gone('; '.join(failures))
    raise Unreachable('; '.join(failures[-2:]))


def disagreements(rows, manifests):
    pinned = {(row['repo'], row['commit'], row['path']): row for row in rows}
    found = []
    for manifest in manifests:
        data = json.loads(manifest.read_text(encoding='utf-8'))
        repo = data['source'].removeprefix('https://github.com/')
        for item in data['files']:
            row = pinned.get((repo, data['commit'], item['name']))
            if row is None:
                found.append(f'{manifest}: {repo}@{data["commit"][:12]}:{item["name"]} is not in {PINS.name}')
            elif (str(item['bytes']), item['sha256']) != (row['bytes'], row['sha256']):
                found.append(f'{manifest}: {item["name"]} pins {item["bytes"]} bytes, sha256 {item["sha256"]}; '
                             f'{PINS.name} pins {row["bytes"]}, {row["sha256"]}')
    return found


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--dest', required=True, type=Path)
    parser.add_argument('--legacy-samples', type=Path)
    parser.add_argument('--cross-check', type=Path, nargs='*', default=[])
    args = parser.parse_args()

    rows = read_pins()
    conflicts = disagreements(rows, args.cross_check)
    for line in conflicts:
        print(f'::error::{line}', flush=True)
    if conflicts:
        return 1
    if args.cross_check:
        print(f'{len(args.cross_check)} other manifest(s) agree with {PINS.name}', flush=True)
    mismatched, unreachable, fetched = [], [], 0
    for row in rows:
        target = args.dest / local_name(row)
        problem = verify(target, row)
        if problem is None:
            continue
        if problem != 'missing':
            print(f'::warning::{local_name(row)}: cached copy is {problem}; fetching it again', flush=True)
        target.parent.mkdir(parents=True, exist_ok=True)
        try:
            data = download(row)
        except Gone as e:
            mismatched.append(f'{row["repo"]}@{row["commit"][:12]} no longer serves {row["path"]} ({e})')
            continue
        except Unreachable as e:
            unreachable.append(f'{row["path"]}: {e}')
            continue
        partial = target.with_name(target.name + '.partial')
        partial.write_bytes(data)
        problem = verify(partial, row)
        if problem is not None:
            partial.unlink()
            mismatched.append(f'{row["repo"]}@{row["commit"][:12]}:{row["path"]} is {problem}')
            continue
        partial.replace(target)
        fetched += 1

    for line in mismatched:
        print(f'::error::{line}', flush=True)
    for line in unreachable:
        print(f'::warning::unreachable: {line}', flush=True)
    present = len(rows) - len(mismatched) - len(unreachable)
    print(f'{present} of {len(rows)} pinned files verified ({fetched} downloaded) under {args.dest}', flush=True)
    if mismatched:
        return 1
    if unreachable:
        return UNREACHABLE

    if args.legacy_samples:
        args.legacy_samples.mkdir(parents=True, exist_ok=True)
        legacy = [row for row in rows if row['tests'] == 'yes']
        for row in legacy:
            shutil.copyfile(args.dest / local_name(row), args.legacy_samples / Path(row['path']).name)
        names = {Path(row['path']).name for row in legacy}
        extra = sorted(p.name for p in args.legacy_samples.glob('*.dwg') if p.name not in names)
        if extra:
            print(f'::warning::{args.legacy_samples} also holds unpinned drawings the corpus tests will read: {", ".join(extra)}')
        print(f'{len(legacy)} legacy test files copied to {args.legacy_samples}', flush=True)
    return 0


if __name__ == '__main__':
    sys.exit(main())
