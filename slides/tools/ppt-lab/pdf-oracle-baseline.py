"""Glyph placement of the PowerPoint-PDF oracle by font (pdf-oracle.mjs; the work folder of pdf-oracle-compare.py): every PowerPoint
glyph matched to the nearest Rust (system fonts) glyph of the same character within 0.6 em, the offsets (Rust - PowerPoint, pt; y up)
grouped by the PowerPoint font name, and dy / font size, which says whether the vertical offset is a line-metric rule (it scales with
the size) or a constant.

usage: python -I pdf-oracle-baseline.py <work dir> <tests/oracle_ppt dir>   -> <oracle dir>/results/baseline.tsv
"""
import ctypes
import os
import sys
from collections import defaultdict

import numpy as np
import pypdfium2 as pdfium
import pypdfium2.raw as raw


def glyphs(page):
    tp = page.get_textpage()
    out = []
    buf = ctypes.create_string_buffer(256)
    flags = ctypes.c_int(0)
    x, y = ctypes.c_double(0), ctypes.c_double(0)
    for i in range(tp.count_chars()):
        cp = raw.FPDFText_GetUnicode(tp.raw, i)
        if cp in (0, 0xFFFE, 0xFFFF) or chr(cp).isspace() or raw.FPDFText_IsGenerated(tp.raw, i) == 1:
            continue
        raw.FPDFText_GetCharOrigin(tp.raw, i, ctypes.byref(x), ctypes.byref(y))
        ln = raw.FPDFText_GetFontInfo(tp.raw, i, buf, 256, ctypes.byref(flags))
        name = buf.raw[:max(0, ln - 1)].decode('latin-1', 'replace')
        name = name.split('+', 1)[1] if len(name) > 7 and name[6] == '+' else name
        out.append((chr(cp), x.value, y.value, raw.FPDFText_GetFontSize(tp.raw, i), name))
    tp.close()
    return out


def main():
    work, oracle = sys.argv[1], sys.argv[2]
    ids = [l.split('\t')[0] for l in open(os.path.join(oracle, 'decks.tsv'), encoding='utf-8') if l.strip() and not l.startswith('#')]
    by_font = defaultdict(list)
    for did in ids:
        p, r = os.path.join(work, 'ppt', f'{did}.pdf'), os.path.join(work, 'rust-system', f'{did}.pdf')
        if not (os.path.exists(p) and os.path.exists(r)):
            continue
        P, R = pdfium.PdfDocument(p), pdfium.PdfDocument(r)
        for i in range(min(len(P), len(R))):
            a, b = glyphs(P[i]), glyphs(R[i])
            idx = defaultdict(list)
            for c in b:
                idx[c[0]].append(c)
            arr = {k: np.array([(c[1], c[2]) for c in v]) for k, v in idx.items()}
            for c in a:
                v = arr.get(c[0])
                if v is None or c[3] <= 0:
                    continue
                d = np.hypot(v[:, 0] - c[1], v[:, 1] - c[2])
                k = int(np.argmin(d))
                if d[k] <= 0.6 * c[3]:
                    by_font[c[4]].append((v[k, 0] - c[1], v[k, 1] - c[2], c[3]))
        P.close()
        R.close()
    rows = []
    for f, v in by_font.items():
        if len(v) < 20:
            continue
        a = np.array(v)
        rows.append((len(v), f, np.median(a[:, 0]), np.median(a[:, 1]), np.median(a[:, 1] / a[:, 2]), np.percentile(a[:, 1] / a[:, 2], 10),
                     np.percentile(a[:, 1] / a[:, 2], 90), float((np.hypot(a[:, 0], a[:, 1]) <= 1).mean())))
    rows.sort(reverse=True)
    with open(os.path.join(oracle, 'results', 'baseline.tsv'), 'w', encoding='utf-8', newline='\n') as f:
        f.write('# glyphs matched\tPowerPoint font\tmedian dx pt\tmedian dy pt\tmedian dy/size (em)\tp10 dy/size\tp90 dy/size\twithin 1 pt\n')
        for r in rows:
            f.write(f'{r[0]}\t{r[1]}\t{r[2]:.3f}\t{r[3]:.3f}\t{r[4]:.4f}\t{r[5]:.4f}\t{r[6]:.4f}\t{r[7]:.3f}\n')
    for r in rows[:25]:
        print(r[0], r[1], f'dx {r[2]:.2f} dy {r[3]:.2f} dy/em {r[4]:.4f} [{r[5]:.4f} {r[6]:.4f}] <=1pt {r[7]:.2f}')


if __name__ == '__main__':
    main()
