;;; dwgrs-solids.lsp: 3D solids with history for the ACSH_* classes and the two PERSSUBENTMANAGER
;;; classes (sections 2 and 3 of the trial work list). Load dwgrs-lib.lsp first. Entry points: DWGRS-S2-ACSH-PRIMITIVES,
;;; DWGRS-S2-ACSH-EXTRUSION and DWGRS-S3-PERS-1 to DWGRS-S3-PERS-7, each run in a fresh drawing.

(defun dwgrs:solid-prep ()
  (setvar "SOLIDHIST" 1)
  (setvar "SHOWHIST" 1)
  (setvar "DELOBJ" 0)
  ;; VERIFY: -VIEW with a named orthographic or isometric view is a hidden option on newer releases
  (command "_.-VIEW" "_SWISO"))

(defun dwgrs:box (p l w h)
  ;; VERIFY: BOX prompts first corner, then Length option, then length, width, height
  (command "_.BOX" p "_L" l w h)
  (entlast))

(defun dwgrs:cylinder (c r h)
  ;; VERIFY: CYLINDER prompts centre, base radius, height
  (command "_.CYLINDER" c r h)
  (entlast))

(defun dwgrs:sphere (c r)
  (command "_.SPHERE" c r)
  (entlast))

(defun dwgrs:build-s2-acsh-primitives (/ e)
  (dwgrs:solid-prep)
  (dwgrs:box '(10.0 20.0 3.0) 12.0 7.0 5.0)
  (dwgrs:box '(-8.0 -8.0 0.0) 3.0 3.0 3.0)
  (dwgrs:cylinder '(-5.0 3.0 2.0) 4.5 11.0)
  (dwgrs:cylinder '(30.0 -12.0 0.0) 2.0 20.0)
  (dwgrs:sphere '(30.0 30.0 30.0) 6.0)
  (dwgrs:sphere '(-20.0 15.0 8.0) 2.5)
  (setq e (dwgrs:box '(50.0 0.0 0.0) 6.0 9.0 2.0))
  ;; VERIFY: CHPROP Color option, Truecolor sub-option, then the r,g,b string
  (command "_.CHPROP" e "" "_C" "_T" "200,60,10" "")
  (princ))

(defun dwgrs:build-s2-acsh-extrusion (/ c1 r2 c3 p3 e3 e1 e2)
  (dwgrs:solid-prep)
  (command "_.CIRCLE" '(0.0 0.0 0.0) 5.0)
  (setq c1 (entlast))
  (command "_.EXTRUDE" c1 "" 10.0)
  (setq e1 (entlast))
  (command "_.RECTANG" '(20.0 0.0 0.0) '(30.0 12.0 0.0))
  (setq r2 (entlast))
  ;; VERIFY: EXTRUDE prompts objects, then the Taper angle option, the angle, then the height
  (command "_.EXTRUDE" r2 "" "_T" 10.0 20.0)
  (setq e2 (entlast))
  (command "_.MOVE" e2 "" '(0.0 0.0 0.0) '(4.0 -3.0 2.0))
  (command "_.CIRCLE" '(50.0 0.0 0.0) 2.5)
  (setq c3 (entlast))
  (dwgrs:mk "ARC"
    (list '(0 . "ARC") '(100 . "AcDbEntity") '(100 . "AcDbCircle")
          '(10 65.0 0.0 0.0) '(40 . 15.0) '(100 . "AcDbArc")
          '(50 . 1.5707963267949) '(51 . 3.14159265358979) '(210 0.0 -1.0 0.0)))
  (setq p3 (entlast))
  ;; VERIFY: EXTRUDE Path option then the path object
  (command "_.EXTRUDE" c3 "" "_P" p3)
  (setq e3 (entlast))
  (command "_.ROTATE" e3 "" '(0.0 0.0 0.0) 30.0)
  (command "_.MOVE" e3 "" '(0.0 0.0 0.0) '(3.0 4.0 5.0))
  (princ))

(defun dwgrs:build-s3-pers-1 ()
  (princ))

(defun dwgrs:build-s3-pers-2 ()
  (dwgrs:solid-prep)
  (dwgrs:box '(0.0 0.0 0.0) 10.0 6.0 4.0)
  (princ))

(defun dwgrs:build-s3-pers-3 ()
  (dwgrs:solid-prep)
  (dwgrs:box '(0.0 0.0 0.0) 10.0 6.0 4.0)
  (dwgrs:box '(20.0 0.0 0.0) 8.0 8.0 8.0)
  (princ))

(defun dwgrs:build-s3-pers-4 ()
  (dwgrs:solid-prep)
  (dwgrs:box '(0.0 0.0 0.0) 10.0 6.0 4.0)
  ;; VERIFY: FILLETEDGE picks the top edge from (0,0,4) to (10,0,4) at its midpoint; accept with two Enters
  (command "_.FILLETEDGE" "_R" 1.0 '(5.0 0.0 4.0) "" "")
  (princ))

(defun dwgrs:build-s3-pers-5 (/ r)
  (dwgrs:solid-prep)
  (command "_.RECTANG" '(0.0 0.0 0.0) '(8.0 5.0 0.0))
  (setq r (entlast))
  (command "_.EXTRUDE" r "" 6.0)
  (princ))

(defun dwgrs:build-s3-pers-6 ()
  (dwgrs:solid-prep)
  (setvar "DIMASSOC" 2)
  (setvar "OSMODE" 0)
  (dwgrs:box '(0.0 0.0 0.0) 10.0 6.0 4.0)
  ;; VERIFY: the dimension should associate to the box edge endpoints; check DIMASSOC records in the DXF
  (command "_.DIMLINEAR" "_END" '(0.0 0.0 4.0) "_END" '(10.0 0.0 4.0) '(5.0 -4.0 4.0))
  (princ))

(defun dwgrs:build-s3-pers-7 ()
  (dwgrs:solid-prep)
  (dwgrs:box '(0.0 0.0 0.0) 10.0 6.0 4.0)
  (dwgrs:box '(20.0 0.0 0.0) 8.0 8.0 8.0)
  (command "_.ERASE" "_ALL" "")
  ;; VERIFY: -PURGE All, name *, no per-name confirmation
  (command "_.-PURGE" "_A" "*" "_N")
  (princ))

(defun c:dwgrs-s2-acsh-primitives ()
  (dwgrs:run "s2-acsh-primitives" 'dwgrs:build-s2-acsh-primitives))

(defun c:dwgrs-s2-acsh-extrusion ()
  (dwgrs:run "s2-acsh-extrusion" 'dwgrs:build-s2-acsh-extrusion))

(defun c:dwgrs-s3-pers-1 ()
  (dwgrs:run "s3-pers-1" 'dwgrs:build-s3-pers-1))

(defun c:dwgrs-s3-pers-2 ()
  (dwgrs:run "s3-pers-2" 'dwgrs:build-s3-pers-2))

(defun c:dwgrs-s3-pers-3 ()
  (dwgrs:run "s3-pers-3" 'dwgrs:build-s3-pers-3))

(defun c:dwgrs-s3-pers-4 ()
  (dwgrs:run "s3-pers-4" 'dwgrs:build-s3-pers-4))

(defun c:dwgrs-s3-pers-5 ()
  (dwgrs:run "s3-pers-5" 'dwgrs:build-s3-pers-5))

(defun c:dwgrs-s3-pers-6 ()
  (dwgrs:run "s3-pers-6" 'dwgrs:build-s3-pers-6))

(defun c:dwgrs-s3-pers-7 ()
  (dwgrs:run "s3-pers-7" 'dwgrs:build-s3-pers-7))
