#!/usr/bin/env python3
"""The fixed-code dispatcher's label list and its body arms stay in step.

src/entities/dispatch.rs gates every fixed-code record through
`fixed_entity_label` before `decode_fixed_entity_body` reads it. A code the
body decodes but the label list omits never reaches the body: the record
comes back `Unhandled` and is counted as skipped, with no error to say a
decoder exists. A code the list names but the body does not decode comes back
`Error` ("no fixed-code entity decoder"). Both lists are read from the source,
so a new decoder that adds its arm to one and not the other fails here.

A label arm with no body arm is tolerated only where it is listed below with
its reason, and the entry must go the day the arm does: the check fails when a
listed exception stops being one, as tests/class_census.rs does for its gaps.

Usage:

    python3 tools/ci/dispatch_arms.py --check
"""

import re
import sys
from pathlib import Path

SOURCE = Path(__file__).resolve().parents[2] / "src" / "entities" / "dispatch.rs"

LABEL_ONLY = {
    "OBJECT_TYPE_GEODATA": (
        "GEODATA (0x4FB) is a non-entity object: decode_in_page sends it to "
        "dispatch_object before dispatch_fixed_code, so its label arm is never read"
    ),
}


def sanitize(src):
    """The source with comments and the insides of string and char literals blanked."""
    out = list(src)
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            out[i:j] = " " * (j - i)
            i = j
        elif src.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif src.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            blank(out, src, i, j)
            i = j
        elif c in "rb" and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")):
            m = re.compile(r'b?r(#*)"').match(src, i)
            if m:
                end = src.find('"' + m.group(1), m.end())
                end = n if end < 0 else end + 1 + len(m.group(1))
                blank(out, src, m.end(), end)
                i = end
            else:
                i += 1
        elif c == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            blank(out, src, i + 1, j)
            i = j + 1
        elif c == "'":
            m = re.compile(r"'(\\.[^']*|[^\\'])'").match(src, i)
            if m:
                blank(out, src, i + 1, m.end() - 1)
                i = m.end()
            else:
                i += 1
        else:
            i += 1
    return "".join(out)


def blank(out, src, a, b):
    for k in range(a, min(b, len(src))):
        if src[k] != "\n":
            out[k] = " "


def close_of(text, open_at):
    pairs = {"{": "}", "(": ")", "[": "]"}
    stack = []
    for k in range(open_at, len(text)):
        ch = text[k]
        if ch in pairs:
            stack.append(pairs[ch])
        elif ch in ")}]":
            if not stack or stack.pop() != ch:
                raise SystemExit(f"{SOURCE.name}: unbalanced {ch!r} at offset {k}")
            if not stack:
                return k
    raise SystemExit(f"{SOURCE.name}: no closing bracket for offset {open_at}")


def match_arms(text, fn):
    sig = re.search(r"\bfn\s+" + fn + r"\s*\(", text)
    if not sig:
        raise SystemExit(f"{SOURCE.name}: no `fn {fn}`")
    args_end = close_of(text, sig.end() - 1)
    body_start = text.index("{", args_end)
    body = text[body_start : close_of(text, body_start) + 1]
    m = re.search(r"\bmatch\s+type_code\s*\{", body)
    if not m:
        raise SystemExit(f"{SOURCE.name}: `fn {fn}` has no `match type_code`")
    arms_text = body[m.end() : close_of(body, m.end() - 1)]
    arms, i, n = [], 0, len(arms_text)
    while True:
        while i < n and arms_text[i].isspace():
            i += 1
        if i >= n:
            return arms
        j, depth = i, 0
        while j < n and not (depth == 0 and arms_text.startswith("=>", j)):
            depth += arms_text[j] in "({["
            depth -= arms_text[j] in ")}]"
            j += 1
        if j >= n:
            raise SystemExit(f"{SOURCE.name}: `fn {fn}` has an arm with no `=>`")
        pattern, i = arms_text[i:j].strip(), j + 2
        while i < n and arms_text[i].isspace():
            i += 1
        if i < n and arms_text[i] == "{":
            end = close_of(arms_text, i) + 1
            expr, i = arms_text[i:end], end
            while i < n and arms_text[i].isspace():
                i += 1
            if i < n and arms_text[i] == ",":
                i += 1
        else:
            j, depth = i, 0
            while j < n and not (depth == 0 and arms_text[j] == ","):
                depth += arms_text[j] in "({["
                depth -= arms_text[j] in ")}]"
                j += 1
            expr, i = arms_text[i:j].strip(), j + 1
        arms.append((pattern, expr))


def atoms(pattern):
    return {"".join(a.split()) for a in pattern.split("|")} - {"_"}


def main():
    if sys.argv[1:] != ["--check"]:
        raise SystemExit(__doc__)
    text = sanitize(SOURCE.read_text())
    labelled, decoded = set(), set()
    for pattern, expr in match_arms(text, "fixed_entity_label"):
        if not expr.startswith("return"):
            labelled |= atoms(pattern)
    for pattern, expr in match_arms(text, "decode_fixed_entity_body"):
        decoded |= atoms(pattern)
    problems = []
    if len(labelled) < 30 or len(decoded) < 30:
        problems.append(
            f"read only {len(labelled)} label arms and {len(decoded)} body arms: "
            f"this script no longer understands {SOURCE.name}"
        )
    for code in sorted(decoded - labelled):
        problems.append(
            f"{code} is decoded by decode_fixed_entity_body but has no label in "
            "fixed_entity_label, so its records come back Unhandled"
        )
    label_only = labelled - decoded
    for code in sorted(label_only - set(LABEL_ONLY)):
        problems.append(
            f"{code} has a label in fixed_entity_label but no arm in "
            "decode_fixed_entity_body, so its records come back as errors"
        )
    for code in sorted(set(LABEL_ONLY) - label_only):
        problems.append(f"{code} is no longer a label-only code: remove it from LABEL_ONLY")
    if problems:
        print("\n".join(problems), file=sys.stderr)
        return 1
    print(
        f"{len(decoded)} decoded codes all labelled; "
        f"{len(label_only)} labelled without a body ({', '.join(sorted(label_only)) or 'none'})"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
