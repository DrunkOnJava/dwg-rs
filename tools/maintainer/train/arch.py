"""ARCHITECTURE.md section numbering for the merge train.

renumber MB MAIN   at the PR head: the `## 7X.` sections the PR adds (relative to merge-base MB,
                   compared by heading text) take the suffixes right after MAIN's last one, and every
                   `§7X` in lines the PR added (any file) follows it, as it follows a section MB
                   has that MAIN gives another suffix (matched by title). Edits files in place;
                   prints the mapping, or nothing when no section moves.
resolve PATH       during a merge: each conflict hunk whose base side is empty (both sides appended
                   at the same place) becomes theirs, a blank line, then ours, less every `## `
                   section of ours that main's file already holds word for word (a PR stacked on
                   another carries that PR's sections, which main now has from its squash). Any
                   other conflict hunk exits 1 without writing.
check [PATH]       the `## 7X.` sections of PATH (default ARCHITECTURE.md) run 7a, 7b, ... with no
                   suffix repeated or skipped; exits 1 naming the first one that does not (CI).
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
    last = max((ordinal(v) for v in on_main.values()), default=0)
    own = {k: suffix(last + i + 1) for i, k in enumerate(mine) if k != suffix(last + i + 1)}
    # A PR stacked on one that has landed cites that PR's sections by the letters they had at the
    # merge base; main gave them new ones when it landed. Those sections are matched by title, when
    # the title is unique on both sides, and the PR's lines follow them too.
    def by_title(text):
        seen = {}
        for m in heading.finditer(text):
            seen.setdefault(title(m.group(0)), []).append(m.group(1))
        return {t: s[0] for t, s in seen.items() if len(s) == 1}
    then, now = by_title(git('show', f'{mb}:ARCHITECTURE.md')), by_title(git('show', f'{main}:ARCHITECTURE.md'))
    moved = {s: now[t] for t, s in then.items() if t in now and now[t] != s and s not in own}
    mapping = {**moved, **own}
    if not mapping:
        return
    pattern = re.compile(r'(§7|^## 7)(' + '|'.join(sorted(mapping, key=len, reverse=True)) + r')(?![a-z])')
    used = set()
    def swap(m):
        used.add(m.group(2))
        return m.group(1) + mapping[m.group(2)]
    for path, lines in added.items():
        # A heading line moves only if it is one of the PR's new sections; an edited heading keeps
        # its suffix even when it shares it with a new one.
        hits = {l for l in lines if pattern.search(l) and (not heading.match(l) or l in new_lines)}
        if not hits:
            continue
        with open(path, encoding='utf-8') as fh:
            text = fh.read().split('\n')
        text = [pattern.sub(swap, l) if l in hits else l for l in text]
        with open(path, 'w', encoding='utf-8') as fh:
            fh.write('\n'.join(text))
    # Only the moves that rewrote a line are reported: an empty report means nothing changed.
    done = [f'7{k}->7{v}' for k, v in own.items() if k in used] + [f'§7{k}->§7{v}' for k, v in moved.items() if k in used]
    if done:
        print(' '.join(done))


def sections(lines):
    """Each `## ` section of lines, heading to the next heading, as stripped text."""
    out, block = set(), []
    for line in lines + ['## ']:
        if line.startswith('## '):
            if block and block[0].startswith('## '):
                out.add('\n'.join(block).strip())
            block = []
        block.append(line)
    return out


def without_known(lines, main_text):
    """Lines less each `## ` section (heading to the next heading) that equals one of main_text's whole sections."""
    known = sections(main_text.split('\n'))
    kept, block = [], []
    def flush():
        text = '\n'.join(block).strip()
        if not (block and block[0].startswith('## ') and text in known):
            kept.extend(block)
    for line in lines:
        if line.startswith('## '):
            flush()
            block = []
        block.append(line)
    flush()
    while kept and not kept[-1].strip():
        kept.pop()
    return kept


def resolve(path):
    stages = {}
    for n in (1, 2, 3):
        stages[n] = f'.arch-stage{n}'
        with open(stages[n], 'w', encoding='utf-8') as fh:
            fh.write(git('show', f':{n}:{path}'))
    merged = subprocess.run(['git', 'merge-file', '-p', '--diff3', stages[2], stages[1], stages[3]],
                            capture_output=True, text=True).stdout
    with open(stages[3], encoding='utf-8') as fh:
        main_text = fh.read()
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
            out += theirs + [''] + without_known(ours, main_text)
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


def check(path):
    with open(path, encoding='utf-8') as f:
        suffixes = re.findall(r'^## 7([a-z]+)\.', f.read(), re.M)
    for n, s in enumerate(suffixes, 1):
        if ordinal(s) != n:
            raise SystemExit(f'{path}: section 7{s} where 7{suffix(n)} belongs '
                             f'(sections: {", ".join("7" + x for x in suffixes)})')


if __name__ == '__main__':
    args = sys.argv[1:]
    if args[:1] == ['renumber'] and len(args) == 3:
        renumber(args[1], args[2])
    elif args[:1] == ['resolve'] and len(args) == 2:
        resolve(args[1])
    elif args[:1] == ['check'] and len(args) <= 2:
        check(args[1] if len(args) == 2 else 'ARCHITECTURE.md')
    else:
        raise SystemExit(__doc__)
