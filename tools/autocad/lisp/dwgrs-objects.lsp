;;; dwgrs-objects.lsp: nongraphical objects and table records for sections 2 and 3 of the trial work list:
;;; MATERIAL, WIPEOUTVARIABLES, DBCOLOR, PDFDEFINITION and PDFUNDERLAY, SUN, saved UCS (#285),
;;; an xref-dependent table entry (#290), PLINEGEN and a polygon mesh (#358), and DWGRS-DYNBASE, the
;;; starting block for the dynamic-block drawings built by hand. Load dwgrs-lib.lsp first.
;;; Every builder reports what AutoCAD refused; the README lists the manual route for each.

(setq *dwgrs-colorbooks*
  '(("RAL CLASSIC$RAL 1000" . 13482632)
    ("DIC COLOR GUIDE(R)$DIC-1" . 14423100)))

(defun dwgrs:material (name groups / d mats)
  (setq d (entmakex (append (list '(0 . "MATERIAL") '(100 . "AcDbMaterial") (cons 1 name)) groups)))
  (if d
    (progn
      (setq mats (dictsearch (namedobjdict) "ACAD_MATERIAL"))
      (if mats
        (dictadd (cdr (assoc -1 mats)) name d)
        (dwgrs:say "no ACAD_MATERIAL dictionary")))
    (dwgrs:say (strcat "entmakex refused MATERIAL " name)))
  d)

(defun dwgrs:build-s2-material ()
  ;; VERIFY: entmakex may refuse MATERIAL; group meaning follows the DXF reference (70 ambient method,
  ;; 71 diffuse method, 76 specular method, 90/91/92 their colours, 40/41/45 their factors)
  (dwgrs:material "DWGRS_AMB" '((70 . 1) (40 . 0.25) (90 . 3368601)))
  (dwgrs:material "DWGRS_DIF" '((71 . 1) (41 . 0.5) (91 . 13408563)))
  (dwgrs:material "DWGRS_SPE" '((76 . 1) (45 . 0.75) (92 . 10079283)))
  (dwgrs:material "DWGRS_ALL" '((70 . 1) (40 . 0.3) (90 . 3368601) (71 . 1) (41 . 0.6) (91 . 13408563)
                                (76 . 1) (45 . 0.9) (92 . 10079283)))
  (dwgrs:material "DWGRS_CUR" '((70 . 0) (40 . 0.35) (71 . 0) (41 . 0.65) (76 . 0) (45 . 0.95)))
  (princ))

(defun dwgrs:build-wipeout (frame)
  ;; VERIFY: WIPEOUT prompts first point, next points, then Close
  (command "_.WIPEOUT" '(0.0 0.0 0.0) '(30.0 0.0 0.0) '(30.0 20.0 0.0) '(0.0 20.0 0.0) "_C")
  (setvar "WIPEOUTFRAME" frame)
  (princ))

(defun dwgrs:build-s2-wipeout-0 ()
  (dwgrs:build-wipeout 0))

(defun dwgrs:build-s2-wipeout-1 ()
  (dwgrs:build-wipeout 1))

(defun dwgrs:build-s2-wipeout-2 ()
  (dwgrs:build-wipeout 2))

(defun dwgrs:build-s2-dbcolor (/ i c)
  ;; VERIFY: the book and colour names must match an installed colour book; edit *dwgrs-colorbooks*
  (setq i 0)
  (foreach c *dwgrs-colorbooks*
    (dwgrs:mk (strcat "LINE in " (car c))
      (list '(0 . "LINE") (cons 10 (list 0.0 (* i 3.0) 0.0)) (cons 11 (list 20.0 (* i 3.0) 0.0))
            (cons 420 (cdr c)) (cons 430 (car c))))
    (setq i (1+ i)))
  (if (not (dictsearch (namedobjdict) "ACAD_COLOR"))
    (dwgrs:say "no ACAD_COLOR dictionary: the colour books were not found"))
  (princ))

(defun dwgrs:build-s2-pdf ()
  ;; VERIFY: -PDFATTACH prompts file, page number, insertion point, scale, rotation
  (command "_.-PDFATTACH" *dwgrs-pdf* 1 '(0.0 0.0 0.0) 1.0 0.0)
  (command "_.-PDFATTACH" *dwgrs-pdf* 2 '(300.0 0.0 0.0) 1.0 0.0)
  (princ))

(defun dwgrs:build-s3-pdfunderlay-a (/ e)
  (command "_.UCS" "_X" 90.0)
  (command "_.-PDFATTACH" *dwgrs-pdf* 1 '(10.0 5.0 0.0) 2.5 30.0)
  (setq e (entlast))
  ;; VERIFY: PDFCLIP prompts object, New boundary, Polygonal, points, then Enter
  (command "_.PDFCLIP" e "_N" "_P" '(30.0 20.0 0.0) '(200.0 10.0 0.0) '(260.0 120.0 0.0)
           '(150.0 220.0 0.0) '(40.0 150.0 0.0) "")
  (command "_.UCS" "_W")
  (princ))

(defun dwgrs:build-s3-pdfunderlay-b (/ e)
  (command "_.-PDFATTACH" *dwgrs-pdf* 1 '(0.0 0.0 0.0) 1.0 0.0)
  (setq e (entlast))
  (command "_.PDFCLIP" e "_N" "_R" '(20.0 20.0 0.0) '(120.0 90.0 0.0))
  (princ))

(defun dwgrs:find-sun (/ vp xd g found)
  (setq vp (tblobjname "VPORT" "*Active"))
  (if vp
    (progn
      (setq found (cdr (assoc 361 (entget vp))))
      (if (not found)
        (progn
          (setq xd (cdr (assoc 360 (entget vp))))
          (if xd
            (foreach g (entget xd)
              (if (= (car g) 3)
                (progn
                  (dwgrs:say (strcat "extension dictionary key " (cdr g)))
                  (if (wcmatch (strcase (cdr g)) "*SUN*")
                    (setq found (cdr (assoc 350 (member g (entget xd))))))))))))))
  found)

(defun dwgrs:build-sun (groups / sun g ok)
  (setvar "LIGHTINGUNITS" 2)
  ;; VERIFY: SUNSTATUS exists and creates the SUN object; the object is group 361 of the active VPORT record
  (if (getvar "SUNSTATUS") (setvar "SUNSTATUS" 1))
  ;; VERIFY: where the SUN object sits and whether entmod may change it; the palette is the fallback
  (setq sun (dwgrs:find-sun))
  (if sun
    (foreach g groups
      (setq ok (dwgrs:set-group sun (car g) (cdr g)))
      (dwgrs:say (strcat "SUN group " (itoa (car g)) (if ok " set" " refused"))))
    (dwgrs:say "no SUN object reachable: use the Sun Properties steps in MANUAL-CHECKLIST.md"))
  (princ))

(defun dwgrs:build-s3-sun-off ()
  (dwgrs:build-sun '((290 . 0) (292 . 1) (421 . 3368601) (291 . 0) (70 . 1) (71 . 512) (280 . 3))))

(defun dwgrs:build-s3-sun-on ()
  (dwgrs:build-sun '((290 . 1) (292 . 0) (421 . 13408563) (291 . 1) (70 . 2) (71 . 1024) (280 . 5) (40 . 2.5))))

(defun dwgrs:build-s3-ucs ()
  ;; VERIFY: entmake accepts UCS records without groups 79, 146, 13, 346 and 71
  (dwgrs:mk "UCS DWGRS_UCS_A"
    '((0 . "UCS") (100 . "AcDbSymbolTableRecord") (100 . "AcDbUCSTableRecord")
      (2 . "DWGRS_UCS_A") (70 . 0) (10 7.0 3.0 2.5) (11 0.8660254 0.5 0.0) (12 -0.5 0.8660254 0.0)))
  (dwgrs:mk "UCS DWGRS_UCS_B"
    '((0 . "UCS") (100 . "AcDbSymbolTableRecord") (100 . "AcDbUCSTableRecord")
      (2 . "DWGRS_UCS_B") (70 . 0) (10 -4.0 1.5 6.0) (11 1.0 0.0 0.0) (12 0.0 0.0 1.0)))
  ;; VERIFY: _S and _R are legacy top-level UCS options on newer releases; _NA then _S / _R is the alternative
  (command "_.UCS" "_X" 90.0)
  (command "_.UCS" "_S" "DWGRS_UCS_C")
  (command "_.UCS" "_W")
  (command "_.UCS" "_R" "DWGRS_UCS_A")
  (command "_.LINE" '(0.0 0.0 0.0) '(10.0 0.0 0.0) "")
  (princ))

(defun dwgrs:build-s3-xref-src ()
  (dwgrs:layer "XLAYER" 1)
  (dwgrs:ltype-dash "XLT")
  (dwgrs:style "XSTYLE" "txt")
  (dwgrs:mk "LINE" '((0 . "LINE") (8 . "XLAYER") (6 . "XLT") (10 0.0 0.0 0.0) (11 30.0 10.0 0.0)))
  (dwgrs:text '(0.0 -5.0 0.0) 2.5 "XREF SOURCE" "XLAYER" "XSTYLE")
  (dwgrs:mk "BLOCK" '((0 . "BLOCK") (8 . "0") (2 . "XBLK") (70 . 0) (10 0.0 0.0 0.0)))
  (dwgrs:mk "LINE" '((0 . "LINE") (8 . "0") (10 0.0 0.0 0.0) (11 5.0 5.0 0.0)))
  (dwgrs:mk "ENDBLK" '((0 . "ENDBLK")))
  (dwgrs:mk "INSERT" '((0 . "INSERT") (8 . "XLAYER") (2 . "XBLK") (10 40.0 0.0 0.0)))
  (princ))

(defun dwgrs:build-s3-xref-host ()
  ;; VERIFY: -XREF prompts Attach, file, insertion point, X scale, Y scale, rotation
  (command "_.-XREF" "_A" (dwgrs:path "masters/s3-xref-src.dwg") '(0.0 0.0 0.0) 1.0 1.0 0.0)
  (princ))

(defun dwgrs:build-s3-plinegen-mesh (/ i j pts)
  (dwgrs:ltype-dash "DWGRS_DASH")
  (dwgrs:mk "LWPOLYLINE plinegen on"
    (append '((0 . "LWPOLYLINE") (100 . "AcDbEntity") (8 . "0") (6 . "DWGRS_DASH")
              (100 . "AcDbPolyline") (90 . 4) (70 . 128) (43 . 0.0))
            (list (cons 10 '(0.0 0.0)) (cons 10 '(10.0 0.0)) (cons 10 '(10.0 6.0)) (cons 10 '(14.0 9.0)))))
  (dwgrs:mk "LWPOLYLINE plinegen on closed"
    (append '((0 . "LWPOLYLINE") (100 . "AcDbEntity") (8 . "0") (6 . "DWGRS_DASH")
              (100 . "AcDbPolyline") (90 . 4) (70 . 129) (43 . 0.0))
            (list (cons 10 '(0.0 20.0)) (cons 10 '(10.0 20.0)) (cons 10 '(10.0 26.0)) (cons 10 '(14.0 29.0)))))
  (dwgrs:mk "LWPOLYLINE plinegen off"
    (append '((0 . "LWPOLYLINE") (100 . "AcDbEntity") (8 . "0") (6 . "DWGRS_DASH")
              (100 . "AcDbPolyline") (90 . 4) (70 . 0) (43 . 0.0))
            (list (cons 10 '(0.0 40.0)) (cons 10 '(10.0 40.0)) (cons 10 '(10.0 46.0)) (cons 10 '(14.0 49.0)))))
  (dwgrs:mk "POLYLINE mesh"
    '((0 . "POLYLINE") (100 . "AcDbEntity") (100 . "AcDb3dPolygonMesh") (66 . 1) (10 0.0 0.0 0.0)
      (70 . 16) (71 . 3) (72 . 3) (73 . 0) (74 . 0) (75 . 0)))
  (setq i 0)
  (repeat 3
    (setq j 0)
    (repeat 3
      (dwgrs:mk "VERTEX"
        (list '(0 . "VERTEX") '(100 . "AcDbEntity") '(100 . "AcDbVertex") '(100 . "AcDb3dPolygonMeshVertex")
              (cons 10 (list (+ 30.0 (* i 5.0)) (* j 5.0) (if (= (rem (+ i j) 2) 0) 0.0 2.0)))
              '(70 . 64)))
      (setq j (1+ j)))
    (setq i (1+ i)))
  (dwgrs:mk "SEQEND" '((0 . "SEQEND")))
  (princ))

(defun c:dwgrs-dynbase ()
  (dwgrs:mk "BLOCK" '((0 . "BLOCK") (8 . "0") (2 . "DYNBLK") (70 . 0) (10 0.0 0.0 0.0)))
  (dwgrs:mk "LWPOLYLINE rectangle"
    (append '((0 . "LWPOLYLINE") (100 . "AcDbEntity") (8 . "0") (100 . "AcDbPolyline") (90 . 4) (70 . 1) (43 . 0.0))
            (list (cons 10 '(0.0 0.0)) (cons 10 '(40.0 0.0)) (cons 10 '(40.0 20.0)) (cons 10 '(0.0 20.0)))))
  (dwgrs:mk "CIRCLE" '((0 . "CIRCLE") (8 . "0") (10 20.0 10.0 0.0) (40 . 4.0)))
  (dwgrs:mk "ENDBLK" '((0 . "ENDBLK")))
  (dwgrs:mk "INSERT" '((0 . "INSERT") (8 . "0") (2 . "DYNBLK") (10 0.0 0.0 0.0)))
  (command "_.ZOOM" "_E")
  (dwgrs:say "block DYNBLK inserted at 0,0: rectangle 40 by 20 with a circle; now BEDIT DYNBLK")
  (princ))

(defun c:dwgrs-s2-material ()
  (dwgrs:run "s2-material" 'dwgrs:build-s2-material))

(defun c:dwgrs-s2-wipeout-0 ()
  (dwgrs:run "s2-wipeout-0" 'dwgrs:build-s2-wipeout-0))

(defun c:dwgrs-s2-wipeout-1 ()
  (dwgrs:run "s2-wipeout-1" 'dwgrs:build-s2-wipeout-1))

(defun c:dwgrs-s2-wipeout-2 ()
  (dwgrs:run "s2-wipeout-2" 'dwgrs:build-s2-wipeout-2))

(defun c:dwgrs-s2-dbcolor ()
  (dwgrs:run "s2-dbcolor" 'dwgrs:build-s2-dbcolor))

(defun c:dwgrs-s2-pdf ()
  (dwgrs:run "s2-pdf" 'dwgrs:build-s2-pdf))

(defun c:dwgrs-s3-pdfunderlay-a ()
  (dwgrs:run "s3-pdfunderlay-a" 'dwgrs:build-s3-pdfunderlay-a))

(defun c:dwgrs-s3-pdfunderlay-b ()
  (dwgrs:run "s3-pdfunderlay-b" 'dwgrs:build-s3-pdfunderlay-b))

(defun c:dwgrs-s3-sun-off ()
  (dwgrs:run "s3-sun-off" 'dwgrs:build-s3-sun-off))

(defun c:dwgrs-s3-sun-on ()
  (dwgrs:run "s3-sun-on" 'dwgrs:build-s3-sun-on))

(defun c:dwgrs-s3-ucs ()
  (dwgrs:run "s3-ucs" 'dwgrs:build-s3-ucs))

(defun c:dwgrs-s3-xref-src ()
  (dwgrs:run "s3-xref-src" 'dwgrs:build-s3-xref-src))

(defun c:dwgrs-s3-xref-host ()
  (dwgrs:run "s3-xref-host" 'dwgrs:build-s3-xref-host))

(defun c:dwgrs-s3-plinegen-mesh ()
  (dwgrs:run "s3-plinegen-mesh" 'dwgrs:build-s3-plinegen-mesh))
