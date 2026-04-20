# Raw entity byte fixtures

Hand-authored byte-level fixtures for per-entity decoder regression
tests. Distinct from `tests/fixtures/canonical/`, which holds full
`.dwg` files and is blocked on the write path (stages 2-5 of
`file_writer.rs`).

## Layout

Each fixture lives in a subdirectory named after the entity:

```
tests/fixtures/raw_entities/
  line/
    minimal_r2018.bin   # LINE with start=(1,2,3), end=(4,5,6), no thickness
    minimal_r2018.toml  # Expected decoded values for assertion
  circle/
    ...
```

The `.bin` file is a byte-level payload that feeds directly into the
per-entity `decode` function (via a `BitCursor::new(&bytes)`). The
`.toml` sidecar lists expected field values so the test harness can
assert equality without the fixture author having to hand-code the
same struct literal in Rust.

## Scope

These fixtures exist to test the **per-entity decoder**, NOT the
full object-stream walker. That is: they skip the 4-byte 0x0DCA
prefix, the MS object size, the handle, the common entity preamble.
The test harness creates a `BitCursor` positioned exactly where
`dispatch.rs` would hand it to the per-entity function.

This intentionally narrows the surface so that decoder-correctness
regressions are caught without waiting on the still-pending fixes
for the object-stream walker (see `ROADMAP.md` 0.2.0 milestone).

## Contributing a fixture

1. Construct the payload by hand or via a small Rust snippet using
   `BitWriter`. Save the bytes to `<entity>/<name>.bin`.
2. Document the expected field values in `<entity>/<name>.toml`.
3. Add a test to `tests/entity_value_regression.rs` that loads the
   fixture and asserts equality.
4. Cite the ODA Open Design Specification section in a comment so
   future readers can verify the bytes match the spec.

As of 0.1.0-alpha.1 this directory is empty — fixtures will be
populated as part of the 0.2.0 entity-correctness milestone. The
directory is committed early so CI and crate-published tarballs can
assume it exists.
