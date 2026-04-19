# Security Policy

## Reporting a Vulnerability

If you discover a security issue in `dwg-rs`, please report it privately
via GitHub's **Security Advisories** feature:

**https://github.com/DrunkOnJava/dwg-rs/security/advisories/new**

Do not open a public issue for security problems.

## Threat Model

`dwg-rs` is a parser that consumes potentially untrusted input — any DWG
file from any source. The relevant threat classes are:

- **Memory safety** — all code is safe Rust (`#![deny(unsafe_code)]` in
  `lib.rs`). Out-of-bounds reads, integer overflows, and panic-on-malformed
  inputs are the primary concerns.
- **Denial of service** — a crafted file that causes exponential runtime
  or unbounded memory allocation. Defensive caps are in place
  (1M-entry bounds on dictionaries, handle maps, spline control points;
  16MB caps on XRECORD payloads; 4096-entry caps on class tables).
- **Decompression bombs** — LZ77 streams with highly repetitive back-
  references that expand to gigabytes from kilobytes. The decoder
  accepts an optional `expected_size` argument that callers should set.

Out of scope:

- Cryptographic security — DWG's R2004+ XOR and Sec_Mask are obfuscation,
  not encryption, and this crate makes no claim of confidentiality.
- Parsing correctness against adversarial files — fuzzing is welcome;
  reports of panics / OOM / unresponsiveness are fair security issues,
  but incorrect geometry decoding is a correctness bug, not a security
  issue.

## Supported Versions

`dwg-rs` is pre-1.0. Security fixes land on `main`; there are no
long-term-support branches yet.

| Version | Supported |
|---------|-----------|
| 0.1.x   | ✓         |
| < 0.1   | ✗         |

## Response Timeline

This is a solo-maintained project. Acknowledgement within 7 days of
report is typical; a fix or mitigation within 30 days when feasible.
Complex issues requiring spec re-interpretation may take longer.

Credit is given in release notes unless the reporter requests otherwise.
