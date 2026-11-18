"""Cause classes of the PowerPoint-PDF oracle (after pdf-oracle-compare.py): detectors that tag each compared page with the known
causes, then the differing pixels (Rust system x PowerPoint) of the tagged pages, split by what the TS canvas sides with.

usage: python -I pdf-oracle-classify.py <work dir> <tests/oracle_ppt dir>   -> <oracle dir>/results/classes.tsv (+ stdout summary)

Detectors (a page may carry several):
  gradient-text   the Rust page fills text with a gradient as a text clip (7 Tr ... 0 Tr ET) + `sh`: pdfium adds a text clip at
                  ET only when a clip mode is in force there, so with the reset it paints the whole clip region (the page) with
                  the gradient (fixed 2026-11-17: the mode stays 7 until ET; pages of the fixed form are not tagged)
  sysclr          the deck's theme/parts use <a:sysClr> whose lastClr is not the Windows value of the colour (PowerPoint takes the
                  system colour: windowText 000000, window FFFFFF, highlight 0078D7, ...; the TS takes lastClr)
  ppt-chart       a .ppt page whose .pptx twin has a chart on the same slide (the converter keeps the chart's fallback picture)
  svg             the Rust report counts an SVG picture it cannot draw (image:unsupported) on the page
  glyph-missing   the Rust report counts characters without any face on the page
  chart           the slide has a chart (graphicFrame with a c:chart)
  wordart         the slide has WordArt (a:prstTxWarp other than textNoShape)
  field           the slide uses a:fld (date/slide number fields)
  baseline        median vertical glyph offset of the page beyond 0.3 pt (Rust - PowerPoint)
  links           the multiset of link URIs differs
"""
import os
import re
import sys
import zipfile
from collections import Counter, defaultdict

SYSTEM = {'windowtext': '000000', 'window': 'FFFFFF', 'highlight': '0078D7', 'highlighttext': 'FFFFFF', 'btnface': 'F0F0F0',
          'btntext': '000000', 'graytext': '6D6D6D', 'menu': 'F0F0F0', 'menutext': '000000', '3ddkshadow': '696969', '3dlight': 'E3E3E3',
          'activecaption': '99B4D1', 'captiontext': '000000', 'infotext': '000000', 'infobk': 'FFFFE1', 'scrollbar': 'C8C8C8',
          'background': '000000', 'windowframe': '646464', 'btnshadow': 'A0A0A0', 'btnhighlight': 'FFFFFF', 'appworkspace': 'ABABAB'}


def read_rows(path):
    rows = [l.rstrip('\n').split('\t') for l in open(path, encoding='utf-8') if l.strip()]
    head = rows[0]
    head[0] = head[0].lstrip('# ')
    return [dict(zip(head, r)) for r in rows[1:]]


def sysclr_mismatch(path):
    if path.lower().endswith('.ppt'):
        return False
    z = zipfile.ZipFile(path)
    for n in z.namelist():
        if n.endswith('.xml'):
            for v, last in re.findall(rb'<a:sysClr val="([^"]+)" lastClr="([0-9A-Fa-f]{6})"', z.read(n)):
                want = SYSTEM.get(v.decode().lower())
                if want and want != last.decode().upper():
                    return True
    return False


def slide_parts(path):
    """Per slide (1-based, presentation order): its XML text (for chart and field detection)."""
    if path.lower().endswith('.ppt'):
        return {}
    z = zipfile.ZipFile(path)
    pres = z.read('ppt/presentation.xml').decode('utf-8', 'replace')
    rels = z.read('ppt/_rels/presentation.xml.rels').decode('utf-8', 'replace')
    target = dict(re.findall(r'Id="([^"]+)"[^>]*Target="([^"]+)"', rels))
    target.update({a: b for b, a in re.findall(r'Target="([^"]+)"[^>]*Id="([^"]+)"', rels)})
    out = {}
    for i, rid in enumerate(re.findall(r'<p:sldId [^>]*r:id="([^"]+)"', pres)):
        t = target.get(rid, '')
        name = 'ppt/' + t.lstrip('/').replace('ppt/', '') if t else ''
        try:
            out[i + 1] = z.read(name).decode('utf-8', 'replace')
        except KeyError:
            out[i + 1] = ''
    return out


def gradient_text_pages(pdf):
    import zlib
    b = open(pdf, 'rb').read()
    objs = {int(m.group(1)): m.end() for m in re.finditer(rb'(\d+) 0 obj\n', b)}
    pages = [n for n, s in objs.items() if b[s:s + 15] == b'<< /Type /Page ']
    flagged = set()
    for k, n in enumerate(pages):
        s = objs[n]
        m = re.search(rb'/Contents (\d+) 0 R', b[s:b.find(b'endobj', s)])
        if not m or int(m.group(1)) not in objs:
            continue
        cs = objs[int(m.group(1))]
        i = b.find(b'stream\n', cs)
        e = b.find(b'endstream', i)
        try:
            d = zlib.decompressobj().decompress(b[i + 7:e])
        except Exception:
            d = b[i + 7:e]
        # only the broken form: the mode reset to 0 before ET (pdfium then drops the clip); a clip mode still set at ET
        # (the fix of 2026-11-17) is honoured by pdfium
        for m in re.finditer(rb'7 Tr(.*?)ET\s*/Sh\d+ sh', d, re.S):
            if b'0 Tr' in m.group(1):
                flagged.add(k + 1)
                break
    return flagged


def main():
    work, oracle = sys.argv[1], sys.argv[2]
    repo = os.path.abspath(os.path.join(oracle, '../../../../..'))
    decks = {r['id']: r for r in read_rows(os.path.join(oracle, 'decks.tsv'))}
    pages = read_rows(os.path.join(oracle, 'results', 'pages.tsv'))
    rust = {r['id']: r for r in read_rows(os.path.join(work, 'rust-system', 'report.tsv'))}
    by_deck = defaultdict(list)
    for p in pages:
        by_deck[p['id']].append(p)
    tags = defaultdict(set)
    for did, plist in by_deck.items():
        path = os.path.join(repo, decks[did]['path (repo-relative)'])
        gt = gradient_text_pages(os.path.join(work, 'rust-system', f'{did}.pdf'))
        sc = sysclr_mismatch(path)
        parts = slide_parts(path)
        twin = None
        if path.lower().endswith('.ppt'):
            tp = path[:-4] + '.pptx'
            if os.path.exists(tp):
                twin = slide_parts(tp)
        rep = rust.get(did, {})
        miss = rep.get('missing (key=count;...)', '')
        for p in plist:
            n = int(p['page'])
            key = (did, n)
            if n in gt:
                tags[key].add('gradient-text')
            if sc:
                tags[key].add('sysclr')
            if twin and 'graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart"' in twin.get(n, ''):
                tags[key].add('ppt-chart')
            if 'image:unsupported' in miss and p['rust_missing'] not in ('', '0'):
                tags[key].add('svg')
            if 'glyph:missing' in miss and p['rust_missing'] not in ('', '0'):
                tags[key].add('glyph-missing')
            if 'drawingml/2006/chart"' in parts.get(n, ''):
                tags[key].add('chart')
            if '<a:prstTxWarp prst="' in parts.get(n, '') and '<a:prstTxWarp prst="textNoShape"' not in parts.get(n, ''):
                tags[key].add('wordart')
            if '<a:fld ' in parts.get(n, ''):
                tags[key].add('field')
            try:
                if abs(float(p['dy_med'])) > 0.3:
                    tags[key].add('baseline')
            except ValueError:
                pass
            if p['links_ppt'] != p['links_rust']:
                tags[key].add('links')
    agg = defaultdict(Counter)
    total = Counter()
    for p in pages:
        key = (p['id'], int(p['page']))
        vals = {k: int(p[k] or 0) for k in ('px_bad', 'px_legacy', 'px_pdf_only', 'px_both')}
        for k, v in vals.items():
            total[k] += v
        total['pages'] += 1
        for t in tags[key] or {'(none)'}:
            agg[t]['pages'] += 1
            agg[t]['decks'] = agg[t].get('decks', 0)
            for k, v in vals.items():
                agg[t][k] += v
    deck_count = defaultdict(set)
    for (did, n), ts in tags.items():
        for t in ts:
            deck_count[t].add(did)
    # exclusive attribution: the first matching tag in this priority order owns the page's pixels
    order = ['gradient-text', 'sysclr', 'ppt-chart', 'svg', 'chart', 'wordart', 'glyph-missing', 'baseline', 'field', 'links']
    excl = Counter()
    excl_split = defaultdict(Counter)
    for p in pages:
        key = (p['id'], int(p['page']))
        own = next((t for t in order if t in tags[key]), '(none)')
        excl[own] += int(p['px_bad'] or 0)
        for k in ('px_legacy', 'px_pdf_only', 'px_both'):
            excl_split[own][k] += int(p[k] or 0)
    with open(os.path.join(oracle, 'results', 'classes.tsv'), 'w', encoding='utf-8', newline='\n') as f:
        f.write('# class\tpages\tdecks\tdiffering px on those pages\texclusive px (priority order)\tTS legacy px\tPDF-only px\tall-three px\texclusive TS legacy\texclusive PDF-only\texclusive all-three\n')
        for t in sorted(agg, key=lambda t: -excl[t]):
            a = agg[t]
            e = excl_split[t]
            f.write(f"{t}\t{a['pages']}\t{len(deck_count[t])}\t{a['px_bad']}\t{excl[t]}\t{a['px_legacy']}\t{a['px_pdf_only']}\t{a['px_both']}\t"
                    f"{e['px_legacy']}\t{e['px_pdf_only']}\t{e['px_both']}\n")
        f.write(f"(all)\t{total['pages']}\t{len(by_deck)}\t{total['px_bad']}\t{total['px_bad']}\t{total['px_legacy']}\t{total['px_pdf_only']}\t{total['px_both']}\n")
    print(open(os.path.join(oracle, 'results', 'classes.tsv'), encoding='utf-8').read())
    # agreement without the two whole-page classes
    import statistics
    clean = [float(p['agree_sys']) for p in pages if not ({'gradient-text', 'sysclr'} & tags[(p['id'], int(p['page']))])]
    print('pages without gradient-text/sysclr:', len(clean), 'mean', round(statistics.mean(clean), 4), 'median', round(statistics.median(clean), 4))


if __name__ == '__main__':
    main()
