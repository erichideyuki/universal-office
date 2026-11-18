"""The comparison of the PowerPoint-PDF oracle (pdf-oracle.mjs compare): PowerPoint's PDF against the Rust Slides PDF (two font
policies) and the frozen TS viewer's canvas, per page. Everything is read through pdfium (pypdfium2), the same way for both PDFs.

usage: python -I pdf-oracle-compare.py <work dir> <tests/oracle_ppt dir>

Per page:
  - page count and size;
  - pixels at 96 dpi on white: a pixel DIFFERS when no pixel of the other image within 1 px (3x3) is within TOL on every channel,
    both ways (antialiasing, hinting and sub-pixel placement do not count); agreement = 1 - differing / all;
  - per differing pixel of Rust x PowerPoint, the TS canvas decides who it sides with: TS legacy (Rust = TS != PowerPoint),
    PDF only (Rust != TS = PowerPoint), both (all three differ);
  - the topmost page object of each PDF under the pixel (text, path, image, shadow = a 1 x 1 image with a soft mask, shading, form);
  - text: the characters in content order (whitespace dropped), similarity, glyph origins of matched characters (pt), font names;
  - link annotations (count, URI, rectangle overlap).
Writes <oracle dir>/results/{pages.tsv,decks.tsv,summary.json,ppt-pdfs.tsv,features.tsv} and <work dir>/diff/*.png (not committed).
"""
import ctypes
import difflib
import hashlib
import json
import os
import sys
from collections import Counter, defaultdict

import numpy as np
import pypdfium2 as pdfium
import pypdfium2.raw as raw
from PIL import Image

TOL = 40
DPI = 96
SCALE = DPI / 72.0
CLASSES = {1: 'text', 2: 'path', 3: 'image', 4: 'shading', 5: 'form'}


def render(page):
    img = page.render(scale=SCALE, fill_color=(255, 255, 255, 255)).to_pil().convert('RGB')
    return np.asarray(img, dtype=np.int16)


def load_png(path, shape):
    im = Image.open(path).convert('RGBA')
    bg = Image.new('RGBA', im.size, (255, 255, 255, 255))
    a = np.asarray(Image.alpha_composite(bg, im).convert('RGB'), dtype=np.int16)
    return fit(a, shape)


def fit(a, shape):
    h, w = shape[:2]
    out = np.full((h, w, 3), 255, dtype=np.int16)
    hh, ww = min(h, a.shape[0]), min(w, a.shape[1])
    out[:hh, :ww] = a[:hh, :ww]
    return out


def nb_min(a, b):
    """Per pixel of a, the smallest max-channel distance to a pixel of b within 1 px."""
    h, w = a.shape[:2]
    pb = np.pad(b, ((1, 1), (1, 1), (0, 0)), mode='edge')
    best = None
    for dy in (0, 1, 2):
        for dx in (0, 1, 2):
            d = np.abs(a - pb[dy:dy + h, dx:dx + w]).max(axis=2)
            best = d if best is None else np.minimum(best, d)
    return best


def differs(a, b):
    return (nb_min(a, b) > TOL) & (nb_min(b, a) > TOL)


def objects(page, h_px):
    """Top-level page objects as (class, x0, y0, x1, y1) in pixels (y down), in paint order."""
    out = []
    ph = page.get_size()[1]
    for o in page.get_objects(max_depth=1):
        t = o.type
        cls = CLASSES.get(t, 'other')
        try:
            l, b, r, tp = o.get_bounds()
        except Exception:
            continue
        if cls == 'image':
            try:
                w, h = o.get_px_size()
                if w == 1 and h == 1:
                    cls = 'shadow'
            except Exception:
                pass
        if cls == 'form':
            kinds = set()
            try:
                n = raw.FPDFFormObj_CountObjects(o.raw)
                for k in range(min(n, 64)):
                    kinds.add(CLASSES.get(raw.FPDFPageObj_GetType(raw.FPDFFormObj_GetObject(o.raw, k)), 'other'))
            except Exception:
                pass
            if kinds == {'image'}:
                cls = 'raster'
        x0, x1 = int(np.floor(l * SCALE)), int(np.ceil(r * SCALE))
        y0, y1 = int(np.floor((ph - tp) * SCALE)), int(np.ceil((ph - b) * SCALE))
        out.append((cls, x0, y0, x1, y1))
    return out


def label_map(objs, shape):
    names = ['none']
    m = np.zeros(shape[:2], dtype=np.int32)
    for cls, x0, y0, x1, y1 in objs:
        if cls not in names:
            names.append(cls)
        x0, y0 = max(0, x0), max(0, y0)
        if x1 > x0 and y1 > y0:
            m[y0:y1, x0:x1] = names.index(cls)
    return m, names


def text_of(page):
    tp = page.get_textpage()
    n = tp.count_chars()
    chars = []
    fonts = Counter()
    buf = ctypes.create_string_buffer(256)
    flags = ctypes.c_int(0)
    x, y = ctypes.c_double(0), ctypes.c_double(0)
    for i in range(n):
        cp = raw.FPDFText_GetUnicode(tp.raw, i)
        if cp in (0, 0xFFFE, 0xFFFF) or chr(cp).isspace() or raw.FPDFText_IsGenerated(tp.raw, i) == 1:
            continue
        raw.FPDFText_GetCharOrigin(tp.raw, i, ctypes.byref(x), ctypes.byref(y))
        size = raw.FPDFText_GetFontSize(tp.raw, i)
        ln = raw.FPDFText_GetFontInfo(tp.raw, i, buf, 256, ctypes.byref(flags))
        name = buf.raw[:max(0, ln - 1)].decode('latin-1', 'replace')
        name = name.split('+', 1)[1] if len(name) > 7 and name[6] == '+' else name
        fonts[name] += 1
        chars.append((chr(cp), x.value, y.value, size))
    tp.close()
    return chars, fonts


def norm(c):
    o = ord(c)
    if 0xF020 <= o <= 0xF0FF or c in '•▪●–—·■∙':
        return '•'
    return c


def compare_text(a, b):
    sa = [norm(c[0]) for c in a]
    sb = [norm(c[0]) for c in b]
    if not sa and not sb:
        return 1.0, 0, []
    sm = difflib.SequenceMatcher(None, sa, sb, autojunk=False)
    ratio = sm.ratio() if len(sa) + len(sb) < 40000 else sm.quick_ratio()
    d = []
    matched = 0
    for blk in sm.get_matching_blocks():
        for k in range(blk.size):
            ca, cb = a[blk.a + k], b[blk.b + k]
            d.append((cb[1] - ca[1], cb[2] - ca[2], cb[3] - ca[3]))
            matched += 1
    return ratio, matched, d


def links(doc, page):
    out = []
    pos = ctypes.c_int(0)
    link = raw.FPDF_LINK()
    while raw.FPDFLink_Enumerate(page.raw, ctypes.byref(pos), ctypes.byref(link)):
        r = raw.FS_RECTF()
        raw.FPDFLink_GetAnnotRect(link, ctypes.byref(r))
        act = raw.FPDFLink_GetAction(link)
        uri = ''
        if act:
            n = raw.FPDFAction_GetURIPath(doc.raw, act, None, 0)
            if n > 0:
                buf = ctypes.create_string_buffer(n)
                raw.FPDFAction_GetURIPath(doc.raw, act, buf, n)
                uri = buf.raw[:n - 1].decode('latin-1', 'replace')
        out.append((uri, r.left, r.bottom, r.right, r.top))
    return out


def iou(a, b):
    ix = max(0, min(a[3], b[3]) - max(a[1], b[1]))
    iy = max(0, min(a[4], b[4]) - max(a[2], b[2]))
    inter = ix * iy
    ua = (a[3] - a[1]) * (a[4] - a[2]) + (b[3] - b[1]) * (b[4] - b[2]) - inter
    return inter / ua if ua > 0 else 0.0


def read_tsv(path):
    rows = []
    if not os.path.exists(path):
        return rows
    with open(path, encoding='utf-8-sig') as f:
        for line in f:
            if line.startswith('#') or not line.strip():
                continue
            rows.append(line.rstrip('\n').split('\t'))
    return rows


def info_of(path):
    if not os.path.exists(path):
        return {}
    with open(path, encoding='utf-8-sig') as f:
        txt = f.read().strip()
    if txt.startswith('failed='):
        return {'failed': txt[7:]}
    return dict(kv.split('=', 1) for kv in txt.split('\t') if '=' in kv)


def pct(v, q):
    return float(np.percentile(v, q)) if len(v) else 0.0


def main():
    work, oracle = sys.argv[1], sys.argv[2]
    res_dir = os.path.join(oracle, 'results')
    diff_dir = os.path.join(work, 'diff')
    os.makedirs(res_dir, exist_ok=True)
    os.makedirs(diff_dir, exist_ok=True)
    decks = read_tsv(os.path.join(oracle, 'decks.tsv'))
    rust_reports = {}
    for pol in ('system', 'open'):
        rust_reports[pol] = {r[0]: r for r in read_tsv(os.path.join(work, f'rust-{pol}', 'report.tsv'))}
    only = set(os.environ.get('UO_PPT_ORACLE_ONLY', '').split(',')) - {''}

    page_rows, deck_rows, ppt_rows = [], [], []
    cls_rust, cls_ppt, cls_pair = Counter(), Counter(), Counter()
    split_tot = Counter()
    font_pairs = Counter()
    link_tot = Counter()
    feat_pages = defaultdict(list)
    worst = []
    for d in decks:
        did, path, group, digest = d[0], d[1], d[2], d[3]
        feats = d[4].split(',') if len(d) > 4 and d[4] else []
        if only and did not in only:
            continue
        pinfo = info_of(os.path.join(work, 'ppt', f'{did}.info'))
        ppdf = os.path.join(work, 'ppt', f'{did}.pdf')
        if 'failed' in pinfo or not os.path.exists(ppdf):
            ppt_rows.append([did, group, digest, '', '', '', '', 'refused: ' + pinfo.get('failed', 'no pdf').replace('\t', ' ')])
            deck_rows.append([did, group, 'ppt-refused', '', '', '', '', '', '', '', ''])
            continue
        with open(ppdf, 'rb') as f:
            pbytes = f.read()
        ppt_rows.append([did, group, digest, hashlib.sha256(pbytes).hexdigest()[:16], str(len(pbytes)), pinfo.get('slides', ''), pinfo.get('hidden', ''), pinfo.get('secs', '')])
        P = pdfium.PdfDocument(ppdf)
        R = {}
        for pol in ('system', 'open'):
            rp = os.path.join(work, f'rust-{pol}', f'{did}.pdf')
            R[pol] = pdfium.PdfDocument(rp) if os.path.exists(rp) else None
        if R['system'] is None:
            err = rust_reports['system'].get(did, [''] * 14)[-1]
            deck_rows.append([did, group, 'rust-failed: ' + err, str(len(P)), '', '', '', '', '', '', ''])
            continue
        np_ppt, np_rs = len(P), len(R['system'])
        rep = rust_reports['system'].get(did)
        miss_pp = rep[11].split(',') if rep and len(rep) > 11 and rep[11] else []
        ts_dir = os.path.join(work, 'ts', did)
        agrees = []
        for i in range(min(np_ppt, np_rs)):
            pp, rs = P[i], R['system'][i]
            pw, ph = pp.get_size()
            rw, rh = rs.get_size()
            A = render(pp)
            B = fit(render(rs), A.shape)
            bad_rp = differs(B, A)
            n = bad_rp.size
            row = {'id': did, 'page': i + 1, 'group': group, 'ppt_w': pw, 'ppt_h': ph, 'rust_w': rw, 'rust_h': rh}
            row['agree_sys'] = 1 - bad_rp.sum() / n
            row['mad_sys'] = float(np.abs(A - B).mean())
            if R['open'] is not None and i < len(R['open']):
                O = fit(render(R['open'][i]), A.shape)
                row['agree_open'] = 1 - differs(O, A).sum() / n
            tsp = os.path.join(ts_dir, f'{i + 1}.png')
            T = None
            if os.path.exists(tsp):
                T = load_png(tsp, A.shape)
                bad_tp = differs(T, A)
                bad_rt = differs(B, T)
                row['agree_ts'] = 1 - bad_tp.sum() / n
                row['agree_rust_ts'] = 1 - bad_rt.sum() / n
                legacy = bad_rp & ~bad_rt
                pdf_only = bad_rp & bad_rt & ~bad_tp
                both = bad_rp & bad_rt & bad_tp
                row['px_legacy'] = int(legacy.sum())
                row['px_pdf_only'] = int(pdf_only.sum())
                row['px_both'] = int(both.sum())
                split_tot['legacy'] += row['px_legacy']
                split_tot['pdf_only'] += row['px_pdf_only']
                split_tot['both'] += row['px_both']
            row['px_bad'] = int(bad_rp.sum())
            split_tot['bad'] += row['px_bad']
            split_tot['all'] += n
            # topmost object of each PDF under the differing pixels
            mr, nr = label_map(objects(rs, A.shape[0]), A.shape)
            mp, npn = label_map(objects(pp, A.shape[0]), A.shape)
            lr, lp = mr[bad_rp], mp[bad_rp]
            page_cls = Counter()
            if lr.size:
                pairs, counts = np.unique(np.stack([lr, lp]), axis=1, return_counts=True)
                for (a_, b_), c in zip(pairs.T, counts):
                    cls_pair[(nr[a_], npn[b_])] += int(c)
                    cls_rust[nr[a_]] += int(c)
                    cls_ppt[npn[b_]] += int(c)
                    page_cls[f'{nr[a_]}/{npn[b_]}'] += int(c)
            row['top_classes'] = ';'.join(f'{k}={v}' for k, v in page_cls.most_common(3))
            # text
            ta, fa = text_of(pp)
            tb, fb = text_of(rs)
            ratio, matched, dpos = compare_text(ta, tb)
            row['chars_ppt'], row['chars_rust'], row['text_ratio'], row['matched'] = len(ta), len(tb), ratio, matched
            if dpos:
                dd = np.array(dpos)
                # signed offsets of the sequence-matched glyphs (Rust - PowerPoint, pt; y up)
                row['dx_med'], row['dy_med'] = float(np.median(dd[:, 0])), float(np.median(dd[:, 1]))
                row['dsize_med'] = float(np.median(np.abs(dd[:, 2])))
            # order-free glyph placement: each PowerPoint glyph against the nearest Rust glyph of the same character
            if ta:
                by = defaultdict(list)
                for c in tb:
                    by[norm(c[0])].append((c[1], c[2]))
                arr = {k: np.array(v) for k, v in by.items()}
                dist = np.full(len(ta), 1e9)
                for k_, c in enumerate(ta):
                    v = arr.get(norm(c[0]))
                    if v is not None:
                        dist[k_] = float(np.min(np.hypot(v[:, 0] - c[1], v[:, 1] - c[2])))
                row['dpos_med'] = float(np.median(dist))
                row['within_1pt'] = float((dist <= 1.0).mean())
                row['within_3pt'] = float((dist <= 3.0).mean())
            row['fonts_ppt'] = ','.join(sorted(fa))
            row['fonts_rust'] = ','.join(sorted(fb))
            for k in sorted(set(fa) | set(fb)):
                font_pairs[(k, k in fa, k in fb)] += fa.get(k, 0) + fb.get(k, 0)
            # links
            la, lb = links(P, pp), links(R['system'], rs)
            row['links_ppt'], row['links_rust'] = len(la), len(lb)
            ok = 0
            used = set()
            for x in la:
                for j, y in enumerate(lb):
                    if j not in used and x[0] == y[0] and iou(x, y) > 0.3:
                        used.add(j)
                        ok += 1
                        break
            row['links_match'] = ok
            link_tot['ppt'] += len(la)
            link_tot['rust'] += len(lb)
            link_tot['match'] += ok
            link_tot['uri_ppt'] += len({x[0] for x in la})
            row['rust_missing'] = miss_pp[i] if i < len(miss_pp) else ''
            agrees.append(row['agree_sys'])
            page_rows.append(row)
            for f in feats:
                feat_pages[f].append(row['agree_sys'])
            worst.append((row['agree_sys'], did, i + 1))
            # diff picture for the inspection: PowerPoint | Rust | TS | differing pixels (red legacy, blue PDF only, magenta both)
            if row['agree_sys'] < 0.995:
                m = np.full(A.shape, 255, dtype=np.uint8)
                m[bad_rp] = (0, 0, 0)
                if T is not None:
                    m[legacy] = (220, 30, 30)
                    m[pdf_only] = (30, 60, 230)
                    m[both] = (200, 0, 200)
                tiles = [A.astype(np.uint8), B.astype(np.uint8), (T if T is not None else np.full(A.shape, 200)).astype(np.uint8), m]
                Image.fromarray(np.concatenate([np.concatenate(tiles[:2], 1), np.concatenate(tiles[2:], 1)], 0)).save(os.path.join(diff_dir, f'{did}-{i + 1}.png'))
        deck_rows.append([did, group, 'ok' if np_ppt == np_rs else f'pages ppt {np_ppt} rust {np_rs}', str(np_ppt), str(np_rs),
                          f'{np.mean(agrees):.4f}' if agrees else '', f'{min(agrees):.4f}' if agrees else '',
                          pinfo.get('hidden', ''), rep[10] if rep and len(rep) > 10 else '', rep[4] if rep else '', rep[5] if rep else ''])
        P.close()
        for v in R.values():
            if v is not None:
                v.close()

    cols = ['id', 'page', 'group', 'ppt_w', 'ppt_h', 'rust_w', 'rust_h', 'agree_sys', 'agree_open', 'agree_ts', 'agree_rust_ts', 'mad_sys', 'px_bad',
            'px_legacy', 'px_pdf_only', 'px_both', 'top_classes', 'chars_ppt', 'chars_rust', 'matched', 'text_ratio', 'dx_med', 'dy_med', 'dpos_med',
            'within_1pt', 'within_3pt', 'dsize_med', 'links_ppt', 'links_rust', 'links_match', 'rust_missing', 'fonts_ppt', 'fonts_rust']

    def fmt(v):
        if isinstance(v, float):
            return f'{v:.4f}'
        return str(v)
    with open(os.path.join(res_dir, 'pages.tsv'), 'w', encoding='utf-8', newline='\n') as f:
        f.write('# ' + '\t'.join(cols) + '\n')
        for r in page_rows:
            f.write('\t'.join(fmt(r.get(c, '')) for c in cols) + '\n')
    with open(os.path.join(res_dir, 'decks.tsv'), 'w', encoding='utf-8', newline='\n') as f:
        f.write('# id\tgroup\tstatus\tppt pages\trust pages\tmean agreement\tmin agreement\thidden slides\trust missing\trust fallback glyphs\trust links\n')
        for r in deck_rows:
            f.write('\t'.join(r) + '\n')
    with open(os.path.join(res_dir, 'ppt-pdfs.tsv'), 'w', encoding='utf-8', newline='\n') as f:
        f.write('# id\tgroup\tdeck sha256 (16)\tPowerPoint PDF sha256 (16)\tbytes\tslides\thidden slides\tseconds (or refusal)\n')
        for r in ppt_rows:
            f.write('\t'.join(r) + '\n')
    # feature impact over the generated decks: mean agreement of pages with the feature vs without
    allp = [r['agree_sys'] for r in page_rows if r['group'] not in ('samples', 'playback', 'playback-ppt', 'legacy')]
    feat_rows = []
    for k, v in feat_pages.items():
        if len(v) >= 3:
            feat_rows.append((float(np.mean(v)), len(v), k))
    feat_rows.sort()
    with open(os.path.join(res_dir, 'features.tsv'), 'w', encoding='utf-8', newline='\n') as f:
        f.write(f'# generator feature\tpages with it\tmean agreement (Rust system x PowerPoint)\tall generated pages {len(allp)}: {np.mean(allp) if allp else 0:.4f}\n')
        for m, n, k in feat_rows:
            f.write(f'{k}\t{n}\t{m:.4f}\n')

    def agg(key, rows):
        v = [r[key] for r in rows if key in r]
        return {'n': len(v), 'mean': float(np.mean(v)) if v else None, 'median': pct(v, 50), 'p10': pct(v, 10), 'min': float(min(v)) if v else None,
                'ge_99': float(np.mean([x >= 0.99 for x in v])) if v else None, 'ge_95': float(np.mean([x >= 0.95 for x in v])) if v else None}
    groups = sorted({r['group'] for r in page_rows})
    summary = {
        'tolerance': TOL, 'dpi': DPI,
        'decks': len(decks if not only else [d for d in decks if d[0] in only]),
        'decks_refused_by_powerpoint': sum(1 for r in deck_rows if r[2] == 'ppt-refused'),
        'decks_page_count_differs': sum(1 for r in deck_rows if r[2].startswith('pages')),
        'decks_rust_failed': sum(1 for r in deck_rows if r[2].startswith('rust-failed')),
        'pages': len(page_rows),
        'page_size_differs': sum(1 for r in page_rows if abs(r['ppt_w'] - r['rust_w']) > 0.5 or abs(r['ppt_h'] - r['rust_h']) > 0.5),
        'agreement': {k: agg(k, page_rows) for k in ('agree_sys', 'agree_open', 'agree_ts', 'agree_rust_ts', 'text_ratio', 'within_1pt', 'within_3pt')},
        'agreement_by_group': {g: {k: agg(k, [r for r in page_rows if r['group'] == g]) for k in ('agree_sys', 'agree_ts', 'agree_rust_ts', 'text_ratio', 'within_1pt')} for g in groups},
        'differing_pixels_split': dict(split_tot),
        'differing_pixels_by_rust_object': dict(cls_rust.most_common()),
        'differing_pixels_by_ppt_object': dict(cls_ppt.most_common()),
        'differing_pixels_by_pair': {f'{a}/{b}': c for (a, b), c in cls_pair.most_common(20)},
        'links': dict(link_tot),
        'fonts': [{'font': k, 'in_ppt': a, 'in_rust': b, 'chars': c} for (k, a, b), c in sorted(font_pairs.items(), key=lambda x: -x[1])[:60]],
        'worst_pages': [{'id': d, 'page': p, 'agree_sys': round(a, 4)} for a, d, p in sorted(worst)[:40]],
    }
    with open(os.path.join(res_dir, 'summary.json'), 'w', encoding='utf-8', newline='\n') as f:
        json.dump(summary, f, indent=1)
    a = summary['agreement']
    print(f"pages {summary['pages']}, refused {summary['decks_refused_by_powerpoint']}, agree_sys mean {a['agree_sys']['mean']}, open {a['agree_open']['mean']}, ts {a['agree_ts']['mean']}, rust-ts {a['agree_rust_ts']['mean']}")


if __name__ == '__main__':
    main()
