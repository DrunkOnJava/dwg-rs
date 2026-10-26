# Roadmap

Last reviewed: 2026-10-21

The plan is the [milestones](https://github.com/DrunkOnJava/dwg-rs/milestones),
the epics under them, and the sections of the spec dwg-rs is built from. This
page describes them; [`docs/spec-coverage.md`](docs/spec-coverage.md) gives
every section of the *Open Design Specification for .dwg files* v5.4.1 its
state on `main`; [`docs/release-1.0-criteria.md`](docs/release-1.0-criteria.md)
is the 1.0 release decision; and [`docs/maintaining.md`](docs/maintaining.md)
says how the tracker is run.

## Where 1.0 is going

dwg-rs 1.0 is a complete, permissively licensed DWG library with no gaps or
limitations (the maintainer's decision, 2026-10-21):

- it reads every release from R13 to R2018, every section of ODA spec
  v5.4.1, with every value equal to AutoCAD's own DXF export of the same
  drawing;
- it writes DWG that AutoCAD opens without recovery, on every release;
- its DXF export equals AutoCAD's, and its SVG, PDF and glTF exports equal
  the decoded geometry;
- the browser viewer has no stubs, and the Python bindings reach the whole
  reader.

## Where it stands (2026-10-21)

| Area | State on `main` | Evidence |
|---|---|---|
| Containers | R14 to R2018 read, R2007 included (#110); R13 never seen | STATUS.md |
| Object walk | Every corpus file tiles its object stream with zero bytes between records; reference closure closed on all 44 R2000-to-R2018 drawings, NOT MEASURED on R14 (#84); class-census shortfalls pinned per file (#230) | the Corpus CI job ([run 40304926288](https://github.com/DrunkOnJava/dwg-rs/actions/runs/40304926288)), `tools/corpus/baseline.tsv` |
| Decode | 13,781 records decoded, 1,985 skipped, 308 errored (85.7%) on 52 public drawings, R14 to R2018; every entity record boundary-checked (#63) | the Corpus CI job ([run 40304926288](https://github.com/DrunkOnJava/dwg-rs/actions/runs/40304926288)), README.md |
| Values | Checked for LINE, CIRCLE and ARC on R2013 only; nothing compared with AutoCAD's DXF | `tests/r2013_entity_values.rs` |
| Spec coverage | Of the 104 object prescriptions in §20.4, 46 close on the public corpus in CI; 22 have no decoder, 30 are partial and 6 are unmeasured; seven data sections are unread; the header variables (§9) are read, close on all 52 and agree with AutoCAD's DXF export on every comparable variable (ARCHITECTURE.md §7n) | [`docs/spec-coverage.md`](docs/spec-coverage.md) |
| Writer | R2004-family section round-trips; no file shown to open in AutoCAD | ARCHITECTURE.md §8 |
| Exports, viewer, Python | DXF, SVG, glTF and PDF written and checked syntactically; the viewer has stubs; Python has type stubs only | STATUS.md |
| CI | Synthetic fixtures only: no real DWG file is read in CI | `.github/workflows/ci.yml` |
| Releases | Nothing published; a v0.1.0-alpha.1 draft from April | GitHub releases, crates.io |

## Milestones

### 0.2.0: measured reader, first release

Every published number comes from a public corpus measured in CI, values are
checked against AutoCAD's DXF, every spec section is traced, the 9 errored
corpus records are closed, and the `dwg` crate is published for the first
time.

- Measure the reader on public files in CI (#94): the
  [nextgis/dwg_samples](https://github.com/nextgis/dwg_samples) drawings
  (AutoCAD 2016 saves of the same drawings as R14 to 2013, each with
  AutoCAD's DXF export) fetched by pinned hash (#95), the R2018 file's
  provenance (#96), the coverage report ratcheted in CI (#97), and a
  Measure workflow (#98).
- AutoCAD's own DXF export as the value oracle (#99): a DXF reader keyed by
  handle (#100), record pairing (#101), field scorers (#102) and their CI
  ratchet (#103).
- Trace every spec section (#104): the coverage matrix's CI check (#105), one
  citation numbering (#106), and the spec's own worked examples as fixtures
  (#107).
- Close the decode residue (#108): the 9 errored records (#71), latent wrong
  field types (#62), the pre-R2007 symbol tables (#34, #35, #40), and two API
  traps (#81, #83).
- First release (#109): trusted publishing (#110), the stale draft (#111),
  numbers from CI (#112), and post-publish checks (#113).
- Repository health (#114): the audit (#115), security settings (#116),
  rulesets as code (#117).

### 0.3.0: every spec section read, on every release

The reader is complete: every section of chapters 2 to 29, and every
prescription of §20.4, for every release from R13 to R2018, on a corpus that
holds each one.

- File structure on every release (#118): R13 (#119), the R13-R15 header
  fields (#120), the second file header (#121), protected and signed
  drawings (#122), colour objects (#123), the containers' constants (#124).
- Header variables, §9 (#125): a table-driven decoder (#126) and the
  variables of each release band (#127 to #130).
- Data sections (#131): AuxHeader (#132), RevHistory, ObjFreeSpace and
  Template (#133), VBAProject (#134), the unnamed and unknown sections
  (#135), DataStorage and the ACIS payloads (#136), and re-verifying the
  sections already read (#137).
- Every object prescription of §20 (#138), by family: common formats (#139),
  text (#140), blocks (#141), polylines (#142), curves (#143), dimensions
  (#144), planar and ACIS (#145), viewports and layouts (#146), leaders,
  multilines and hatches (#147), symbol tables (#148), dictionaries and
  non-graphical objects (#149), raster, OLE and geolocation (#150), the
  TABLE family (#151), and the classes the spec does not prescribe (#152).
- A corpus that holds every section on every release (#153): a registry
  (#154), a section-by-release coverage report (#155), an authored corpus
  (#156), a survey of public sources (#157), and a held-out set for 1.0
  (#158).

### 0.4.0: a DWG writer AutoCAD opens

Byte-identical round-trips (#160), the R2007 (#161) and R13-R2000 (#162)
containers, a real LZ77 compressor (#163), header variables (#164), an
encoder for every object (#165), an authoring API (#166), and an openability
protocol with recorded evidence (#167). Epic: #159.

### 0.5.0: exports equal to AutoCAD's

DXF equal to AutoCAD's (#169), nothing dropped silently (#170), SVG and PDF
geometry (#171), glTF that validates (#172), and solids tessellated from their
ACIS data (#173). Epic: #168.

### 0.6.0: browser viewer and Python bindings

The viewer without stubs (#174): selection (#175), section box and clipping
(#176), progressive open (#177), print preview and spaces (#178), and browser
tests with the zero-network invariant (#179). Python (#180): a pyo3 crate
(#181), wheels on PyPI (#182), and end-to-end tests (#183).

### 0.9.0: hardening and the stable surface

Hardening (#184): one ParseLimits (#185), best-effort diagnostics (#186), fuzz
hours (#187), no panics on 64- or 32-bit targets (#188), performance budgets
(#189). The stable surface (#190): a workspace split (#191), semver checks
(#192), examples that run (#193), and release policies (#194).

### 1.0.0: complete and stable

The criteria in [`docs/release-1.0-criteria.md`](docs/release-1.0-criteria.md)
all passed (#195), held-out files measured before any fix (#196), and
practitioners' reports (#197).

## How to help

The highest-leverage work is in 0.2.0: CI measurement (#94) and the DXF
oracle (#99) make every later claim checkable. After that, any `missing` or
`partial` row of [`docs/spec-coverage.md`](docs/spec-coverage.md) is a
self-contained task: pick its family issue, say which section you are taking,
and cite the section in the PR. Corpus files with a DXF export and a licence
that lets CI fetch them are as valuable as code (#153). Read
[`CONTRIBUTING.md`](CONTRIBUTING.md) and [`CLEANROOM.md`](CLEANROOM.md) first.
