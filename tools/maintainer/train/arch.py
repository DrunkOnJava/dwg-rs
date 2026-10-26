"""ARCHITECTURE.md section numbering for the merge train.

renumber MB MAIN   at the PR head: every `## 7X.` section the PR adds (relative to merge-base MB)
                   whose letter is already taken on MAIN moves to the next free letter, and every
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


def renumber(mb, main):
    taken = set(re.findall(r'^## 7([a-z])\.', git('show', f'{main}:ARCHITECTURE.md'), re.M))
    base = set(re.findall(r'^## 7([a-z])\.', git('show', f'{mb}:ARCHITECTURE.md'), re.M))
    added = {}
    current = None
    for line in git('diff', '-U0', mb, 'HEAD').splitlines():
        if line.startswith('+++ '):
            current = line[6:] if line.startswith('+++ b/') else None
        elif line.startswith('+') and current:
            added.setdefault(current, set()).add(line[1:])
    mine = re.findall(r'^## 7([a-z])\.', '\n'.join(added.get('ARCHITECTURE.md', ())), re.M)
    mine = sorted(set(mine) - base)
    if not mine:
        return
    last = max(taken | base)
    free = [c for c in 'abcdefghijklmnopqrstuvwxyz' if c > last]
    if len(free) < len(mine):
        raise SystemExit('no section letters left after 7' + last)
    mapping = {k: v for k, v in zip(mine, free) if k != v}
    if not mapping:
        return
    pattern = re.compile(r'(§7|^## 7)(' + '|'.join(mapping) + r')(?![a-z])')
    for path, lines in added.items():
        hits = {l for l in lines if pattern.search(l)}
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
