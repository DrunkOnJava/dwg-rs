#!/usr/bin/env python3
"""Supplement compiler-enforced no_std/no-allocator grammar isolation."""
from pathlib import Path
import re

root = Path('src/streaming')
files = sorted(root.rglob('*.rs'))
assert files, 'streaming grammar is missing'
for path in files:
    text = path.read_text()
    # Also ban these in comments, so an exception is explicit and reviewable.
    for pattern in (r'\bextern\s+crate\b', r'\binclude\s*!', r'\bstd\s*::', r'\balloc\s*::', r'\b(?:Vec|String|Box|Rc|Arc)\b', r'\bread_to_(?:end|string)\b', r'\bSeekFrom\b', r'\bDwgFile\b', r'\bunsafe\b'):
        assert not re.search(pattern, text), f'{path}: forbidden grammar capability {pattern}'
    # The independent no_std build resolves only this grammar and crc. New
    # transport/legacy dependencies cannot compile in that environment.
print(f'Grammar isolation: {len(files)} files; compile no_std.rs to enforce dependencies')
