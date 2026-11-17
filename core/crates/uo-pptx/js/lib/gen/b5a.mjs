// The B5a deck builder: SmartArt decks. Each deck has 1-2 slides with 1-3 graphic frames of a
// `dgm:relIds` diagram: data model (`dgm:dataModel`), layout definition, quick style, colours, and
// (in some frames) the cached drawing (`dsp:drawing`). Frames WITHOUT a usable drawing make the reader
// regenerate the layout (diagram-layout.ts) or fall back (diagram-fallback.ts).
//
// The layout definitions are compact versions of the application's own families (list, process, cycle,
// radial, hierarchy, pyramid, matrix, chevron, vertical list): the structure the engine interprets
// (layoutNode / forEach / choose / presOf / constrLst / ruleLst / alg / shape), authored here so no
// Office file is needed. Seeded: same seed -> same bytes.
import { zipBytes } from '../zip.mjs';
import { Rand } from './rand.mjs';
import { NS, XML, appXml, contentTypes, coreXml, layoutSpecs, layoutXml, makeTemplate, masterXml, presPropsXml, presentationXml, relsXml, tableStylesXml, themeXml } from './parts.mjs';
import { esc } from './dml.mjs';
import { words } from './text.mjs';

const DGM = 'xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"';
const GRP = '<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>';
const SHP = (t, extra = '') => `<dgm:shape ${t ? `type="${t}" ` : ''}r:blip=""><dgm:adjLst>${extra}</dgm:adjLst></dgm:shape>`;
const TX = (m = 0.3) => ['tMarg', 'bMarg', 'lMarg', 'rMarg'].map((k) => `<dgm:constr type="${k}" refType="primFontSz" fact="${m}"/>`).join('');
const FONTRULE = '<dgm:rule type="primFontSz" val="5" fact="NaN" max="NaN"/>';
const f = (r, a, b) => (a + (b - a) * r.below(1000) / 1000).toFixed(3);

/** Layout families: name -> { id, build(r) -> layoutDef body, pres: layoutNode names tied to data nodes, shape: data tree kind }. */
const FAMILIES = {
  default: {
    id: 'default', kind: 'list', pres: ['node'],
    body: (r) => `<dgm:layoutNode name="diagram"><dgm:varLst><dgm:dir/><dgm:resizeHandles val="exact"/></dgm:varLst><dgm:choose name="N0"><dgm:if name="N1" func="var" arg="dir" op="equ" val="norm"><dgm:alg type="snake"><dgm:param type="grDir" val="${r.pick(['tL', 'tR', 'bL', 'bR'])}"/><dgm:param type="flowDir" val="${r.pick(['row', 'row', 'col'])}"/><dgm:param type="contDir" val="${r.pick(['sameDir', 'revDir'])}"/><dgm:param type="off" val="${r.pick(['ctr', 'ctr', 'off'])}"/>${r.chance(0.2) ? `<dgm:param type="bkpt" val="fixed"/><dgm:param type="bkPtFixedVal" val="${r.int(1, 4)}"/>` : ''}</dgm:alg></dgm:if><dgm:else name="N2"><dgm:alg type="snake"><dgm:param type="grDir" val="tR"/><dgm:param type="flowDir" val="row"/></dgm:alg></dgm:else></dgm:choose>${SHP('')}<dgm:presOf/><dgm:constrLst><dgm:constr type="w" for="ch" forName="node" refType="w"/><dgm:constr type="h" for="ch" forName="node" refType="w" refFor="ch" refForName="node" fact="${f(r, 0.4, 0.8)}"/><dgm:constr type="w" for="ch" forName="sibTrans" refType="w" refFor="ch" refForName="node" fact="${f(r, 0.05, 0.2)}"/><dgm:constr type="sp" refType="w" refFor="ch" refForName="sibTrans"/><dgm:constr type="primFontSz" for="ch" forName="node" op="equ" val="65"/></dgm:constrLst><dgm:ruleLst/><dgm:forEach name="N3" axis="ch" ptType="node"><dgm:layoutNode name="node"><dgm:varLst><dgm:bulletEnabled val="1"/></dgm:varLst><dgm:alg type="tx"/>${SHP(r.pick(['rect', 'roundRect', 'ellipse']))}<dgm:presOf axis="desOrSelf" ptType="node"/><dgm:constrLst>${TX()}</dgm:constrLst><dgm:ruleLst>${FONTRULE}</dgm:ruleLst></dgm:layoutNode><dgm:forEach name="N4" axis="followSib" ptType="sibTrans" cnt="1"><dgm:layoutNode name="sibTrans"><dgm:alg type="sp"/>${SHP('')}<dgm:presOf/><dgm:constrLst/><dgm:ruleLst/></dgm:layoutNode></dgm:forEach></dgm:forEach></dgm:layoutNode>`,
  },
  process1: {
    id: 'process1', kind: 'list', pres: ['node', 'sibTrans'],
    body: (r) => `<dgm:layoutNode name="Name0"><dgm:varLst><dgm:dir/><dgm:resizeHandles val="exact"/></dgm:varLst><dgm:choose name="N1"><dgm:if name="N2" func="var" arg="dir" op="equ" val="norm"><dgm:alg type="lin"/></dgm:if><dgm:else name="N3"><dgm:alg type="lin"><dgm:param type="linDir" val="fromR"/></dgm:alg></dgm:else></dgm:choose>${SHP('')}<dgm:presOf/><dgm:constrLst><dgm:constr type="w" for="ch" forName="node" refType="w"/><dgm:constr type="h" for="ch" forName="node" refType="w" fact="${f(r, 0.4, 0.8)}"/><dgm:constr type="w" for="ch" forName="sibTrans" refType="w" refFor="ch" refForName="node" fact="${f(r, 0.25, 0.5)}"/><dgm:constr type="primFontSz" for="ch" forName="node" op="equ" val="65"/><dgm:constr type="primFontSz" for="des" forName="connectorText" op="equ" val="55"/><dgm:constr type="primFontSz" for="des" forName="connectorText" refType="primFontSz" refFor="ch" refForName="node" op="lte" fact="0.8"/></dgm:constrLst><dgm:ruleLst/><dgm:forEach name="N4" axis="ch" ptType="node"><dgm:layoutNode name="node"><dgm:varLst><dgm:bulletEnabled val="1"/></dgm:varLst><dgm:alg type="tx"/>${SHP('roundRect', '<dgm:adj idx="1" val="0.1"/>')}<dgm:presOf axis="desOrSelf" ptType="node"/><dgm:constrLst><dgm:constr type="h" refType="w" op="gte" fact="0.4"/>${TX()}</dgm:constrLst><dgm:ruleLst>${FONTRULE}</dgm:ruleLst></dgm:layoutNode><dgm:forEach name="N5" axis="followSib" ptType="sibTrans" cnt="1"><dgm:layoutNode name="sibTrans"><dgm:alg type="conn"><dgm:param type="begPts" val="auto"/><dgm:param type="endPts" val="auto"/></dgm:alg>${SHP('conn')}<dgm:presOf axis="self"/><dgm:constrLst><dgm:constr type="h" refType="w" fact="0.62"/><dgm:constr type="connDist"/><dgm:constr type="begPad" refType="connDist" fact="0.25"/><dgm:constr type="endPad" refType="connDist" fact="0.22"/></dgm:constrLst><dgm:ruleLst/><dgm:layoutNode name="connectorText"><dgm:alg type="conn"/><dgm:shape type="conn" r:blip="" hideGeom="1"><dgm:adjLst/></dgm:shape><dgm:presOf axis="self"/><dgm:constrLst><dgm:constr type="lMarg"/><dgm:constr type="rMarg"/><dgm:constr type="tMarg"/><dgm:constr type="bMarg"/></dgm:constrLst><dgm:ruleLst/></dgm:layoutNode></dgm:layoutNode></dgm:forEach></dgm:forEach></dgm:layoutNode>`,
  },
  vList2: {
    id: 'vList2', kind: 'tree2', pres: ['parentText', 'childText'],
    body: (r) => `<dgm:layoutNode name="linear"><dgm:varLst><dgm:animLvl val="lvl"/><dgm:resizeHandles val="exact"/></dgm:varLst><dgm:alg type="lin"><dgm:param type="linDir" val="fromT"/><dgm:param type="vertAlign" val="${r.pick(['mid', 't'])}"/></dgm:alg>${SHP('')}<dgm:presOf/><dgm:constrLst><dgm:constr type="w" for="ch" forName="parentText" refType="w"/><dgm:constr type="h" for="ch" forName="parentText" refType="primFontSz" refFor="ch" refForName="parentText" fact="${f(r, 0.4, 0.6)}"/><dgm:constr type="w" for="ch" forName="childText" refType="w"/><dgm:constr type="h" for="ch" forName="childText" refType="primFontSz" refFor="ch" refForName="parentText" fact="0.46"/><dgm:constr type="h" for="ch" forName="parentText" op="equ"/><dgm:constr type="primFontSz" for="ch" forName="parentText" op="equ" val="65"/><dgm:constr type="primFontSz" for="ch" forName="childText" refType="primFontSz" refFor="ch" refForName="parentText" op="equ"/><dgm:constr type="h" for="ch" forName="spacer" refType="primFontSz" refFor="ch" refForName="parentText" fact="0.08"/></dgm:constrLst><dgm:ruleLst><dgm:rule type="primFontSz" for="ch" forName="parentText" val="5" fact="NaN" max="NaN"/></dgm:ruleLst><dgm:forEach name="N0" axis="ch" ptType="node"><dgm:layoutNode name="parentText" styleLbl="node1"><dgm:varLst><dgm:chMax val="0"/><dgm:bulletEnabled val="1"/></dgm:varLst><dgm:alg type="tx"><dgm:param type="parTxLTRAlign" val="l"/></dgm:alg>${SHP('roundRect')}<dgm:presOf axis="self"/><dgm:constrLst>${TX()}</dgm:constrLst><dgm:ruleLst><dgm:rule type="h" val="INF" fact="NaN" max="NaN"/></dgm:ruleLst></dgm:layoutNode><dgm:choose name="N1"><dgm:if name="N2" axis="ch" ptType="node" func="cnt" op="gte" val="1"><dgm:layoutNode name="childText" styleLbl="revTx"><dgm:varLst><dgm:bulletEnabled val="1"/></dgm:varLst><dgm:alg type="tx"><dgm:param type="stBulletLvl" val="1"/></dgm:alg>${SHP('rect')}<dgm:presOf axis="des" ptType="node"/><dgm:constrLst><dgm:constr type="tMarg" refType="primFontSz" fact="0.1"/><dgm:constr type="bMarg" refType="primFontSz" fact="0.1"/><dgm:constr type="lMarg" refType="w" fact="0.09"/></dgm:constrLst><dgm:ruleLst><dgm:rule type="h" val="INF" fact="NaN" max="NaN"/></dgm:ruleLst></dgm:layoutNode></dgm:if><dgm:else name="N3"/></dgm:choose><dgm:forEach name="N4" axis="followSib" ptType="sibTrans" cnt="1"><dgm:layoutNode name="spacer"><dgm:alg type="sp"/>${SHP('')}<dgm:presOf/><dgm:constrLst/><dgm:ruleLst/></dgm:layoutNode></dgm:forEach></dgm:forEach></dgm:layoutNode>`,
  },
  cycle2: {
    id: 'cycle2', kind: 'list', pres: ['node', 'sibTrans'],
    body: (r) => `<dgm:layoutNode name="cycle"><dgm:varLst><dgm:dir/><dgm:resizeHandles val="exact"/></dgm:varLst><dgm:choose name="N0"><dgm:if name="N1" func="var" arg="dir" op="equ" val="norm"><dgm:alg type="cycle"><dgm:param type="rotPath" val="${r.pick(['none', 'alongPath'])}"/>${r.chance(0.3) ? `<dgm:param type="stAng" val="${r.int(0, 90)}"/>` : ''}${r.chance(0.2) ? `<dgm:param type="spanAng" val="${r.pick([180, 270, -360])}"/>` : ''}</dgm:alg></dgm:if><dgm:else name="N2"><dgm:alg type="cycle"><dgm:param type="stAng" val="0"/><dgm:param type="spanAng" val="-360"/></dgm:alg></dgm:else></dgm:choose>${SHP('')}<dgm:presOf/><dgm:constrLst><dgm:constr type="w" for="ch" forName="node" refType="w" fact="${f(r, 0.2, 0.4)}"/><dgm:constr type="h" for="ch" forName="node" refType="w" refFor="ch" refForName="node"/><dgm:constr type="sibSp" refType="w" refFor="ch" refForName="node" fact="${f(r, 0.05, 0.2)}"/><dgm:constr type="primFontSz" for="ch" forName="node" op="equ" val="65"/></dgm:constrLst><dgm:ruleLst/><dgm:forEach name="N3" axis="ch" ptType="node"><dgm:layoutNode name="node"><dgm:varLst><dgm:bulletEnabled val="1"/></dgm:varLst><dgm:alg type="tx"/>${SHP(r.pick(['ellipse', 'roundRect']))}<dgm:presOf axis="desOrSelf" ptType="node"/><dgm:constrLst>${TX()}</dgm:constrLst><dgm:ruleLst>${FONTRULE}</dgm:ruleLst></dgm:layoutNode><dgm:forEach name="N4" axis="followSib" ptType="sibTrans" cnt="1"><dgm:layoutNode name="sibTrans"><dgm:alg type="conn"><dgm:param type="begPts" val="auto"/><dgm:param type="endPts" val="auto"/>${r.chance(0.4) ? '<dgm:param type="dim" val="1D"/><dgm:param type="connRout" val="curve"/>' : ''}</dgm:alg>${SHP('conn')}<dgm:presOf axis="self"/><dgm:constrLst><dgm:constr type="h" refType="w" fact="0.5"/><dgm:constr type="connDist"/><dgm:constr type="begPad" refType="connDist" fact="0.1"/><dgm:constr type="endPad" refType="connDist" fact="0.1"/></dgm:constrLst><dgm:ruleLst/></dgm:layoutNode></dgm:forEach></dgm:forEach></dgm:layoutNode>`,
  },
  radial1: {
    id: 'radial1', kind: 'list', pres: ['node', 'center'],
    body: (r) => `<dgm:layoutNode name="cycle"><dgm:varLst><dgm:chMax val="8"/><dgm:dir/><dgm:resizeHandles val="exact"/></dgm:varLst><dgm:alg type="cycle"><dgm:param type="ctrShpMap" val="fNode"/></dgm:alg>${SHP('')}<dgm:presOf/><dgm:constrLst><dgm:constr type="w" for="ch" forName="center" refType="w" fact="${f(r, 0.25, 0.4)}"/><dgm:constr type="h" for="ch" forName="center" refType="w" refFor="ch" refForName="center"/><dgm:constr type="w" for="ch" forName="node" refType="w" refFor="ch" refForName="center" fact="${f(r, 0.5, 0.9)}"/><dgm:constr type="h" for="ch" forName="node" refType="w" refFor="ch" refForName="node"/><dgm:constr type="sp" refType="w" refFor="ch" refForName="center" fact="0.1"/><dgm:constr type="sibSp" refType="w" refFor="ch" refForName="node" fact="0.1"/><dgm:constr type="primFontSz" for="des" forName="node" op="equ" val="65"/></dgm:constrLst><dgm:ruleLst/><dgm:layoutNode name="center" styleLbl="node0"><dgm:alg type="tx"/>${SHP('ellipse')}<dgm:presOf axis="self"/><dgm:constrLst>${TX()}</dgm:constrLst><dgm:ruleLst>${FONTRULE}</dgm:ruleLst></dgm:layoutNode><dgm:forEach name="N1" axis="ch" ptType="node"><dgm:layoutNode name="node" styleLbl="node1"><dgm:alg type="tx"/>${SHP('ellipse')}<dgm:presOf axis="desOrSelf" ptType="node"/><dgm:constrLst>${TX()}</dgm:constrLst><dgm:ruleLst>${FONTRULE}</dgm:ruleLst></dgm:layoutNode></dgm:forEach></dgm:layoutNode>`,
  },
  hierarchy: {
    id: 'orgChart1', kind: 'tree3', pres: ['root', 'composite'],
    body: (r) => `<dgm:layoutNode name="hierRoot1"><dgm:varLst><dgm:orgChart val="1"/><dgm:dir/><dgm:resizeHandles val="exact"/></dgm:varLst><dgm:alg type="hierRoot"><dgm:param type="hierAlign" val="${r.pick(['tCtrCh', 'tCtrCh', 'tL', 'tR', 'lCtrCh', 'bCtrCh'])}"/></dgm:alg>${SHP('')}<dgm:presOf/><dgm:constrLst><dgm:constr type="w" for="ch" forName="composite" refType="w"/><dgm:constr type="h" for="ch" forName="composite" refType="w" fact="0.12"/><dgm:constr type="sp" refType="h" refFor="ch" refForName="composite" fact="${f(r, 0.4, 0.9)}"/><dgm:constr type="primFontSz" for="des" forName="root" op="equ" val="65"/></dgm:constrLst><dgm:ruleLst/><dgm:forEach name="N0" axis="ch" ptType="node" cnt="1"><dgm:layoutNode name="composite"><dgm:alg type="composite"/>${SHP('')}<dgm:presOf/><dgm:constrLst><dgm:constr type="l" for="ch" forName="root"/><dgm:constr type="t" for="ch" forName="root"/><dgm:constr type="w" for="ch" forName="root" refType="w"/><dgm:constr type="h" for="ch" forName="root" refType="h"/></dgm:constrLst><dgm:ruleLst/><dgm:layoutNode name="root" styleLbl="node1"><dgm:alg type="tx"/>${SHP('rect')}<dgm:presOf axis="self"/><dgm:constrLst>${TX()}</dgm:constrLst><dgm:ruleLst>${FONTRULE}</dgm:ruleLst></dgm:layoutNode></dgm:layoutNode><dgm:layoutNode name="hc"><dgm:alg type="hierChild"><dgm:param type="linDir" val="fromL"/><dgm:param type="chAlign" val="t"/></dgm:alg>${SHP('')}<dgm:presOf/><dgm:constrLst><dgm:constr type="sibSp" refType="w" refFor="par" fact="0"/><dgm:constr type="sibSp" for="self" refType="w" fact="0.04"/></dgm:constrLst><dgm:ruleLst/><dgm:forEach name="N1" axis="ch" ptType="node"><dgm:layoutNode name="hr2"><dgm:alg type="hierRoot"/>${SHP('')}<dgm:presOf/><dgm:constrLst><dgm:constr type="w" for="ch" forName="composite2" refType="w"/><dgm:constr type="h" for="ch" forName="composite2" refType="w" fact="0.1"/></dgm:constrLst><dgm:ruleLst/><dgm:layoutNode name="composite2"><dgm:alg type="composite"/>${SHP('')}<dgm:presOf/><dgm:constrLst><dgm:constr type="w" for="ch" forName="root" refType="w"/><dgm:constr type="h" for="ch" forName="root" refType="h"/></dgm:constrLst><dgm:ruleLst/><dgm:layoutNode name="root" styleLbl="node2"><dgm:alg type="tx"/>${SHP('rect')}<dgm:presOf axis="self"/><dgm:constrLst>${TX()}</dgm:constrLst><dgm:ruleLst>${FONTRULE}</dgm:ruleLst></dgm:layoutNode></dgm:layoutNode><dgm:layoutNode name="hc2"><dgm:alg type="hierChild"/>${SHP('')}<dgm:presOf/><dgm:constrLst><dgm:constr type="sibSp" fact="0.1" refType="w" refFor="ch" refForName="hr3"/></dgm:constrLst><dgm:ruleLst/><dgm:forEach name="N2" axis="ch" ptType="node"><dgm:layoutNode name="hr3"><dgm:alg type="hierRoot"/>${SHP('')}<dgm:presOf/><dgm:constrLst><dgm:constr type="w" for="ch" forName="root3" refType="w"/><dgm:constr type="h" for="ch" forName="root3" refType="w" fact="0.1"/></dgm:constrLst><dgm:ruleLst/><dgm:layoutNode name="root3" styleLbl="node3"><dgm:alg type="tx"/>${SHP('rect')}<dgm:presOf axis="self"/><dgm:constrLst>${TX()}</dgm:constrLst><dgm:ruleLst>${FONTRULE}</dgm:ruleLst></dgm:layoutNode></dgm:layoutNode></dgm:forEach></dgm:layoutNode></dgm:layoutNode></dgm:forEach></dgm:layoutNode></dgm:forEach></dgm:layoutNode>`,
  },
  pyramid1: {
    id: 'pyramid1', kind: 'tree2', pres: ['level'],
    body: (r) => `<dgm:layoutNode name="Name0"><dgm:varLst><dgm:dir/><dgm:resizeHandles val="exact"/></dgm:varLst><dgm:alg type="pyra"><dgm:param type="linDir" val="${r.pick(['fromB', 'fromT'])}"/><dgm:param type="pyraAcctPos" val="${r.pick(['aft', 'bef'])}"/><dgm:param type="pyraLvlNode" val="level"/><dgm:param type="pyraAcctBkgdNode" val="descendantBackground"/><dgm:param type="pyraAcctTxNode" val="descendantText"/></dgm:alg>${SHP('')}<dgm:presOf/><dgm:constrLst><dgm:constr type="w" refType="w"/><dgm:constr type="pyraAcctRatio" val="${f(r, 0.3, 0.7)}"/><dgm:constr type="primFontSz" for="des" forName="levelText" op="equ" val="65"/></dgm:constrLst><dgm:ruleLst/><dgm:forEach name="N0" axis="ch" ptType="node"><dgm:layoutNode name="comp"><dgm:alg type="composite"/>${SHP('')}<dgm:presOf/><dgm:constrLst><dgm:constr type="l" for="ch" forName="level"/><dgm:constr type="w" for="ch" forName="levelText" refType="w" refFor="ch" refForName="level" fact="0.6"/><dgm:constr type="h" for="ch" forName="levelText" refType="h" refFor="ch" refForName="level" fact="0.8"/><dgm:constr type="t" for="ch" forName="levelText" refType="h" refFor="ch" refForName="level" fact="0.2"/></dgm:constrLst><dgm:ruleLst/><dgm:layoutNode name="level" styleLbl="node1"><dgm:alg type="sp"/>${SHP('trapezoid', '<dgm:adj idx="1" val="0.5"/>')}<dgm:presOf/><dgm:constrLst/><dgm:ruleLst/></dgm:layoutNode><dgm:layoutNode name="levelText"><dgm:alg type="tx"/><dgm:shape type="rect" r:blip="" hideGeom="1"><dgm:adjLst/></dgm:shape><dgm:presOf axis="self"/><dgm:constrLst>${TX()}</dgm:constrLst><dgm:ruleLst>${FONTRULE}</dgm:ruleLst></dgm:layoutNode><dgm:layoutNode name="descendantBackground" styleLbl="node2"><dgm:alg type="sp"/>${SHP('trapezoid')}<dgm:presOf axis="des" ptType="node"/><dgm:constrLst/><dgm:ruleLst/></dgm:layoutNode><dgm:layoutNode name="descendantText"><dgm:alg type="tx"/><dgm:shape type="rect" r:blip="" hideGeom="1"><dgm:adjLst/></dgm:shape><dgm:presOf axis="des" ptType="node"/><dgm:constrLst>${TX()}</dgm:constrLst><dgm:ruleLst>${FONTRULE}</dgm:ruleLst></dgm:layoutNode></dgm:layoutNode></dgm:forEach></dgm:layoutNode>`,
  },
  matrix1: {
    id: 'matrix1', kind: 'quad', pres: ['q1', 'q2'],
    body: (r) => `<dgm:layoutNode name="matrix"><dgm:varLst><dgm:chMax val="4"/><dgm:dir/><dgm:resizeHandles val="exact"/></dgm:varLst><dgm:alg type="composite"><dgm:param type="ar" val="${r.pick(['1', '1.2', '1.6'])}"/></dgm:alg>${SHP('')}<dgm:presOf/><dgm:constrLst><dgm:constr type="l" for="ch" forName="q1"/><dgm:constr type="t" for="ch" forName="q1"/><dgm:constr type="w" for="ch" forName="q1" refType="w" fact="0.48"/><dgm:constr type="h" for="ch" forName="q1" refType="h" fact="0.48"/><dgm:constr type="l" for="ch" forName="q2" refType="w" fact="0.52"/><dgm:constr type="t" for="ch" forName="q2"/><dgm:constr type="w" for="ch" forName="q2" refType="w" fact="0.48"/><dgm:constr type="h" for="ch" forName="q2" refType="h" fact="0.48"/><dgm:constr type="l" for="ch" forName="q3"/><dgm:constr type="t" for="ch" forName="q3" refType="h" fact="0.52"/><dgm:constr type="w" for="ch" forName="q3" refType="w" fact="0.48"/><dgm:constr type="h" for="ch" forName="q3" refType="h" fact="0.48"/><dgm:constr type="l" for="ch" forName="q4" refType="w" fact="0.52"/><dgm:constr type="t" for="ch" forName="q4" refType="h" fact="0.52"/><dgm:constr type="w" for="ch" forName="q4" refType="w" fact="0.48"/><dgm:constr type="h" for="ch" forName="q4" refType="h" fact="0.48"/><dgm:constr type="primFontSz" for="des" forName="q1" op="equ" val="65"/></dgm:constrLst><dgm:ruleLst/>${[1, 2, 3, 4].map((k) => `<dgm:forEach name="F${k}" axis="ch" ptType="node" st="${k}" cnt="1"><dgm:layoutNode name="q${k}" styleLbl="node1"><dgm:alg type="tx"/>${SHP(r.pick(['rect', 'roundRect', 'round1Rect']))}<dgm:presOf axis="desOrSelf" ptType="node"/><dgm:constrLst>${TX()}</dgm:constrLst><dgm:ruleLst>${FONTRULE}</dgm:ruleLst></dgm:layoutNode></dgm:forEach>`).join('')}</dgm:layoutNode>`,
  },
  chevron1: {
    id: 'chevron1', kind: 'tree2', pres: ['parTxOnly'],
    body: (r) => `<dgm:layoutNode name="Name0"><dgm:varLst><dgm:dir/><dgm:animLvl val="lvl"/><dgm:resizeHandles val="exact"/></dgm:varLst><dgm:alg type="lin"><dgm:param type="linDir" val="${r.pick(['fromL', 'fromL', 'fromR'])}"/><dgm:param type="nodeVertAlign" val="${r.pick(['mid', 't', 'b'])}"/></dgm:alg>${SHP('')}<dgm:presOf/><dgm:constrLst><dgm:constr type="w" for="ch" forName="parTxOnly" refType="w"/><dgm:constr type="h" for="ch" forName="parTxOnly" refType="w" refFor="ch" refForName="parTxOnly" fact="${f(r, 0.3, 0.5)}"/><dgm:constr type="w" for="ch" forName="parTxOnlySpace" refType="w" refFor="ch" refForName="parTxOnly" fact="-0.1"/><dgm:constr type="primFontSz" for="ch" forName="parTxOnly" op="equ" val="65"/></dgm:constrLst><dgm:ruleLst/><dgm:forEach name="N1" axis="ch" ptType="node"><dgm:layoutNode name="parTxOnly" styleLbl="node1"><dgm:varLst><dgm:chMax val="0"/><dgm:bulletEnabled val="1"/></dgm:varLst><dgm:alg type="tx"/>${SHP('chevron', '<dgm:adj idx="1" val="0.4"/>')}<dgm:presOf axis="desOrSelf" ptType="node"/><dgm:constrLst><dgm:constr type="lMarg" refType="primFontSz" fact="0.2"/><dgm:constr type="rMarg" refType="primFontSz" fact="0.2"/></dgm:constrLst><dgm:ruleLst><dgm:rule type="primFontSz" val="5" fact="NaN" max="NaN"/></dgm:ruleLst></dgm:layoutNode><dgm:forEach name="N2" axis="followSib" ptType="sibTrans" cnt="1"><dgm:layoutNode name="parTxOnlySpace"><dgm:alg type="sp"/>${SHP('')}<dgm:presOf/><dgm:constrLst/><dgm:ruleLst/></dgm:layoutNode></dgm:forEach></dgm:forEach></dgm:layoutNode>`,
  },
  hList1: {
    id: 'hList1', kind: 'tree2', pres: ['composite'],
    body: (r) => `<dgm:layoutNode name="Name0"><dgm:varLst><dgm:dir/><dgm:resizeHandles val="exact"/></dgm:varLst><dgm:alg type="lin"><dgm:param type="linDir" val="fromL"/><dgm:param type="nodeVertAlign" val="t"/></dgm:alg>${SHP('')}<dgm:presOf/><dgm:constrLst><dgm:constr type="w" for="ch" forName="composite" refType="w"/><dgm:constr type="h" for="ch" forName="composite" refType="h"/><dgm:constr type="w" for="ch" forName="space" refType="w" refFor="ch" refForName="composite" fact="0.04"/><dgm:constr type="primFontSz" for="des" forName="parTx" op="equ" val="65"/><dgm:constr type="primFontSz" for="des" forName="desTx" op="equ" val="65"/></dgm:constrLst><dgm:ruleLst/><dgm:forEach name="N0" axis="ch" ptType="node"><dgm:layoutNode name="composite"><dgm:alg type="composite"/>${SHP('')}<dgm:presOf/><dgm:constrLst><dgm:constr type="l" for="ch" forName="parTx"/><dgm:constr type="w" for="ch" forName="parTx" refType="w"/><dgm:constr type="h" for="ch" forName="parTx" refType="h" fact="0.2"/><dgm:constr type="t" for="ch" forName="desTx" refType="h" fact="0.22"/><dgm:constr type="w" for="ch" forName="desTx" refType="w"/><dgm:constr type="h" for="ch" forName="desTx" refType="h" fact="0.78"/></dgm:constrLst><dgm:ruleLst/><dgm:layoutNode name="parTx" styleLbl="node1"><dgm:alg type="tx"/>${SHP('rect')}<dgm:presOf axis="self"/><dgm:constrLst>${TX()}</dgm:constrLst><dgm:ruleLst><dgm:rule type="primFontSz" val="5" fact="NaN" max="NaN"/></dgm:ruleLst></dgm:layoutNode><dgm:layoutNode name="desTx" styleLbl="alignAccFollowNode1"><dgm:varLst><dgm:bulletEnabled val="1"/></dgm:varLst><dgm:alg type="tx"><dgm:param type="stBulletLvl" val="1"/><dgm:param type="txAnchorVert" val="t"/></dgm:alg>${SHP('rect')}<dgm:presOf axis="des" ptType="node"/><dgm:constrLst>${TX(0.2)}</dgm:constrLst><dgm:ruleLst><dgm:rule type="primFontSz" val="5" fact="NaN" max="NaN"/></dgm:ruleLst></dgm:layoutNode></dgm:layoutNode><dgm:forEach name="N1" axis="followSib" ptType="sibTrans" cnt="1"><dgm:layoutNode name="space"><dgm:alg type="sp"/>${SHP('')}<dgm:presOf/><dgm:constrLst/><dgm:ruleLst/></dgm:layoutNode></dgm:forEach></dgm:forEach></dgm:layoutNode>`,
  },
};
// layouts the fallback knows by uniqueId (an unsupported algorithm in the definition forces the fallback)
const FALLBACK_IDS = ['orgChart1', 'process5', 'process1', 'list1', 'chevron1', 'vList3', 'hList1', 'hierarchy4', 'vList5', 'vProcess5', 'cycle2', 'default', 'matrix1'];

const SCHEMES = ['accent1', 'accent2', 'accent3', 'accent4', 'accent5', 'accent6', 'dk2', 'lt1', 'tx1'];
const LBLS = ['node0', 'node1', 'node2', 'node3', 'node4', 'alignNode1', 'lnNode1', 'vennNode1', 'asst1', 'asst2', 'sibTrans2D1', 'sibTrans1D1', 'parChTrans1D1', 'parChTrans1D2', 'parChTrans1D3', 'parChTrans2D1', 'fgAcc1', 'alignAcc1', 'bgAcc1', 'revTx', 'fgAccFollowNode1', 'alignAccFollowNode1', 'bgAccFollowNode1', 'bgShp', 'fgShp', 'fgImgPlace1'];

function clr(r) {
  const mods = r.pickN(['<a:tint val="60000"/>', '<a:shade val="80000"/>', '<a:alpha val="50000"/>', '<a:hueOff val="0"/><a:satOff val="0"/><a:lumOff val="0"/><a:alphaOff val="0"/>', '<a:lumMod val="60000"/><a:lumOff val="40000"/>', '<a:alpha val="90000"/><a:tint val="40000"/>'], r.chance(0.5) ? 1 : 0).join('');
  return r.chance(0.12) ? `<a:srgbClr val="${r.hex6()}">${mods}</a:srgbClr>` : `<a:schemeClr val="${r.pick(SCHEMES)}">${mods}</a:schemeClr>`;
}
function clrLst(r, tag) {
  const n = r.pick([1, 1, 2, 3, 4]);
  const meth = r.pick(['repeat', 'span', 'span', 'cycle']);
  return `<dgm:${tag} meth="${meth}"${r.chance(0.4) ? ` hueDir="${r.pick(['cw', 'ccw'])}"` : ''}>${Array.from({ length: n }, () => clr(r)).join('')}</dgm:${tag}>`;
}
function colorsXml(r) {
  const lbls = LBLS.filter(() => r.chance(0.85)).map((l) => `<dgm:styleLbl name="${l}">${clrLst(r, 'fillClrLst')}${clrLst(r, 'linClrLst')}<dgm:effectClrLst/><dgm:txLinClrLst/>${clrLst(r, 'txFillClrLst')}<dgm:txEffectClrLst/></dgm:styleLbl>`);
  return `${XML}<dgm:colorsDef ${DGM} uniqueId="urn:microsoft.com/office/officeart/2005/8/colors/gen${r.below(9)}"><dgm:title val=""/><dgm:desc val=""/><dgm:catLst><dgm:cat type="accent1" pri="11200"/></dgm:catLst>${lbls.join('')}</dgm:colorsDef>`;
}
function styleXml(r) {
  const lbls = LBLS.filter(() => r.chance(0.85)).map((l) => `<dgm:styleLbl name="${l}"><dgm:scene3d><a:camera prst="orthographicFront"/><a:lightRig rig="threePt" dir="t"/></dgm:scene3d><dgm:sp3d/><dgm:txPr/><dgm:style><a:lnRef idx="${r.int(0, 3)}"><a:scrgbClr r="0" g="0" b="0"/></a:lnRef><a:fillRef idx="${r.pick([0, 1, 1, 1, 2, 3])}"><a:scrgbClr r="0" g="0" b="0"/></a:fillRef><a:effectRef idx="${r.int(0, 2)}"><a:scrgbClr r="0" g="0" b="0"/></a:effectRef><a:fontRef idx="minor"><a:schemeClr val="${r.pick(['lt1', 'dk1', 'tx1'])}"/></a:fontRef></dgm:style></dgm:styleLbl>`);
  return `${XML}<dgm:styleDef ${DGM} uniqueId="urn:microsoft.com/office/officeart/2005/8/quickstyle/simple${r.int(1, 5)}"><dgm:title val=""/><dgm:desc val=""/><dgm:catLst><dgm:cat type="simple" pri="10100"/></dgm:catLst><dgm:scene3d><a:camera prst="orthographicFront"/><a:lightRig rig="threePt" dir="t"/></dgm:scene3d>${lbls.join('')}</dgm:styleDef>`;
}

function textOf(r, custom) {
  const n = r.weighted([[1, 3], [2, 4], [3, 2], [6, 1], [14, 1]]);
  const text = words(r, n, 0.1).text;
  const run = (t, sz) => `<a:r><a:rPr lang="en-US"${sz ? ` sz="${sz}" b="${r.below(2)}"` : ''}${r.chance(0.1) ? ' i="1"' : ''}/><a:t>${esc(t)}</a:t></a:r>`;
  let p = `<a:p>${run(text, custom ? r.pick([1200, 1800, 2400, 3200]) : 0)}</a:p>`;
  if (r.chance(0.15)) p = p.replace('<a:p>', `<a:p><a:pPr algn="${r.pick(['l', 'ctr', 'r'])}">${r.chance(0.4) ? `<a:spcBef><a:spcPct val="${r.int(0, 60000)}"/></a:spcBef>` : '<a:lnSpc><a:spcPts val="1800"/></a:lnSpc>'}</a:pPr>`);
  if (r.chance(0.12)) p += `<a:p>${run(words(r, 2).text, custom ? 1400 : 0)}</a:p>`;
  return { xml: `<dgm:t><a:bodyPr/><a:lstStyle/>${p}</dgm:t>`, custom };
}

/** The data model: [{id, parent, kids}] by kind, as dgm:pt / dgm:cxn lists (with trans points and pres points). */
function dataXml(r, fam, ids) {
  const kind = fam.kind;
  const nodes = [];
  const mk = (parent, type = 'node') => { const n = { id: `{N${nodes.length}-${r.hex6()}}`, parent, type, kids: [], par: `{P${nodes.length}}`, sib: `{S${nodes.length}}`, cx: `{C${nodes.length}}`, ord: 0 }; nodes.push(n); if (parent) { n.ord = parent.kids.length; parent.kids.push(n); } return n; };
  const doc = { id: '{DOC}', parent: null, kids: [], type: 'doc' };
  const top = (n) => { for (let i = 0; i < n; i++) mk(doc); };
  if (kind === 'list') top(r.int(2, 9));
  else if (kind === 'quad') top(r.int(1, 4));
  else if (kind === 'tree2') { top(r.int(1, 6)); for (const t of [...doc.kids]) if (r.chance(0.6)) for (let i = r.int(1, 3); i > 0; i--) mk(t); }
  else {
    top(1);
    const lvl1 = [];
    for (let i = r.int(1, 4); i > 0; i--) lvl1.push(mk(doc.kids[0], r.chance(0.12) ? 'asst' : 'node'));
    for (const t of lvl1) if (r.chance(0.7)) for (let i = r.int(1, 3); i > 0; i--) mk(t);
  }
  const custom = r.chance(0.2);
  const pts = [`<dgm:pt modelId="${doc.id}" type="doc"><dgm:prSet loTypeId="urn:microsoft.com/office/officeart/2005/8/layout/${fam.id}" loCatId="list" qsTypeId="urn:microsoft.com/office/officeart/2005/8/quickstyle/simple1" qsCatId="simple" csTypeId="urn:microsoft.com/office/officeart/2005/8/colors/accent1_2" csCatId="accent1" phldr="0"/><dgm:spPr/><dgm:t><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang="en-US"/></a:p></dgm:t></dgm:pt>`];
  const cxns = [];
  const presPts = [];
  let cid = 0;
  for (const n of nodes) {
    const t = textOf(r, custom);
    const sp = r.chance(0.12) ? `<dgm:spPr><a:solidFill><a:srgbClr val="${r.hex6()}"/></a:solidFill>${r.chance(0.5) ? '<a:ln w="19050"><a:solidFill><a:schemeClr val="accent2"/></a:solidFill></a:ln>' : ''}</dgm:spPr>` : '<dgm:spPr/>';
    pts.push(`<dgm:pt modelId="${n.id}"${n.type === 'asst' ? ' type="asst"' : ''}><dgm:prSet${custom ? ' custT="1"' : ''}${r.chance(0.1) ? ' phldrT="[Text]"' : ''}/>${sp}${t.xml}</dgm:pt>`);
    pts.push(`<dgm:pt modelId="${n.par}" type="parTrans" cxnId="${n.cx}"><dgm:prSet/><dgm:spPr/><dgm:t><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang="en-US"/></a:p></dgm:t></dgm:pt>`);
    pts.push(`<dgm:pt modelId="${n.sib}" type="sibTrans" cxnId="${n.cx}"><dgm:prSet/><dgm:spPr/><dgm:t><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang="en-US"/></a:p></dgm:t></dgm:pt>`);
    const parent = n.parent ?? doc;
    cxns.push(`<dgm:cxn modelId="${n.cx}" srcId="${parent.id}" destId="${n.id}" srcOrd="${n.ord}" destOrd="0" parTransId="${n.par}" sibTransId="${n.sib}"/>`);
    // presentation points: user adjustments on the nodes the layout names
    if (r.chance(0.35) && fam.pres.length) {
      const name = r.pick(fam.pres);
      const a = [];
      if (r.chance(0.5)) a.push(`custScaleX="${r.int(40000, 160000)}"`);
      if (r.chance(0.5)) a.push(`custScaleY="${r.int(40000, 160000)}"`);
      if (r.chance(0.2)) a.push(`custLinFactNeighborX="${r.int(-30000, 30000)}"`);
      if (r.chance(0.2)) a.push(`custLinFactNeighborY="${r.int(-30000, 30000)}"`);
      const lv = r.chance(0.3) ? `<dgm:presLayoutVars>${r.chance(0.5) ? '<dgm:bulletEnabled val="1"/>' : ''}${r.chance(0.4) ? `<dgm:hierBranch val="${r.pick(['std', 'hang', 'init', 'r', 'l'])}"/>` : ''}</dgm:presLayoutVars>` : '';
      presPts.push(`<dgm:pt modelId="{PR${cid++}}" type="pres"><dgm:prSet presAssocID="${n.id}" presName="${name}" presStyleLbl="${r.pick(['node1', 'node2', 'revTx'])}" presStyleIdx="${r.below(3)}" presStyleCnt="${r.int(1, 6)}" ${a.join(' ')}>${lv}</dgm:prSet>${r.chance(0.3) ? `<dgm:spPr><a:solidFill><a:schemeClr val="accent${r.int(1, 6)}"/></a:solidFill></dgm:spPr>` : '<dgm:spPr/>'}</dgm:pt>`);
    }
  }
  // a few hostile-ish quirks: a duplicate connection, a connection to a missing node, a cycle
  if (r.chance(0.08) && nodes.length > 1) cxns.push(`<dgm:cxn modelId="{X1}" srcId="${nodes[1].id}" destId="${nodes[0].id}" srcOrd="0" destOrd="0"/>`);
  if (r.chance(0.05)) cxns.push(`<dgm:cxn modelId="{X2}" srcId="${doc.id}" destId="{MISSING}" srcOrd="9" destOrd="0"/>`);
  if (r.chance(0.1)) cxns.push(`<dgm:cxn modelId="{X3}" type="presOf" srcId="${nodes[0].id}" destId="{PR0}" srcOrd="0" destOrd="0"/>`);
  ids.nodes = nodes;
  return `${XML}<dgm:dataModel ${DGM}><dgm:ptLst>${pts.join('')}${presPts.join('')}</dgm:ptLst><dgm:cxnLst>${cxns.join('')}</dgm:cxnLst><dgm:bg/><dgm:whole/></dgm:dataModel>`;
}

function layoutDef(r, fam, unsupported) {
  const id = unsupported ? r.pick(FALLBACK_IDS) : fam.id;
  const body = unsupported ? `<dgm:layoutNode name="diagram"><dgm:alg type="venn"/>${SHP('')}<dgm:presOf/><dgm:constrLst/><dgm:ruleLst/></dgm:layoutNode>` : fam.body(r);
  return { id, xml: `${XML}<dgm:layoutDef ${DGM} uniqueId="urn:microsoft.com/office/officeart/2005/8/layout/${id}"><dgm:title val=""/><dgm:desc val=""/><dgm:catLst><dgm:cat type="list" pri="400"/></dgm:catLst>${body}</dgm:layoutDef>` };
}

function drawingXml(r, w, h, n) {
  const sps = Array.from({ length: n }, (_, i) => {
    const x = Math.round(w * 0.05 + (i * w * 0.9) / n), cw = Math.round((w * 0.8) / n);
    return `<dsp:sp modelId="{D${i}}"><dsp:nvSpPr><dsp:cNvPr id="0" name=""/><dsp:cNvSpPr/></dsp:nvSpPr><dsp:spPr><a:xfrm><a:off x="${x}" y="${Math.round(h * 0.3)}"/><a:ext cx="${cw}" cy="${Math.round(h * 0.4)}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></dsp:spPr><dsp:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US" sz="1800"/><a:t>${esc(words(r, 2).text)}</a:t></a:r></a:p></dsp:txBody><dsp:txXfrm><a:off x="${x}" y="${Math.round(h * 0.3)}"/><a:ext cx="${cw}" cy="${Math.round(h * 0.4)}"/></dsp:txXfrm></dsp:sp>`;
  }).join('');
  return `${XML}<dsp:drawing xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" xmlns:dsp="http://schemas.microsoft.com/office/drawing/2008/diagram" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><dsp:spTree><dsp:nvGrpSpPr><dsp:cNvPr id="0" name=""/><dsp:cNvGrpSpPr/></dsp:nvGrpSpPr><dsp:grpSpPr/>${sps}</dsp:spTree></dsp:drawing>`;
}

const DT = 'application/vnd.openxmlformats-officedocument.drawingml.diagram';
const TYPES = { data: `${DT}Data+xml`, layout: `${DT}Layout+xml`, quickStyle: `${DT}Style+xml`, colors: `${DT}Colors+xml`, drawing: 'application/vnd.ms-office.drawingml.diagramDrawing+xml' };

class Ctx {
  constructor(r, features) { this.r = r; this.features = features; this.rels = []; this.id = 1; }
  feat(t) { this.features.add(t); }
  nextId() { return ++this.id; }
  addImage() { throw new Error('b5a: no pictures'); }
  addLink() { throw new Error('b5a: no links'); }
}

/** Builds deck number `seed`: { bytes, features, slides }. */
export function buildSmartArtDeck(seed, profile) {
  const r = new Rand(seed);
  const features = new Set();
  const base = new Ctx(r, features);
  const t = makeTemplate(base);
  const specs = layoutSpecs(t).slice(0, 3).concat(layoutSpecs(t).slice(4));
  const files = [];
  const add = (name, data, opt) => files.push([name, data, opt]);
  const layoutRIds = specs.map((_, i) => `rId${i + 2}`);
  const master = masterXml(new Ctx(r, features), t, layoutRIds);
  const layouts = specs.map((s, i) => layoutXml(new Ctx(r, features), t, s, i));
  const nSlides = r.int(profile.slides[0], profile.slides[1]);
  const slideParts = [];
  const diag = []; // diagram parts [name, xml]
  let dn = 0;
  for (let si = 0; si < nSlides; si++) {
    const rels = [['rId1', 'slideLayout', `../slideLayouts/slideLayout${specs.length}.xml`]];
    const frames = [];
    for (let fi = r.int(1, 3); fi > 0; fi--) {
      dn++;
      const famName = r.pick(Object.keys(FAMILIES));
      const fam = FAMILIES[famName];
      const mode = r.weighted([['regen', 50], ['drawing', 14], ['emptyDrawing', 6], ['fallback', 20], ['nolayout', 3], ['nodata', 2], ['guess', 5]]);
      features.add(`b5a:${famName}`); features.add(`b5a:${mode}`);
      const fw = r.pick([Math.round(t.w * 0.9), Math.round(t.w * 0.5), 5486400, 8128000, 3000000]);
      const fh = r.pick([Math.round(t.h * 0.8), Math.round(t.h * 0.4), 3657600, 5418667, 1800000]);
      const ids = {};
      const unsupported = mode === 'fallback' || r.chance(0.04);
      const lo = layoutDef(r, fam, unsupported);
      const data = dataXml(r, fam, ids);
      const base = (n) => `ppt/diagrams/${n}${dn}.xml`;
      const rid = (k) => `rId${rels.length + 1 + k}`;
      const entries = [['dm', 'diagramData', 'data', data]];
      if (mode !== 'nolayout') entries.push(['lo', 'diagramLayout', 'layout', lo.xml]);
      entries.push(['qs', 'diagramQuickStyle', 'quickStyle', r.chance(0.9) ? styleXml(r) : null], ['cs', 'diagramColors', 'colors', r.chance(0.9) ? colorsXml(r) : null]);
      const attrs = [];
      for (const [k, type, part, xml] of entries) {
        if (xml === null) continue;
        const id = `rId${rels.length + 1}`;
        rels.push([id, type, `../diagrams/${part}${dn}.xml`]);
        diag.push([`ppt/diagrams/${part}${dn}.xml`, xml, part]);
        attrs.push(`r:${k}="${id}"`);
      }
      if (mode === 'nodata') attrs.shift();
      if (mode === 'drawing' || mode === 'emptyDrawing' || mode === 'guess') {
        diag.push([`ppt/diagrams/drawing${dn}.xml`, mode === 'emptyDrawing' ? drawingXml(r, fw, fh, 0) : drawingXml(r, fw, fh, r.int(1, 4)), 'drawing']);
        if (mode !== 'guess') {
          const did = `rId${rels.length + 1}`;
          rels.push([did, 'http://schemas.microsoft.com/office/2007/relationships/diagramDrawing', `../diagrams/drawing${dn}.xml`]);
        }
      }
      void rid; void base;
      const id = 10 + frames.length;
      frames.push(`<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="${id}" name="Diagram ${id}"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="${r.int(0, 600000)}" y="${r.int(0, 600000)}"/><a:ext cx="${fw}" cy="${fh}"/></p:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/diagram"><dgm:relIds xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" ${attrs.join(' ')}/></a:graphicData></a:graphic></p:graphicFrame>`);
    }
    slideParts.push({ xml: `${XML}<p:sld ${NS}><p:cSld><p:spTree>${GRP}${frames.join('')}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>`, rels });
  }
  const presRels = [['rId1', 'slideMaster', 'slideMasters/slideMaster1.xml']];
  const slideRIds = slideParts.map((_, i) => { presRels.push([`rId${i + 2}`, 'slide', `slides/slide${i + 1}.xml`]); return `rId${i + 2}`; });
  const after = slideParts.length + 2;
  presRels.push([`rId${after}`, 'presProps', 'presProps.xml'], [`rId${after + 1}`, 'tableStyles', 'tableStyles.xml'], [`rId${after + 2}`, 'theme', 'theme/theme1.xml']);
  const parts = ['ppt/presentation.xml', 'ppt/presProps.xml', 'ppt/tableStyles.xml', 'ppt/theme/theme1.xml', 'ppt/slideMasters/slideMaster1.xml', ...layouts.map((_, i) => `ppt/slideLayouts/slideLayout${i + 1}.xml`), ...slideParts.map((_, i) => `ppt/slides/slide${i + 1}.xml`)];
  let ct = contentTypes(parts);
  ct = ct.replace('</Types>', `${diag.map(([n, , k]) => `<Override PartName="/${n}" ContentType="${TYPES[k]}"/>`).join('')}</Types>`);
  add('[Content_Types].xml', ct);
  add('_rels/.rels', relsXml([['rId1', 'officeDocument', 'ppt/presentation.xml'], ['rId2', 'http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties', 'docProps/core.xml'], ['rId3', 'extended-properties', 'docProps/app.xml']]));
  add('docProps/app.xml', appXml(t, nSlides));
  add('docProps/core.xml', coreXml(`Generated SmartArt deck ${seed}`));
  add('ppt/presentation.xml', presentationXml(t, slideRIds, { masterRId: 'rId1', notesMasterRId: null }));
  add('ppt/_rels/presentation.xml.rels', relsXml(presRels));
  add('ppt/presProps.xml', presPropsXml(t));
  add('ppt/tableStyles.xml', tableStylesXml());
  add('ppt/theme/theme1.xml', themeXml(t));
  add('ppt/slideMasters/slideMaster1.xml', master);
  add('ppt/slideMasters/_rels/slideMaster1.xml.rels', relsXml([['rId1', 'theme', '../theme/theme1.xml'], ...layoutRIds.map((rid, i) => [rid, 'slideLayout', `../slideLayouts/slideLayout${i + 1}.xml`])]));
  layouts.forEach((xml, i) => {
    add(`ppt/slideLayouts/slideLayout${i + 1}.xml`, xml);
    add(`ppt/slideLayouts/_rels/slideLayout${i + 1}.xml.rels`, relsXml([['rId1', 'slideMaster', '../slideMasters/slideMaster1.xml']]));
  });
  slideParts.forEach((s, i) => {
    add(`ppt/slides/slide${i + 1}.xml`, s.xml);
    add(`ppt/slides/_rels/slide${i + 1}.xml.rels`, relsXml(s.rels));
  });
  for (const [n, xml] of diag) add(n, xml);
  return { bytes: zipBytes(files), features: [...features].sort(), slides: nSlides };
}
