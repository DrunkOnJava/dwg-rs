"""Cell-level 3-way merge of a conflicted TSV keyed by its first column.

Run inside a repository with a merge in progress: reads stages 1 (base), 2 (ours) and 3 (theirs)
of PATH and writes the merged file to PATH. A cell changed on one side takes that side; a cell
changed differently on both sides, a duplicate key, or a row deleted on one side and edited on the
other is a real conflict: nothing is written and the exit status is 1.
"""
import re
import subprocess
import sys


def stage(n, path):
    return subprocess.run(['git', 'show', f':{n}:{path}'], capture_output=True, text=True, check=True).stdout


def parse(text):
    comments, rows, order = [], {}, []
    skip = None
    for line in text.splitlines():
        if line.startswith('#') or not line.strip():
            comments.append(line)
            continue
        cells = line.split('\t')
        if skip is None:
            skip = cells.index('expect') if 'expect' in cells else -1
        key = '\t'.join(c for i, c in enumerate(cells) if i != skip) if skip >= 0 else cells[0]
        if key in rows:
            raise SystemExit(f'duplicate key {key!r}')
        rows[key] = line.split('\t')
        order.append(key)
    return comments, rows, order


SPEC = re.compile(r"\s*(The spec's own example.*)$", re.S)
ORACLE = re.compile(r"\s*(DXF oracle \(examples/dxf_oracle\):.*)$", re.S)


def segments(cell):
    """Split a note into (free text, oracle segment, spec-example segment); the last two are tool-written."""
    cell = cell or ''
    s = SPEC.search(cell)
    rest, spec = (cell[:s.start()], s.group(1)) if s else (cell, '')
    o = ORACLE.search(rest)
    free, oracle = (rest[:o.start()], o.group(1)) if o else (rest, '')
    return free.strip(), oracle.strip(), spec.strip()


def pick(b, o, t, what, prefer=False):
    if o == t or o == b:
        return t
    if t == b:
        return o
    if any(x and (SPEC.search(x) or ORACLE.search(x)) for x in (b, o, t)):
        (bf, bo, bs), (of, oo, os_), (tf, to, ts) = segments(b), segments(o), segments(t)
        free = pick(bf, of, tf, what + ' (free text)', prefer)
        oracle = oo if oo != bo and oo else to
        spec = os_ if os_ != bs and os_ else ts
        return ' '.join(x for x in (free, oracle, spec) if x)
    # One side appended a sentence to the base cell, the other rewrote it: keep the rewrite and the appended sentence.
    if b and t.startswith(b) and t[len(b):].strip():
        suffix = t[len(b):]
        return o if suffix.strip() in o else o.rstrip() + ' ' + suffix.strip()
    if b and o.startswith(b) and o[len(b):].strip():
        suffix = o[len(b):]
        return t if suffix.strip() in t else t.rstrip() + ' ' + suffix.strip()
    if prefer:
        print(f'PREFERRED THE PR: {what}: kept {o!r}; dropped main\'s {t!r} (base {b!r})', file=sys.stderr)
        return o
    raise SystemExit(f'conflict: {what}: ours {o!r} vs theirs {t!r} (base {b!r})')


def main(path):
    bc, base, _ = parse(stage(1, path))
    oc, ours, oorder = parse(stage(2, path))
    tc, theirs, torder = parse(stage(3, path))
    comments = pick(bc, oc, tc, 'comment lines')
    merged = {}
    for key in dict.fromkeys(torder + oorder):
        b, o, t = base.get(key), ours.get(key), theirs.get(key)
        if o is None and t is None:
            continue
        if o is None or t is None:
            present = o if t is None else t
            if b is None:
                merged[key] = present
            elif present == b:
                continue
            else:
                raise SystemExit(f'conflict: row {key!r} deleted on one side, edited on the other')
            continue
        width = max(len(x) for x in (b or [], o, t))
        pad = lambda r: (r or []) + [''] * (width - len(r or []))
        bb, oo, tt = pad(b), pad(o), pad(t)
        row = [pick(bb[i] if b else None, oo[i], tt[i], f'{key} column {i + 1}', i + 1 in PREFER) for i in range(width)]
        while row and row[-1] == '' and len(row) > max(len(o), len(t)):
            row.pop()
        merged[key] = row
    order = list(torder)
    for i, key in enumerate(oorder):
        if key in merged and key not in order:
            prev = next((k for k in reversed(oorder[:i]) if k in order), None)
            order.insert(order.index(prev) + 1 if prev else 0, key)
    lines = comments + ['\t'.join(merged[k]) for k in order if k in merged]
    with open(path, 'w', encoding='utf-8') as fh:
        fh.write('\n'.join(lines) + '\n')


PREFER = set()

if __name__ == '__main__':
    if len(sys.argv) > 3 and sys.argv[2] == '--prefer-ours':
        PREFER = {int(c) for c in sys.argv[3].split(',')}
    main(sys.argv[1])
