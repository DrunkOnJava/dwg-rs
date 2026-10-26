"""ARCHITECTURE.md section numbering for the merge train.

renumber MB MAIN   at the PR head: the `## 7X.` sections the PR adds (relative to merge-base MB,
                   compared by heading text) take the suffixes right after MAIN's last one, and every
                   `§7X` in lines the PR added (any file) follows it. Edits files in place; prints
                   the mapping, or nothing when no section moves.
resolve PATH       during a merge: each conflict hunk whose base side is empty (both sides appended
                   at the same place) becomes theirs, a blank line, then ours. Any other conflict
                   hunk exits 1 without writing.
"""
import re
import subprocess
import sys


def git(*args):
    return subprocess.run(['git', *args], capture_output=True, text=True, check=True).stdout


def ordinal(suffix):
    """Section suffixes run a..z, then aa, ab, ...: bijective base 26."""
    n = 0
    for ch in suffix:
        n = n * 26 + ord(ch) - 96
    return n


def suffix(n):
    s = ''
    while n:
        n, r = divmod(n - 1, 26)
        s = chr(97 + r) + s
    return s


def renumber(mb, main):
    heading = re.compile(r'^## 7([a-z]+)\..*$', re.M)
    on_main = {m.group(0): m.group(1) for m in heading.finditer(git('show', f'{main}:ARCHITECTURE.md'))}
    added, removed = {}, {}
    current = None
    for line in git('diff', '-U0', mb, 'HEAD').splitlines():
        if line.startswith('+++ ') or line.startswith('--- '):
            if line.startswith('+++ '):
                current = line[6:] if line.startswith('+++ b/') else None
        elif line.startswith('+') and current:
            added.setdefault(current, set()).add(line[1:])
        elif line.startswith('-') and current:
            removed.setdefault(current, set()).add(line[1:])
    # The PR's own sections are the headings it adds that main does not already have, compared by
    # their whole text: a suffix alone cannot tell the PR's 7n from another section's 7n on main.
    # A heading the PR rewrites (it removes one with the same suffix or the same title) is an edit
    # of an existing section, not a new one, and keeps its place.
    def title(h):
        return h.split('. ', 1)[1] if '. ' in h else h
    gone = [(m.group(1), title(m.group(0))) for m in heading.finditer('\n'.join(removed.get('ARCHITECTURE.md', ())))]
    new_heads = [(m.group(1), m.group(0)) for m in heading.finditer(open('ARCHITECTURE.md', encoding='utf-8').read())
                 if m.group(0) in added.get('ARCHITECTURE.md', ()) and m.group(0) not in on_main]
    # Each removed heading accounts for at most one added one: a moved section (same title) first,
    # then a retitled one (same suffix), in document order; what is left over is new.
    for match in (lambda h, g, t: title(h) == t, lambda h, g, t: h.split('. ', 1)[0][4:] == g):
        kept = []
        for suffix_, h in new_heads:
            hit = next((i for i, (g, t) in enumerate(gone) if match(h, g, t)), None)
            if hit is None:
                kept.append((suffix_, h))
            else:
                gone.pop(hit)
        new_heads = kept
    mine = sorted({suffix_ for suffix_, _ in new_heads}, key=ordinal)
    new_lines = {h for _, h in new_heads}
    if not mine:
        return
    last = max((ordinal(v) for v in on_main.values()), default=0)
    mapping = {k: suffix(last + i + 1) for i, k in enumerate(mine) if k != suffix(last + i + 1)}
    if not mapping:
        return
    pattern = re.compile(r'(§7|^## 7)(' + '|'.join(sorted(mapping, key=len, reverse=True)) + r')(?![a-z])')
    for path, lines in added.items():
        # A heading line moves only if it is one of the PR's new sections; an edited heading keeps
        # its suffix even when it shares it with a new one.
        hits = {l for l in lines if pattern.search(l) and (not heading.match(l) or l in new_lines)}
        if not hits:
            continue
        with open(path, encoding='utf-8') as fh:
            text = fh.read().split('\n')
        text = [pattern.sub(lambda m: m.group(1) + mapping[m.group(2)], l) if l in hits else l for l in text]
        with open(path, 'w', encoding='utf-8') as fh:
            fh.write('\n'.join(text))
    print(' '.join(f'7{k}->7{v}' for k, v in mapping.items()))


def resolve(path):
    stages = {}
    for n in (1, 2, 3):
        stages[n] = f'.arch-stage{n}'
        with open(stages[n], 'w', encoding='utf-8') as fh:
            fh.write(git('show', f':{n}:{path}'))
    merged = subprocess.run(['git', 'merge-file', '-p', '--diff3', stages[2], stages[1], stages[3]],
                            capture_output=True, text=True).stdout
    for f in stages.values():
        subprocess.run(['rm', '-f', f])
    out, state, ours, base, theirs = [], None, [], [], []
    for line in merged.split('\n'):
        if line.startswith('<<<<<<< '):
            state, ours, base, theirs = 'ours', [], [], []
        elif line.startswith('||||||| ') and state == 'ours':
            state = 'base'
        elif line == '=======' and state == 'base':
            state = 'theirs'
        elif line.startswith('>>>>>>> ') and state == 'theirs':
            if any(l.strip() for l in base):
                raise SystemExit(f'conflict in {path} rewrites shared text; not an append')
            while theirs and not theirs[-1].strip():
                theirs.pop()
            out += theirs + [''] + ours
            state = None
        elif state == 'ours':
            ours.append(line)
        elif state == 'base':
            base.append(line)
        elif state == 'theirs':
            theirs.append(line)
        else:
            out.append(line)
    if state is not None:
        raise SystemExit(f'unterminated conflict in {path}')
    with open(path, 'w', encoding='utf-8') as fh:
        fh.write('\n'.join(out))


if __name__ == '__main__':
    if sys.argv[1] == 'renumber':
        renumber(sys.argv[2], sys.argv[3])
    else:
        resolve(sys.argv[2])
