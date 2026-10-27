;;; dwgrs-annotation.lsp: annotative TEXT for ACDB_TEXTOBJECTCONTEXTDATA_CLASS (#337) and the
;;; associative dimensions for DIMASSOC (#329). Load dwgrs-lib.lsp first. Entry points:
;;; DWGRS-S3-TEXTCTX and DWGRS-S2-DIMASSOC, each run in a fresh drawing.

(defun dwgrs:anno-style (name)
  (dwgrs:appid "AcadAnnotative")
  ;; VERIFY: the AcadAnnotative payload below is what AutoCAD writes for an annotative text style
  (dwgrs:mk (strcat "annotative STYLE " name)
    (list '(0 . "STYLE") '(100 . "AcDbSymbolTableRecord") '(100 . "AcDbTextStyleTableRecord")
          (cons 2 name) '(70 . 0) '(40 . 0.0) '(41 . 1.0) '(50 . 0.0) '(71 . 0) '(42 . 2.5)
          '(3 . "txt") '(4 . "")
          (list -3 (list "AcadAnnotative" '(1000 . "AnnotativeData") '(1002 . "{")
                         '(1070 . 1) '(1070 . 1) '(1002 . "}"))))))

(defun dwgrs:atext (style just pt h rot str)
  ;; VERIFY: TEXT prompts Style, style name, Justify, option, point, height, rotation, string
  (command "_.TEXT" "_S" style "_J" just pt h rot str)
  ;; VERIFY: TEXT may ask for a further line after the string; dwgrs:finish sends Enter only if it does
  (dwgrs:finish)
  (entlast))

(defun dwgrs:flag-text (style pt str flag / e)
  (setq e (dwgrs:atext style "_BL" pt 3.0 0.0 str))
  ;; VERIFY: entmod accepts group 71 appended to an annotative TEXT
  (dwgrs:set-group e 71 flag)
  e)

(defun dwgrs:add-scale (e scale)
  ;; VERIFY: -OBJECTSCALE prompts objects, Add, scale name, then Enter to finish
  (command "_.-OBJECTSCALE" e "" "_A" scale ""))

(defun dwgrs:build-s3-textctx (/ e1 e2 e3 e4)
  (setvar "OSMODE" 0)
  (dwgrs:layer "TXTCTX" 5)
  (dwgrs:anno-style "DWGRS_ANNO")
  (setvar "CANNOSCALE" "1:1")
  (setq e1 (dwgrs:atext "DWGRS_ANNO" "_MC" '(12.0 7.0 0.0) 3.5 30.0 "ANNO R30 MC"))
  (dwgrs:add-scale e1 "1:2")
  (setq e2 (dwgrs:atext "DWGRS_ANNO" "_TR" '(-9.5 22.25 0.0) 4.0 75.0 "ANNO R75 TR"))
  (dwgrs:add-scale e2 "1:5")
  (dwgrs:atext "DWGRS_ANNO" "_BL" '(31.0 -4.0 0.0) 3.0 30.0 "ANNO R30 OFFSET")
  (dwgrs:atext "DWGRS_ANNO" "_BL" '(31.0 -14.0 0.0) 3.0 75.0 "ANNO R75 OFFSET")
  (dwgrs:flag-text "DWGRS_ANNO" '(60.0 0.0 0.0) "ANNO BACKWARD" 2)
  (dwgrs:flag-text "DWGRS_ANNO" '(60.0 10.0 0.0) "ANNO UPSIDEDOWN" 4)
  (dwgrs:flag-text "DWGRS_ANNO" '(60.0 20.0 0.0) "ANNO BOTH" 6)
  (setq e3 (dwgrs:atext "DWGRS_ANNO" "_BL" '(90.0 5.0 0.0) 3.0 15.0 "ANNO MIRROR VERT"))
  (setvar "MIRRTEXT" 1)
  (command "_.MIRROR" e3 "" '(90.0 -10.0 0.0) '(90.0 30.0 0.0) "_N")
  (setq e4 (dwgrs:atext "DWGRS_ANNO" "_BL" '(110.0 5.0 0.0) 3.0 15.0 "ANNO MIRROR HORIZ"))
  (command "_.MIRROR" e4 "" '(100.0 5.0 0.0) '(130.0 5.0 0.0) "_N")
  (setvar "MIRRTEXT" 0)
  (princ))

(defun dwgrs:dim-lines (/ c)
  (command "_.LINE" '(0.0 0.0 0.0) '(20.0 0.0 0.0) "")
  (command "_.LINE" '(0.0 0.0 0.0) '(14.0 14.0 0.0) "")
  (command "_.LINE" '(5.0 -4.0 0.0) '(5.0 12.0 0.0) "")
  (command "_.CIRCLE" '(40.0 10.0 0.0) 8.0)
  (entlast))

(defun dwgrs:build-s2-dimassoc (/ c)
  (setvar "DIMASSOC" 2)
  (setvar "OSMODE" 0)
  (setq c (dwgrs:dim-lines))
  ;; VERIFY: each osnap override must snap to the intended object so the dimension is associative;
  ;; the DXF shows one DIMASSOC record per associative dimension, none when the snap missed
  (command "_.DIMLINEAR" "_END" '(0.0 0.0 0.0) "_END" '(20.0 0.0 0.0) '(10.0 -9.0 0.0))
  (command "_.DIMLINEAR" "_MID" '(10.0 0.0 0.0) "_END" '(14.0 14.0 0.0) '(24.0 8.0 0.0))
  (command "_.DIMLINEAR" "_QUA" '(32.0 10.0 0.0) "_QUA" '(48.0 10.0 0.0) '(40.0 24.0 0.0))
  (command "_.DIMLINEAR" "_CEN" '(48.0 10.0 0.0) "_END" '(20.0 0.0 0.0) '(34.0 -6.0 0.0))
  (command "_.DIMLINEAR" "_INT" '(5.0 0.0 0.0) "_END" '(14.0 14.0 0.0) '(-6.0 7.0 0.0))
  (command "_.DIMLINEAR" "_NEA" '(15.0 0.0 0.0) "_NEA" '(7.0 7.0 0.0) '(26.0 -14.0 0.0))
  (command "_.DIMDIAMETER" (list c '(48.0 10.0 0.0)) '(52.0 16.0 0.0))
  ;; VERIFY: DIMANGULAR with Enter selects the vertex form: vertex, first endpoint, second endpoint, arc location
  (command "_.DIMANGULAR" "" "_INT" '(0.0 0.0 0.0) "_END" '(20.0 0.0 0.0) "_END" '(14.0 14.0 0.0) '(9.0 4.0 0.0))
  (princ))

(defun c:dwgrs-s3-textctx ()
  (dwgrs:run "s3-textctx" 'dwgrs:build-s3-textctx))

(defun c:dwgrs-s2-dimassoc ()
  (dwgrs:run "s2-dimassoc" 'dwgrs:build-s2-dimassoc))
