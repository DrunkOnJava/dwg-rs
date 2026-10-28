# Manual drawings for the AutoCAD trial

Everything here is a drawing the AutoLISP builders cannot make, or a fallback for a builder that
printed `refused` or `no ... reachable`. AutoLISP and `.scr` run on AutoCAD for Mac; the Block Editor,
the Materials Browser and the Sun Properties palette are the parts that do not script. Menu and palette
names below follow Autodesk's public documentation and may differ in the Mac build: every one is a
`VERIFY` point, and the command-line form is given beside it where the command exists.

`<work>` is the folder `generate.py` wrote (default `~/Library/Caches/dwg-autocad`), `<out>` is
`<work>/out`.

## How to save a hand-built drawing

1. `NEW`, template `acadiso.dwt` (or the template the builders used), so every drawing starts alike.
2. Build the drawing from its section below.
3. Save the master in the newest format, under the item's exact name:
   `SAVEAS`, format `2018`, file `<out>/masters/<item>.dwg`. With `FILEDIA` at 1 the dialog opens; type
   the path into its name field, or run `FILEDIA` 0 first and type it at the command line.
4. When every master you want is saved, run `python3 -I tools/autocad/generate.py --work <work> --resume`, then in
   AutoCAD `-SCRIPT` with `<work>/scripts/save-all.scr`. It reopens each master and saves every release
   plus its ASCII DXF.
5. `python3 -I tools/autocad/check.py` lists what is still missing.

Vary one thing per drawing and change nothing else: the DXF twin is read against the DWG record by
record, so every field you leave at its default proves nothing about it.

## Any item whose builder failed

The builder prints `dwgrs:` lines in the command-line text, and saves a half-built drawing as
`<out>/masters/<item>.FAILED.dwg`. The `VERIFY` comment above the failing step in
`tools/autocad/lisp/*.lsp` names the prompt sequence it assumed. Run the same command by hand, note the
prompts that differ, fix the `.lsp` line, rerun the item's `items/build-<item>.scr`. A fix that holds is
worth committing back.

## s2-material

The builder tries `entmakex` for five MATERIAL objects (ambient, diffuse, specular, all three, and
current-colour methods). If it printed `entmakex refused MATERIAL`, AutoCAD for Mac does not allow that
object to be made from AutoLISP.

The Materials Browser and Material Editor (`MATBROWSEROPEN`, `MATEDITOROPEN`) expose Color, Glossiness,
Highlights, Reflectivity, Transparency and Self-Illumination, not an ambient or specular colour of their
own. So vary one control per material and let the DXF show which group moved:

| Material | Change from the default, nothing else |
| --- | --- |
| DWGRS_COLOR | Color 200,60,10 |
| DWGRS_GLOSS | Glossiness 80 |
| DWGRS_HILITE | Highlights switched Metal to Non-metal |
| DWGRS_SELFILL | Self-Illumination luminance 500 |
| DWGRS_REFL | Reflectivity direct 70 |

Create each with the Browser's new generic material button, then attach it to its own box
(`BOX` anywhere, `MATERIALATTACH` or drag from the Browser). Save as `s2-material`.

## s2-material-maps

MATERIAL map settings. The map image is `<work>/in/dwgrs-map.png`, a 64 by 64 colour checker.

| Material | Texture setting | Mapper on its box |
| --- | --- | --- |
| DWGRS_MAP_PLANAR | Color image dwgrs-map.png, scale 100 percent, tiling on | `MATERIALMAP` Planar |
| DWGRS_MAP_BOX | same image | `MATERIALMAP` Box |
| DWGRS_MAP_CYL | same image, tiling off | `MATERIALMAP` Cylindrical |
| DWGRS_MAP_SPH | same image, scale 50 percent, offset 10 | `MATERIALMAP` Spherical |
| DWGRS_MAP_FADE | same image, Image Fade 50 | `MATERIALMAP` Planar |

One box per material. Save as `s2-material-maps`.

## s2-dbcolor

If the builder printed `no ACAD_COLOR dictionary`, the book or colour names in `*dwgrs-colorbooks*` did not
match an installed colour book. Draw three lines. For each: select it, Properties palette, Color, Select
Color, Color Books tab, a book, a colour. Use three different books (for example RAL CLASSIC, DIC COLOR
GUIDE and PANTONE). At the command line, `-COLOR` then `COlorbook` lists the book and colour names; put
them into `*dwgrs-colorbooks*` and rerun the builder instead if you prefer. Save as `s2-dbcolor`.

## s2-pdf, s3-pdfunderlay-a and s3-pdfunderlay-b

The PDF is `<work>/in/dwgrs-two-page.pdf`: page 1 is Letter size, page 2 is A4, so the two definitions
differ. If `-PDFATTACH` or `PDFCLIP` prompted differently from the builder, do it with the dialogs.

- `s2-pdf`: `PDFATTACH`, the file, page 1, insertion 0,0, scale 1, rotation 0. Again with page 2 at
  300,0. Two PDFDEFINITION objects, one per page.
- `s3-pdfunderlay-a`: `UCS`, `X`, 90 first, so the normal is not 0,0,1. `PDFATTACH`, page 1, insertion
  10,5, scale 2.5, rotation 30. `PDFCLIP`, pick the underlay, New boundary, Polygonal, then five
  points in the UCS: 30,20; 200,10; 260,120; 150,220; 40,150; Enter. `UCS`, `W`.
- `s3-pdfunderlay-b`: `PDFATTACH`, page 1, 0,0, scale 1, rotation 0. `PDFCLIP`, New boundary,
  Rectangular, corners 20,20 and 120,90.

The DXF must show groups 41 to 43, 50, 210 to 230 and 11/21 on the PDFUNDERLAY of `s3-pdfunderlay-a`.

## s3-sun-off and s3-sun-on

The builder looks for a SUN object through the active viewport's extension dictionary and changes it with
`entmod`; most likely it printed `no SUN object reachable`. By hand:

1. `LIGHTINGUNITS` 2, `DEFAULTLIGHTING` 0, then `SUNPROPERTIES` to open the palette. If the Sun is only
   created once a sky or location is set, run `GEOGRAPHICLOCATION` first and set any location.
2. `s3-sun-off`: Status Off, Shadows Off, Daylight savings On (Sun Angle Calculator), Color a true colour
   that is not white (51,102,153), shadow type 1, shadow map size 512, softness 3.
3. `s3-sun-on`: Status On, Shadows On, Color 204,153,51, shadow type 2, map size 1024, softness 5,
   intensity factor 2.5.
4. A named colour-book colour is the second colour variant: set the Sun colour through the colour-book
   tab in a third drawing, `s3-sun-book`, and add `s3-sun-book` to the catalogue in `generate.py` if you
   want it checked.

Each saves as its own master. `SUN` exists on R2007 to R2013 and R2018, so the saves cover R2007, R2010,
R2013 and R2018.

## Dynamic blocks: s3-dyn-eg1, s3-dyn-eg2, s3-dyn-eg3, s2-dyn-polar, s2-dyn-vis3, s2-dyn-vis5, s2-dyn-scale-b

None of the Block Editor commands take their selections without a pick, so these are built by hand.
Every drawing starts the same way:

1. `NEW`, then `(load "<work>/lisp/dwgrs-lib.lsp")`, `(load "<work>/lisp/dwgrs-objects.lsp")`,
   `DWGRS-DYNBASE`. That inserts block `DYNBLK` at 0,0: a 40 by 20 rectangle with a circle of radius 4 at 20,10.
2. `BEDIT`, `DYNBLK`. The Block Authoring palettes open (`BAUTHORPALETTE` if not). Parameters are added
   with `BPARAMETER`, actions with `BACTIONTOOL`; type the points below at the prompts.
3. When done, `BCLOSE`, save the changes, and save the drawing as the master (above).

| Item | Parameters and actions |
| --- | --- |
| s3-dyn-eg1 | Linear P1: start 0,0, end 40,0, label 20,-5. Stretch on P1: parameter point the end, frame 30,-5 to 45,25, objects the rectangle. Scale on P1: object the circle |
| s3-dyn-eg2 | eg1, plus Linear P2: start 0,0, end 0,20, label -5,10. Stretch on P2: frame -5,15 to 45,25, objects the rectangle. Scale on P2: the circle |
| s3-dyn-eg3 | eg2, plus Linear P3: start -10,0, end -10,20, label -15,10. Stretch on P3: frame -5,-5 to 5,25, objects the rectangle. Scale on P3: the circle |
| s2-dyn-polar | Polar P1: base 0,0, end 40,0. Polar Stretch on P1: frame 30,-5 to 45,25, objects the rectangle. Rotate action on P1: the circle |
| s2-dyn-vis3 | Draw a second and third shape over the circle (a square 6 by 6, a triangle). Visibility parameter at -10,-10. `BVSTATE`: three states VIS1, VIS2, VIS3, one shape visible in each (`BVSHOW`, `BVHIDE`) |
| s2-dyn-vis5 | As vis3 with five states and five different shapes |
| s2-dyn-scale-b | Linear P1: 0,0 to 40,0. Scale on P1 for the whole block. In the Properties palette set the Scale action Base type to Independent, Base X 40, Base Y 20 |

In eg2 and eg3 also set the parameters' Number of grips to 2 and, for P1, move the grip to the Start point
(Properties palette), so the grip node ids differ between drawings. The
ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION value comes with each saved release; nothing here sets it.

## s3-datatable

No core command, and no command found in Autodesk's public documentation, produces a DATATABLE for a
drawing the user builds. The corpus's DATATABLE objects came from `sample_base` on R14 to R2004, and the
R2007 to R2018 saves of that drawing hold none. Two things to try, in order:

1. Open `acadsharp/sample_AC1014.dwg` from the corpus, `SAVEAS` 2007, 2010, 2013 and 2018 into
   `<out>/s3/`, and look for `DATATABLE` in the DXF (`check.py` counts it). If the object survives, it
   survived; note which release.
2. `DATAEXTRACTION` on a drawing with a few blocks and attributes, finished to a table, saved to R2007+.
   If a DATATABLE appears in the DXF, rebuild it with a string column, a point column and a named table.

If neither produces one, say so in the pull request that touches #335; the class needs another source.

## s2-dimassoc

If the DXF shows fewer DIMASSOC records than dimensions, an osnap override missed and the dimension is not
associative. By hand, with `DIMASSOC` 2 and running object snaps off, pick the endpoints with object snap
overrides (`END`, `MID`, `QUA`, `CEN`, `INT`, `NEA`). Fix a dimension that did not associate with
`DIMREASSOCIATE`. The angular dimension is `DIMANGULAR`, Enter, vertex, first endpoint, second endpoint.

## s3-xref-host

If `-XREF` prompted differently, attach `<out>/masters/s3-xref-src.dwg` from the External References
palette (`XATTACH` opens the dialog): Attachment, full path, insertion 0,0, scale 1, rotation 0. The host
must hold `s3-xref-src|XLAYER` and the other dependent entries; `s3-xref-src` itself is built and saved by
its builder.

## s4-multi-typed

The AutoLISP variant of this drawing writes Cyrillic, Greek and Japanese as `\U+XXXX` escapes. This one has
the characters typed, so AutoCAD holds real Unicode and decides how a pre-R2007 save stores them.

1. Add the input sources in System Settings, Keyboard, Input Sources (Russian, Greek, Japanese Hiragana),
   or paste the strings below. This changes keyboards only, not the macOS language.
2. `NEW`. `TEXT` at 0,0, height 2.5: `é ü ß ñ ° ± ©`. `TEXT` at 0,-5: `Привет мир`. At 0,-10:
   `Αθήνα`. At 0,-15: `日本語 こんにちは`.
3. `MTEXT` at 40,-10, width 80: the four strings above, one per line.
4. `BLOCK` named `BLK` containing a line and an attribute definition tag TAG1; insert it at 0,-25 with the
   value `é ü ß ñ ° ± © Привет Αθήνα 日本語`.
5. Create layers named `LAY_é ü ß ñ ° ± ©` and `Слой` (`LAYER`), a text style named `Στυλ` (`STYLE`),
   and rename the block `BLK` to `Блок` (`RENAME`).
6. Save as `s4-multi-typed`. Code points, for a keyboard that cannot type them: Cyrillic 041F 0440 0438 0432
   0435 0442; Greek 0391 03B8 03AE 03BD 03B1; Japanese 65E5 672C 8A9E and 3053 3093 306B 3061 306F.

Saves R2004, R2000 and R14 go through `save-all.scr` like every other item, and `check.py` prints each
DXF's `$DWGCODEPAGE`, its high-byte count and its `\U+` escape count.

### The locale repeat

The Russian and Japanese saves are a scripted run with AutoCAD's own language switched; the steps, with the
exact `defaults` commands, are in README.md ("Code-page saves under Russian and Japanese"). The typed
drawing above is not part of that run.
