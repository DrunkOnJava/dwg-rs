# AutoCAD trial runbook

Scripts, AutoLISP builders and a checker for a session with AutoCAD for Mac open. Everything that can be
prepared without AutoCAD is prepared here, so the session only runs it. Nothing in this directory runs
AutoCAD, downloads anything or writes inside the repository: the generator writes under a work folder
outside it.

The work list is the maintainer's list for the trial: section 1 (R13 re-save twins, #119, #323, #345),
sections 2 and 3 (controlled drawings for the classes the specification never prints, #329, #331, #332,
#335, #337, #348, #358, #285, #290) and section 4 (pre-R2007 text in a real code page, #375, following #370
and #374). The scripts save ASCII DXF with `SAVEAS DXF V <version> 16 <file>`, not `DXFOUT`, because that
sequence is the one confirmed (see "Unverified prompt sequences").

| File | Does |
| --- | --- |
| `generate.py` | Reads `tools/corpus/pins.tsv` and a corpus folder; stages the R13 drawings; writes the `.scr` scripts, a copy of the `.lsp` files, a two-page PDF, a PNG and `manifest.tsv` |
| `lisp/*.lsp` | AutoLISP builders for the controlled drawings. Core AutoLISP only (`entmake`, `command`, `setvar`, file functions): no `vla-`, `vlax-` or `vl-` function |
| `check.py` | Walks the output folder against the manifest and prints a table and the next steps |
| `MANUAL-CHECKLIST.md` | Click-by-click steps for what AutoLISP cannot create, and the fallbacks |

## Before AutoCAD: generate

```sh
python3 -I tools/autocad/generate.py --scripted-only --corpus ~/Library/Caches/dwg-corpus
```

`--scripted-only` leaves out the drawings built by hand (`MANUAL-CHECKLIST.md`), so the manifest and the
checker list only what the scripts produce; without it they are listed and the checker asks for them.

`--pins FILE` names another pins file. The 22 R13 drawings (SketchUp Cabin and 21 Sovol) are pinned by
PR #323, not yet on `main`: until it lands, pass `--pins` the `tools/corpus/pins.tsv` of its branch,
and `--corpus` the folder that branch's `fetch.py` filled. Section 1 takes every `AC1012` DWG row of the
pins it is given, so the US Forest Service drawings pinned on the R13 corpus branch are run too
(`--sources sketchup,sovol` limits it to the 22). `--work DIR` moves the work folder (default `~/Library/Caches/dwg-autocad`;
the path must be plain ASCII). `--template` names the drawing template for `NEW` (default
`acadiso.dwt`). Every run saves its options in `<work>/session.json`. `--resume` reads them back for
whatever the command line does not give, and leaves out only what already has all its outputs and passes the
checker's tests (non-empty, the right DWG magic or DXF `$ACADVER`, and not older than the master or staged
input it was saved from): an empty, wrong or stale file is built again.
Always give the same `--work` when you resume; the checker's recovery commands do.
`--keyword-prefix _` writes the DXF and Version keywords of SAVEAS with the underscore that selects the
English keyword, for a session in another language (see the Russian and Japanese section).

It stages each R13 drawing under an ASCII name (`<source>-<nn>-<sha8>.dwg`, sha256 checked against the
pin), because the Sovol file names are Chinese and have spaces, and a script file is read in a code page.
The manifest ties each staged name back to the original file name and sha256.

## In AutoCAD

Use the English interface for this run; the command names in the scripts carry `_.`, and SAVEAS's format
keywords and the DXF `V` keyword are written as AutoCAD 2025 accepted them (`--keyword-prefix _` is for other languages). Sign-in and the trial are the maintainer's.

Before the first command:

- Open a blank new drawing and leave it as the only one. A script needs an open document to run in, and
  `00-start.scr` saves that drawing as `<out>/session.dwg` so that `NEW` and `OPEN` cannot replace it.
- Keep the Command window open: builders print `dwgrs:` lines there and a failure scrolls past.
- `00-start.scr` adds `<work>/lisp` to `TRUSTEDPATHS` so `SECURELOAD` does not stop each `(load ...)`. If a
  load dialog still appears, answer Always load, or set `TRUSTEDPATHS` by hand.

The first three commands, with `<work>` the folder the generator printed (default
`~/Library/Caches/dwg-autocad`):

```
-SCRIPT   <work>/scripts/00-start.scr
-SCRIPT   <work>/scripts/s1-all.scr
-SCRIPT   <work>/scripts/build-all.scr
```

`-SCRIPT` asks for the file name at the command line whatever `FILEDIA` is; `SCRIPT` does the same once
`FILEDIA` is 0. Use a work folder whose path has no spaces if you can; the scripts quote a path that has
one, which is unverified on the Mac build.

1. `00-start.scr` writes `<out>/session-info.txt` with `DWGCODEPAGE`, `SYSCODEPAGE`, `LOCALE` and the
   build, which section 4 needs first (they decide which page the saves carry). It also runs
   `DWGRS-CHARTEST`, which prints the character codes AutoLISP stores for an accented letter in `chr`
   mode and in `escape` mode, and writes the strings to `<out>/chartest.txt`. The `chr` line reads
   `116 233 117` or `116 195 169 117` (AutoCAD holds the string as UTF-8) followed by `92 85 43 48 52 49
   54`; either means `chr` works. A `63` where the accent should be means it does not: rerun the generator
   with `--char-mode escape` before the build step.
2. `s1-all.scr` opens each staged R13 drawing, `SAVEAS` R2000 DWG and `SAVEAS DXF` R2000 ASCII, then closes it.
3. `build-all.scr` makes a fresh drawing per AutoLISP item (`NEW`, load the builder, build), and saves it
   as a master DWG in `<out>/masters/`.
4. Not in a scripted-only run: build the manual masters from `MANUAL-CHECKLIST.md` (dynamic blocks, MATERIAL
   maps, DATATABLE, the typed code-page drawing, and a fallback for any builder that refused).
5. `python3 -I tools/autocad/generate.py --work <work> --resume`, so `save-*.scr` cover the masters that now
   exist.
6. `-SCRIPT <work>/scripts/save-all.scr` reopens every master and saves each release (newest first) as DWG
   plus `SAVEAS DXF V <version> 16` ASCII DXF. Sections can run alone: `save-s2.scr`, `save-s3.scr`,
   `save-s4.scr`.
7. `python3 -I tools/autocad/check.py`.

If a script stops at a prompt, press Esc and run the single-piece script that failed: `scripts/s1-<name>.scr`,
`scripts/items/build-<item>.scr` or `scripts/items/save-<item>.scr`. The checker prints which. The scripts set
`FILEDIA` and `CMDDIA` to 0 at the start and back to 1 before the final `CLOSE` (a drawing-less AutoCAD
cannot run `SETVAR`); a script that dies midway leaves them at 0, so run `FILEDIA` 1 and `CMDDIA` 1 yourself.

## Code-page saves under Russian and Japanese

The maintainer chose a scripted-only run with AutoCAD's own language set to Russian and then to Japanese for
the code-page saves (#375: pages 1251 and 932). Only AutoCAD's language changes: a per-application
`AppleLanguages` override on the AutoCAD bundle, removed afterwards; the macOS language is not touched. Do it
after the English run is finished and checked. Each language gets its own work folder, so
`session-info.txt` and the masters of one run are not overwritten by the next. `<AUTOCAD_BUNDLE_ID>` stands
for the bundle id, read once AutoCAD is installed.

1. Read the bundle id and keep it in a shell variable (the app name is whatever the install is called):

   ```sh
   BUNDLE=<AUTOCAD_BUNDLE_ID>
   mdls -name kMDItemCFBundleIdentifier -raw "/Applications/<AutoCAD folder>/<AutoCAD app>.app"
   ```

2. Record any existing override. Expect `The domain/default pair ... does not exist`; if a value prints,
   write it down, because step 9 writes it back instead of deleting:

   ```sh
   defaults read "$BUNDLE" AppleLanguages
   ```

3. Quit AutoCAD (Cmd+Q), then set Russian for this bundle only:

   ```sh
   defaults write "$BUNDLE" AppleLanguages -array ru
   ```

4. Generate the section 4 scripts in their own folder, with the English keyword prefix:

   ```sh
   python3 -I tools/autocad/generate.py --sections 4 --scripted-only --keyword-prefix _ --work ~/Library/Caches/dwg-autocad-ru
   ```

5. Start AutoCAD, open a blank drawing, and at the Command prompt (the underscore selects the English
   command name):

   ```
   _-SCRIPT   <work>/scripts/00-start.scr
   ```

   Read `<out>/session-info.txt`. `LOCALE` must say Russian and `DWGCODEPAGE` must say `ANSI_1251`. If
   `DWGCODEPAGE` still says `ANSI_1252`, the override changed the interface and not the page: quit AutoCAD,
   go to step 9 and report it; the saves would carry the wrong page.
6. Build, then save out:

   ```
   _-SCRIPT   <work>/scripts/build-s4.scr
   ```

   ```sh
   python3 -I tools/autocad/generate.py --work ~/Library/Caches/dwg-autocad-ru --resume
   ```

   ```
   _-SCRIPT   <work>/scripts/save-s4.scr
   ```

7. `python3 -I tools/autocad/check.py --work ~/Library/Caches/dwg-autocad-ru`. The R2004, R2000 and R12 DXFs
   must show `$DWGCODEPAGE` `ANSI_1251`, with `high=` and `esc=` counts; the Cyrillic text shows whether the
   page holds it as bytes or as `\U+XXXX`.
8. Quit AutoCAD and repeat steps 3 to 7 with Japanese: `-array ja`, the folder
   `~/Library/Caches/dwg-autocad-jp`, and `DWGCODEPAGE` `ANSI_932`.
9. Remove the override: `defaults delete "$BUNDLE" AppleLanguages` (or `defaults write "$BUNDLE" AppleLanguages
   -array <the value from step 2>`). `defaults read "$BUNDLE" AppleLanguages` must fail again with
   `does not exist`. Start AutoCAD and confirm the interface is English.

Whether AutoCAD for Mac follows `AppleLanguages`, and whether `DWGCODEPAGE` follows it, is unverified until the
install exists; step 5's check is what settles it.

## Where outputs land

```
<out>/session-info.txt
<out>/s1/<source>-<nn>-<sha8>.R2000.dwg|.dxf          section 1
<out>/masters/<item>.dwg                              each controlled drawing, saved as 2018
<out>/s2|s3|s4/<item>.<release>.dwg|.dxf              every release with its ASCII DXF
```

Releases: R2018, R2013, R2010, R2007, R2004, R2000 (which the item names), and for section 4 R14. R14 has no DXF
format, so its DXF twin is R12 (`<item>.R12.dxf`, `$ACADVER` AC1009), the nearest. `manifest.tsv` lists every
expected file with its magic or `$ACADVER`, the DXF record types it must hold, its licence and its hosting.

## The checker

```sh
python3 -I tools/autocad/check.py [--work DIR] [--tsv results.tsv] [--quiet]
```

One row per file present: status, item, kind, release, bytes, the DWG's six-byte magic or the DXF's
`$ACADVER`, `$DWGCODEPAGE`, sha256 prefix and the counts of the record types the item should produce. A
count below the expectation is status `TYPES`: AutoCAD wrote the file but silently skipped the object. Status
`STALE` means the export is older than the master (or staged input) it was saved from, so the master changed
after it and the save has to run again; it compares file times, so keep the work folder's times (`cp -p`, `rsync -t`). For the
section 4 drawings it adds `high=` (bytes above 0x7F: real code-page bytes) and `esc=` (`\U+XXXX` escapes). DXFs
are read as bytes, never decoded. Missing, empty or wrong files are listed as next steps. Exit status 0 only
when everything is present and right. `--tsv` writes every file with its full sha256 and byte count, which is
what a pins row needs.

## Which outputs may be pinned

| Output | Licence | Hosting |
| --- | --- | --- |
| Twins of the SketchUp Cabin drawing | Apache-2.0 (the source repository) | pin, with attribution |
| Twins of the Sovol3d drawings | none stated | local only; never redistributed |
| Twins of the US Forest Service drawings | public domain (US government work) | pin |
| Controlled drawings and their DXFs | built from the scripts here, no third-party content | pin |

The manifest's `hosting` column says which a file is. The controlled drawings come from an AutoCAD trial
install under the maintainer's decision that this is a non-commercial open-source project; the Sovol twins are
evidence read on this Mac and compared locally.

## What is scripted and what is by hand

| Work-list item | Route |
| --- | --- |
| Section 1: 22 R13 re-save twins | scripted: `s1-all.scr` |
| MATERIAL colours (70/71/76, 90/91/92) | `entmakex` attempt (`s2-material`), UI fallback in the checklist |
| MATERIAL map settings | by hand: `s2-material-maps` |
| WIPEOUTVARIABLES frame on and off | scripted: `s2-wipeout-0`, `-1`, `-2` |
| DBCOLOR second book | scripted (`s2-dbcolor`, book names unverified), UI fallback |
| PDFDEFINITION second page, #331 | scripted (`s2-pdf`, `s3-pdfunderlay-a`, `-b`), dialog fallback |
| Dynamic blocks, ACAD_EVALUATION_GRAPH | by hand from a scripted base block: `s2-dyn-*`, `s3-dyn-eg1` to `eg3` |
| DYNAMICBLOCKPURGEPREVENTER version | not settable; written per release with the dynamic blocks |
| DIMASSOC, #329 | scripted: `s2-dimassoc` (eight dimensions, an angular with three references) |
| ACSH_BOX and history | scripted: `s2-acsh-primitives` |
| ACSH_EXTRUSION | scripted: `s2-acsh-extrusion` |
| #337 annotative TEXT | scripted: `s3-textctx` |
| #332 SUN | `entmod` attempt on the SUN object (low confidence), palette steps in the checklist |
| #335 DATATABLE | no known command: exploratory steps in the checklist |
| PERSSUBENTMANAGER, seven drawings | scripted: `s3-pers-1` to `s3-pers-7` (R2010 and R2013) |
| #358 LWPOLYLINE PLINEGEN, polygon mesh | scripted: `s3-plinegen-mesh` |
| #285 saved UCS on R2000 and R2004 | scripted: `s3-ucs` |
| #290 xref-dependent table entries | scripted: `s3-xref-src`, `s3-xref-host` |
| #375 code-page text | scripted: `s4-latin1`, `s4-latin1-esc`, `s4-multi`; typed: `s4-multi-typed` |
| #375 OS-language repeat | needs the maintainer's agreement: procedure at the end of the checklist |

## Unverified prompt sequences

Every command sequence not confirmed without AutoCAD carries a `VERIFY:` comment: `grep -rn VERIFY tools/autocad`.
Confirmed against the script that saved the pinned ACadSharp `sample_base` twins under AutoCAD 2025 (an AutoCAD
command script with no DWG or DXF implementation, read as the CLEANROOM.md "Public corpus" entry records): the
SAVEAS format keywords `2018`, `2013`, `2010`, `2007`, `2004`, `2000` and `LT98` (R14), and the DXF form
`SAVEAS DXF V <version> 16 <file>` including `R12`. Not confirmed on the Mac 2027 trial: those same
sequences on this build, `-SCRIPT` and `TRUSTEDPATHS` as used in `00-start.scr`, `OPEN` of a drawing from a third-party writer (a recover or proxy prompt stops the
script), `*push-error-using-command*` and `*pop-error-mode*` in the builders' error handler (with `command-s` as
the fallback), `NEW` resolving the template and a script continuing across `NEW`, `OPEN` and `CLOSE`, and every
modelling and annotation sequence in the `.lsp` files.

## Clean-room

Written from Autodesk's public product documentation of AutoLISP and of the commands and system variables
used. No Autodesk SDK, ODA SDK, LibreDWG or other DWG implementation source was consulted, and nothing here
decodes or encodes DWG: the scripts only ask AutoCAD to save its own files. The scripts and builders are this
project's own work.
