"""Prune the repository's GitHub Actions caches (the audit's "Actions cache" check).

Usage: python3 tools/maintainer/clean_caches.py [--delete]

Lists, and with --delete removes, every cache that can no longer be restored usefully:
a pull request's caches once that pull request is closed or merged, and on any other ref every
cache but the most recently used one per key family (the key with its trailing hash parts
dropped). Without --delete it only prints what it would remove and the space that frees.
"""
import json
import re
import subprocess
import sys


def gh(*args):
    return subprocess.run(['gh', *args], capture_output=True, text=True, check=True).stdout


def family(key):
    return re.sub(r'(-[0-9a-f]{8,})+$', '', key)


def main(delete):
    caches = json.loads(gh('cache', 'list', '--limit', '1000', '--json',
                           'id,key,ref,sizeInBytes,lastAccessedAt'))
    open_prs = {str(n) for n in json.loads(gh('pr', 'list', '--state', 'open', '--limit', '1000',
                                              '--json', 'number', '--jq', '[.[].number]'))}
    doomed, newest = [], {}
    for c in sorted(caches, key=lambda c: c['lastAccessedAt'], reverse=True):
        pr = re.fullmatch(r'refs/pull/(\d+)/merge', c['ref'])
        if pr and pr.group(1) not in open_prs:
            doomed.append((c, f"PR #{pr.group(1)} is closed"))
        elif (c['ref'], family(c['key'])) in newest:
            doomed.append((c, f"superseded by {newest[(c['ref'], family(c['key']))]}"))
        else:
            newest[(c['ref'], family(c['key']))] = c['key']
    total = sum(c['sizeInBytes'] for c in caches)
    freed = sum(c['sizeInBytes'] for c, _ in doomed)
    for c, why in doomed:
        print(f"{c['sizeInBytes'] / 2**20:8.1f} MB  {c['ref']}  {c['key']}  ({why})")
        if delete:
            gh('cache', 'delete', str(c['id']))
    verb = 'deleted' if delete else 'would delete'
    print(f"{len(caches)} caches, {total / 2**30:.2f} GB; {verb} {len(doomed)}, "
          f"{freed / 2**30:.2f} GB, leaving {(total - freed) / 2**30:.2f} GB")


if __name__ == '__main__':
    main('--delete' in sys.argv[1:])
