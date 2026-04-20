# entity-decoder template

Scaffold a new entity decoder for dwg-rs.

## Usage

```bash
cargo install cargo-generate
cargo generate --git https://github.com/DrunkOnJava/dwg-rs --name <entity_name> templates/entity-decoder
```

Then move the generated file into `src/entities/`, add a `pub mod` line in
`src/entities/mod.rs`, and wire the dispatcher in `src/entities/dispatch.rs`.

See [`docs/EXTENDING_DECODERS.md`](../../docs/EXTENDING_DECODERS.md) for the
full walkthrough.
