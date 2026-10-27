;;; dwgrs-lib.lsp: shared helpers for the dwg-rs AutoCAD trial builders (tools/autocad/README.md).
;;; Core AutoLISP only: no vl-, vla- or vlax- function, so it loads on AutoCAD for Mac.
;;; Commands carry the _. prefix and keywords the _ prefix, so a localized session still runs.
;;; Written from Autodesk's public AutoLISP and command documentation; no SDK or DWG library source.

(if (not *dwgrs-char-mode*) (setq *dwgrs-char-mode* "chr"))

(defun dwgrs:say (s)
  (princ (strcat "\n;; dwgrs: " s))
  (princ))

(defun dwgrs:path (rel)
  (if (not *dwgrs-out*) (error "dwgrs: *dwgrs-out* is not set"))
  (strcat *dwgrs-out* "/" rel))

(defun dwgrs:str (v)
  (cond ((null v) "n/a")
        ((= (type v) 'STR) v)
        ((= (type v) 'INT) (itoa v))
        ((= (type v) 'REAL) (rtos v 2 8))
        (T "?")))

(defun dwgrs:hex4 (n / d s)
  (setq d "0123456789ABCDEF" s "")
  (repeat 4
    (setq s (strcat (substr d (1+ (rem n 16)) 1) s)
          n (/ n 16)))
  s)

(defun dwgrs:u (cp)
  (if (and (< cp 256) (= *dwgrs-char-mode* "chr"))
    (chr cp)
    (strcat "\\U+" (dwgrs:hex4 cp))))

(defun dwgrs:ustr (parts / out p)
  (setq out "")
  (foreach p parts
    (setq out (strcat out (if (= (type p) 'INT) (dwgrs:u p) p))))
  out)

(defun dwgrs:name (parts fallback)
  (if (= *dwgrs-char-mode* "chr") (dwgrs:ustr parts) fallback))

(defun dwgrs:codes (s / i out)
  (setq i 0 out "")
  (repeat (strlen s)
    (setq i (1+ i)
          out (strcat out (itoa (ascii (substr s i 1))) " ")))
  out)

(defun dwgrs:mk (what lst / r)
  (setq r (entmake lst))
  (if (not r) (dwgrs:say (strcat "entmake refused: " what)))
  r)

(defun dwgrs:appid (name)
  (if (not (tblsearch "APPID" name)) (regapp name)))

(defun dwgrs:layer (name aci)
  (dwgrs:mk (strcat "LAYER " name)
    (list '(0 . "LAYER") '(100 . "AcDbSymbolTableRecord") '(100 . "AcDbLayerTableRecord")
          (cons 2 name) '(70 . 0) (cons 62 aci) '(6 . "Continuous"))))

(defun dwgrs:style (name font)
  (dwgrs:mk (strcat "STYLE " name)
    (list '(0 . "STYLE") '(100 . "AcDbSymbolTableRecord") '(100 . "AcDbTextStyleTableRecord")
          (cons 2 name) '(70 . 0) '(40 . 0.0) '(41 . 1.0) '(50 . 0.0) '(71 . 0) '(42 . 2.5)
          (cons 3 font) '(4 . ""))))

(defun dwgrs:ltype-dash (name)
  (dwgrs:mk (strcat "LTYPE " name)
    (list '(0 . "LTYPE") '(100 . "AcDbSymbolTableRecord") '(100 . "AcDbLinetypeTableRecord")
          (cons 2 name) '(70 . 0) '(3 . "Dash") '(72 . 65) '(73 . 2) '(40 . 1.5)
          '(49 . 1.0) '(74 . 0) '(49 . -0.5) '(74 . 0))))

(defun dwgrs:text (pt h str layer style)
  (dwgrs:mk "TEXT"
    (list '(0 . "TEXT") (cons 8 layer) (cons 10 pt) (cons 40 h) (cons 1 str) (cons 7 style))))

(defun dwgrs:mtext (pt h w str layer style)
  (dwgrs:mk "MTEXT"
    (list '(0 . "MTEXT") '(100 . "AcDbEntity") (cons 8 layer) '(100 . "AcDbMText")
          (cons 10 pt) (cons 40 h) (cons 41 w) '(71 . 1) '(72 . 5) (cons 1 str) (cons 7 style))))

(defun dwgrs:finish (/ n)
  (setq n 0)
  (while (and (> (getvar "CMDACTIVE") 0) (< n 3))
    (command "")
    (setq n (1+ n))))

(defun dwgrs:set-group (e code val / g old)
  (setq g (entget e) old (assoc code g))
  (entmod (if old (subst (cons code val) old g) (append g (list (cons code val))))))

(defun dwgrs:save-as (ver file / ex)
  ;; VERIFY: EXPERT 2 suppresses the replace-existing-file prompt of SAVEAS
  (setq ex (getvar "EXPERT"))
  (setvar "EXPERT" 2)
  (command "_.SAVEAS" ver file)
  (setvar "EXPERT" ex))

(defun dwgrs:save-master (item / f)
  (setq f (dwgrs:path (strcat "masters/" item ".dwg")))
  (dwgrs:save-as "2018" f)
  (dwgrs:say (strcat "saved master " f)))

(defun dwgrs:run (item builder / *error*)
  (setq *error*
    (lambda (m / n)
      (princ (strcat "\n;; dwgrs: " item " failed: " (if m m "?")))
      (setq n 0)
      (while (and (> (getvar "CMDACTIVE") 0) (< n 10))
        (command)
        (setq n (1+ n)))
      (dwgrs:save-as "2018" (dwgrs:path (strcat "masters/" item ".FAILED.dwg")))
      (princ)))
  (apply builder '())
  (command "_.ZOOM" "_E")
  (dwgrs:save-master item)
  (princ))

(defun dwgrs:info (/ f v)
  (setq f (open (dwgrs:path "session-info.txt") "w"))
  (if f
    (progn
      (foreach v '("_VERNUM" "ACADVER" "PRODUCT" "RELEASE" "PLATFORM" "LOCALE"
                   "DWGCODEPAGE" "SYSCODEPAGE" "MEASUREMENT" "INSUNITS" "DWGNAME" "DWGPREFIX")
        (write-line (strcat v "=" (dwgrs:str (getvar v))) f)
        (dwgrs:say (strcat v "=" (dwgrs:str (getvar v)))))
      (close f))
    (dwgrs:say "cannot write session-info.txt: does the output folder exist?"))
  (princ))

(defun c:dwgrs-info ()
  (dwgrs:info))

(defun c:dwgrs-chartest (/ mode s e str f)
  (setq f (open (dwgrs:path "chartest.txt") "w"))
  (foreach mode '("chr" "escape")
    (setq *dwgrs-char-mode* mode
          s (strcat "t" (dwgrs:u 233) "u" (dwgrs:u 1046)))
    (entmake (list '(0 . "TEXT") '(10 0.0 0.0 0.0) '(40 . 2.5) (cons 1 s)))
    (setq e (entlast)
          str (cdr (assoc 1 (entget e))))
    (dwgrs:say (strcat "mode " mode " stored codes: " (dwgrs:codes str)))
    (if f (write-line (strcat mode ": " str) f))
    (entdel e))
  (if f (close f))
  (setq *dwgrs-char-mode* "chr")
  (dwgrs:say "chr mode works when its list is 116 233 117 or 116 195 169 117, then 92 85 43 48 52 49 54; a 63 in place of the accent means it failed: rerun the generator with --char-mode escape")
  (princ))
