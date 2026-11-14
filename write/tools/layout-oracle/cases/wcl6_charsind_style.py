# WordCraft study, lines, review of L6: a paragraph style's *Chars indents against direct twips and direct *Chars of 0.
# Usage: python wcl6_charsind_style.py <out dir> -> <out dir>/p-charsind-style.docx (measured: Word PDF via word-export.ps1
# under tools/parallel/word-lock.mjs, glyphs by core/target/wordcraft-study/A/extract_xs.py; fixture
# core/crates/uo-docx-layout/tests/oracle_wc/l-charsind/charsind-style). Calibri 11 by NAME only (never embedded): 1 char = 11 pt.
import zipfile, os
W = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"'
def T(s):
    return s + ' linha um do paragrafo Hxg e a segunda linha que continua bastante para quebrar com folga no fim da coluna aqui e mais'
def style(i, ind):
    return f'<w:style w:type="paragraph" w:customStyle="1" w:styleId="{i}"><w:name w:val="{i}"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr><w:ind {ind}/></w:pPr></w:style>'
styles = ''.join([
    style('SC', 'w:leftChars="300" w:firstLineChars="200"'),
    style('SH', 'w:leftChars="300" w:hangingChars="200"'),
    style('SR', 'w:rightChars="1000"'),
    style('ST', 'w:left="1440" w:firstLine="720"'),
    style('SC2', 'w:leftChars="300" w:left="1440"'),
    style('SL', 'w:leftChars="150"'),
    style('SF', 'w:firstLineChars="200"'),
    style('SR2', 'w:rightChars="1000"'),
    style('SH2', 'w:leftChars="300" w:hangingChars="200"'),
    style('SN', 'w:leftChars="777"'),
    '<w:style w:type="paragraph" w:customStyle="1" w:styleId="SB"><w:name w:val="SB"/><w:basedOn w:val="Normal"/><w:pPr><w:ind w:leftChars="300"/></w:pPr><w:rPr><w:sz w:val="40"/></w:rPr></w:style>',
])
def p(text, ppr=''):
    pp = f'<w:pPr><w:spacing w:after="0"/>{ppr}</w:pPr>' if True else ''
    # pStyle must come first: rebuild
    if ppr.startswith('<w:pStyle'):
        k = ppr.index('/>') + 2
        pp = f'<w:pPr>{ppr[:k]}<w:spacing w:after="0"/>{ppr[k:]}</w:pPr>'
    return f'<w:p>{pp}<w:r><w:t xml:space="preserve">{text}</w:t></w:r></w:p>'
def ps(i, extra=''):
    return f'<w:pStyle w:val="{i}"/>{extra}'
cases = [
    ('a SC ref', ''),
    ('b SC ref', ps('SC')),
    ('c SC + left1440', ps('SC', '<w:ind w:left="1440"/>')),
    ('d SC + left0', ps('SC', '<w:ind w:left="0"/>')),
    ('e SC + firstLine720', ps('SC', '<w:ind w:firstLine="720"/>')),
    ('f SC + hanging720', ps('SC', '<w:ind w:hanging="720"/>')),
    ('g SH ref', ps('SH')),
    ('h SH + hanging720', ps('SH', '<w:ind w:hanging="720"/>')),
    ('i SH + left1440', ps('SH', '<w:ind w:left="1440"/>')),
    ('j SR ref', ps('SR', '<w:jc w:val="right"/>')),
    ('k SR + right1440', ps('SR', '<w:ind w:right="1440"/><w:jc w:val="right"/>')),
    ('l ST ref', ps('ST')),
    ('m ST + leftChars300', ps('ST', '<w:ind w:leftChars="300"/>')),
    ('n ST + firstLineChars100', ps('ST', '<w:ind w:firstLineChars="100"/>')),
    ('o SC + leftChars0', ps('SC', '<w:ind w:leftChars="0"/>')),
    ('p SC + left1440 leftChars0', ps('SC', '<w:ind w:leftChars="0" w:left="1440"/>')),
    ('q SC2 + leftChars0', ps('SC2', '<w:ind w:leftChars="0"/>')),
    ('r SL + leftChars0', ps('SL', '<w:ind w:leftChars="0"/>')),
    ('s SF + firstLineChars0', ps('SF', '<w:ind w:firstLineChars="0"/>')),
    ('t SR2 + rightChars0', ps('SR2', '<w:ind w:rightChars="0"/><w:jc w:val="right"/>')),
    ('u SH2 + hangingChars0', ps('SH2', '<w:ind w:hangingChars="0"/>')),
    ('v SN + leftChars0', ps('SN', '<w:ind w:leftChars="0"/>')),
    ('w SB 20pt + leftChars0', ps('SB', '<w:ind w:leftChars="0"/>')),
    ('x direct leftChars0 only', '<w:ind w:leftChars="0"/>'),
]
body = ''.join(p(T(t), pp) for t, pp in cases)
sect = '<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1701" w:bottom="1417" w:left="1701" w:header="708" w:footer="708" w:gutter="0"/><w:cols w:space="708"/></w:sectPr>'
doc = f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {W}><w:body>{body}{sect}</w:body></w:document>'
dd = '<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="Calibri" w:cs="Calibri"/><w:sz w:val="22"/><w:szCs w:val="22"/><w:lang w:val="pt-BR" w:eastAsia="en-US" w:bidi="ar-SA"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>'
sty = f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles {W}>{dd}<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style>{styles}</w:styles>'
ct = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/></Types>'
rels = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>'
drels = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdS" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>'
import sys
out = os.path.join(sys.argv[1] if len(sys.argv) > 1 else '.', 'p-charsind-style.docx')
with zipfile.ZipFile(out, 'w', zipfile.ZIP_DEFLATED) as z:
    z.writestr('[Content_Types].xml', ct)
    z.writestr('_rels/.rels', rels)
    z.writestr('word/document.xml', doc)
    z.writestr('word/styles.xml', sty)
    z.writestr('word/_rels/document.xml.rels', drels)
print(out)
