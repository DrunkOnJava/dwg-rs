# AGENTS.md

The repository's guidance for every coding agent is in `CLAUDE.md`; read it first. It is not specific to Claude Code.

## Review guidelines

- dwg-rs is a clean-room implementation (`CLEANROOM.md`). Never cite, quote, link or paraphrase LibreDWG, the ODA/Teigha SDK, an Autodesk SDK or any other GPL or proprietary DWG implementation in a review comment, and never suggest consulting one. Ground a finding in the ODA *Open Design Specification for .dwg files* v5.4.1 (cite its section), Autodesk's public *DXF Reference*, AutoCAD's own DWG and DXF output in the pinned corpus (`tools/corpus/pins.tsv`), or this repository's code.
- Do not ask for unit tests. The project adds none; it proves behaviour on the pinned public corpus, the DXF oracle and the end-to-end verifiers, so ask for that evidence instead.
