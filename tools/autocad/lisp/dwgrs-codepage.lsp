;;; dwgrs-codepage.lsp: the section 4 text drawings (#375): accented and symbol text in TEXT, MTEXT,
;;; an attribute value, a layer name, a block name and a text style name; a variant with Cyrillic,
;;; Greek and Japanese added. Load dwgrs-lib.lsp first. Entry points: DWGRS-S4-LATIN1,
;;; DWGRS-S4-LATIN1-ESC and DWGRS-S4-MULTI, each run in a fresh drawing; each saves its master.

(defun dwgrs:s4-glyphs ()
  (list 233 " " 252 " " 223 " " 241 " " 176 " " 177 " " 169))

(defun dwgrs:s4-insert (layer style val pt)
  (dwgrs:mk "INSERT"
    (list '(0 . "INSERT") '(100 . "AcDbEntity") (cons 8 layer) '(100 . "AcDbBlockReference")
          '(66 . 1) (cons 2 *dwgrs-s4-block*) (cons 10 pt)))
  (dwgrs:mk "ATTRIB"
    (list '(0 . "ATTRIB") '(100 . "AcDbEntity") (cons 8 layer) '(100 . "AcDbText")
          (cons 10 pt) '(40 . 2.5) (cons 1 val) (cons 7 style)
          '(100 . "AcDbAttribute") '(2 . "TAG1") '(70 . 0)))
  (dwgrs:mk "SEQEND" '((0 . "SEQEND"))))

(defun dwgrs:s4-block (layer style default)
  (dwgrs:mk "BLOCK"
    (list '(0 . "BLOCK") '(100 . "AcDbEntity") '(8 . "0") '(100 . "AcDbBlockBegin")
          (cons 2 *dwgrs-s4-block*) '(70 . 2) '(10 0.0 0.0 0.0)))
  (dwgrs:mk "LINE"
    '((0 . "LINE") (8 . "0") (10 0.0 0.0 0.0) (11 12.0 0.0 0.0)))
  (dwgrs:mk "ATTDEF"
    (list '(0 . "ATTDEF") '(100 . "AcDbEntity") '(8 . "0") '(100 . "AcDbText")
          '(10 0.0 3.0 0.0) '(40 . 2.5) (cons 1 default) '(50 . 0.0) '(41 . 1.0) '(51 . 0.0)
          (cons 7 style) '(71 . 0) '(72 . 0) '(11 0.0 0.0 0.0) '(210 0.0 0.0 1.0)
          '(100 . "AcDbAttributeDefinition") '(3 . "Prompt") '(2 . "TAG1") '(70 . 0) '(73 . 0) '(74 . 0)))
  (dwgrs:mk "ENDBLK" '((0 . "ENDBLK"))))

(defun dwgrs:build-s4 (extra / nl ns g)
  (setq g (dwgrs:s4-glyphs)
        nl (dwgrs:name (cons "LAY_" (dwgrs:s4-glyphs)) "LAY_ESC")
        ns (dwgrs:name (cons "STY_" (dwgrs:s4-glyphs)) "STY_ESC")
        *dwgrs-s4-block* (dwgrs:name (cons "BLK_" (dwgrs:s4-glyphs)) "BLK_ESC"))
  (dwgrs:layer nl 3)
  (dwgrs:style ns "txt")
  (dwgrs:s4-block nl ns (dwgrs:ustr (cons "ATTRDEF " g)))
  (dwgrs:text '(0.0 0.0 0.0) 2.5 (dwgrs:ustr (cons "TEXT " g)) nl ns)
  (dwgrs:mtext '(0.0 -10.0 0.0) 2.5 80.0
               (strcat (dwgrs:ustr (cons "MTEXT " g)) "\\P" (dwgrs:ustr (cons "line two " g)))
               nl ns)
  (dwgrs:s4-insert nl ns (dwgrs:ustr (cons "ATTRIB " g)) '(0.0 -25.0 0.0))
  (if extra (dwgrs:s4-extra nl ns))
  (princ))

(defun dwgrs:s4-extra (nl ns / cyr gre jpa jpb)
  (setq cyr (list 1055 1088 1080 1074 1077 1090)
        gre (list 913 952 942 957 945)
        jpa (list 26085 26412 35486)
        jpb (list 12371 12435 12395 12385 12399))
  (dwgrs:text '(0.0 -40.0 0.0) 2.5 (dwgrs:ustr (cons "Cyrillic " cyr)) nl ns)
  (dwgrs:text '(0.0 -45.0 0.0) 2.5 (dwgrs:ustr (cons "Greek " gre)) nl ns)
  (dwgrs:text '(0.0 -50.0 0.0) 2.5 (dwgrs:ustr (cons "Japanese " (append jpa (list " ") jpb))) nl ns)
  (dwgrs:mtext '(40.0 -10.0 0.0) 2.5 80.0
               (strcat (dwgrs:ustr (cons "Cyrillic " cyr)) "\\P"
                       (dwgrs:ustr (cons "Greek " gre)) "\\P"
                       (dwgrs:ustr (cons "Japanese " (append jpa (list " ") jpb))))
               nl ns)
  (dwgrs:s4-insert nl ns
    (dwgrs:ustr (append (list "ATTRIB ") cyr (list " ") gre (list " ") jpa))
    '(40.0 -25.0 0.0))
  (princ))

(defun dwgrs:build-s4-latin1 ()
  (dwgrs:build-s4 nil))

(defun dwgrs:build-s4-latin1-esc ( / old)
  (setq old *dwgrs-char-mode* *dwgrs-char-mode* "escape")
  (dwgrs:build-s4 nil)
  (setq *dwgrs-char-mode* old))

(defun dwgrs:build-s4-multi ()
  (dwgrs:build-s4 T))

(defun c:dwgrs-s4-latin1 ()
  (dwgrs:run "s4-latin1" 'dwgrs:build-s4-latin1))

(defun c:dwgrs-s4-latin1-esc ()
  (dwgrs:run "s4-latin1-esc" 'dwgrs:build-s4-latin1-esc))

(defun c:dwgrs-s4-multi ()
  (dwgrs:run "s4-multi" 'dwgrs:build-s4-multi))
