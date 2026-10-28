"""The DXF oracle baseline a refreshed PR is ratcheted against.

Usage: oracle_base.py MERGE_BASE PR_HEAD [MAIN] > baseline.tsv

Prints MAIN's (default origin/main) tools/oracle/baseline.tsv less every row (keyed by release, type, field and
group, as dxf_oracle --baseline keys them) that the PR's own committed TSV dropped since the merge
base: a field the PR renamed or retired is not a fall in agreement.
"""
import subprocess
import sys


def rows(ref):
    r = subprocess.run(['git', 'show', f'{ref}:tools/oracle/baseline.tsv'], capture_output=True, text=True)
    return r.stdout.splitlines() if r.returncode == 0 else []


def key(line):
    f = line.split('\t')
    return None if line.startswith('#') or len(f) < 5 else (f[0], f[2], f[3], f[4])


if __name__ == '__main__':
    retired = {key(l) for l in rows(sys.argv[1])} - {key(l) for l in rows(sys.argv[2])}
    main = sys.argv[3] if len(sys.argv) > 3 else 'origin/main'
    sys.stdout.write(''.join(l + '\n' for l in rows(main) if key(l) is None or key(l) not in retired))
