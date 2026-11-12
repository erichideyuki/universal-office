// Types of js/session-stories.mjs (wave 5, frontier S; record core/docs/modules/uo-bindings.md "Wave 5 — S: stories").
// The answers are the JSON of src/session/shell_stories.rs; `ExecOutcome` is the generated one of ./gen/session.ts.

import type { ExecOutcome } from './gen/session.ts';
import type { WriteBinding, WriteLayout } from './write.js';
import type { WriteSession } from './session.js';

/** A cell hop of the TS (`CellHop`: row, grid column, the block of the next level). */
export interface StoryHop { r: number; c: number; b?: number }
export interface StoryRect { r1: number; c1: number; r2: number; c2: number }

export type StoryCtx =
  | { kind: 'body' }
  | { kind: 'cell'; anchor: number; hops: StoryHop[] }
  | { kind: 'note'; footnote: boolean; id: number }
  | { kind: 'hf'; hfKind: 'header' | 'footer'; part: number; page: number }
  | { kind: 'textbox'; para: number | null; run: number | null; at: string; host: number; focus: number; page: number; outer: StoryCtx };

export interface StoryContext {
  ctx: StoryCtx;
  tableSel: { anchor: number; path: StoryHop[]; rect: StoryRect; origin: { r: number; c: number } } | null;
  locked: boolean;
}

export type StoryLocation =
  | { kind: 'body'; line: number; para: number }
  | { kind: 'cell'; line: number; anchor: number; hops: StoryHop[]; block: number }
  | { kind: 'hf'; line: number; which: 'hdr' | 'ftr'; slot: 'default' | 'first' | 'even'; part: number }
  | { kind: 'note'; line: number; footnote: boolean; id: number }
  | { kind: 'other'; line: number };

export interface StoryCellBox { anchor: number; path: StoryHop[]; r: number; c: number; x: number; y: number; w: number; h: number; span: number }

export interface StoryTableState {
  rows: number;
  cols: number;
  rect: StoryRect;
  cells: boolean;
  nested: boolean;
  canMerge: boolean;
  style?: string;
  look: Record<string, boolean>;
  vAlign: string;
  textDirection: string;
  headerRow: boolean;
  rowHeight: { twips: number; rule: string };
  colWidth: number;
  shading?: string;
  tblPr: Record<string, unknown>;
  trPr: Record<string, unknown>;
  tcPr: Record<string, unknown>;
  styleBorders?: Record<string, unknown>;
  availTwips: number;
  anchor: number;
  path: StoryHop[];
  layoutIn: { rowHeights: number[] };
}

export interface StoryBox { x: number; y: number; w: number; h: number }

export interface StoryHfSide {
  slot: 'default' | 'first' | 'even';
  /** the section that defines the part (or the page's: a new part is created there) */
  section: number;
  part: number | null;
  top: number;
  bottom: number;
  linked: boolean;
}

export interface StoryHfPage {
  page: number;
  section: number;
  sections: number;
  number: number;
  evenOdd: boolean;
  /** the variant of the Word rule (`hfTypeOfPage`) */
  type: 'default' | 'first' | 'even';
  width: number;
  height: number;
  marginLeft?: number;
  marginRight?: number;
  marginTop?: number;
  marginBottom?: number;
  header: StoryHfSide;
  footer: StoryHfSide;
}

export interface StoryNoteRun { ref: { footnote: boolean; id: number } | null; mark: boolean }

export interface StoryTextbox {
  /** the chain head's top-level run (null: in a cell, a group or a box; wave 6) */
  para: number | null;
  run: number | null;
  host: number;
  focus: number;
  page: number;
  rect: StoryBox;
  rot: number;
  onBorder: boolean;
  hf: boolean;
}

export type StoryTextSeparator = 'paragraph' | 'tab' | 'semicolon' | 'comma' | { char: string };

export interface StoriesBinding {
  storyContext(s: WriteSession): StoryContext;
  locateAt(s: WriteSession, layout: WriteLayout, page: number, x: number, y: number): StoryLocation | null;
  cellAt(s: WriteSession, layout: WriteLayout, page: number, x: number, y: number): StoryCellBox | null;
  tableState(s: WriteSession, layout: WriteLayout | null): StoryTableState | null;
  tableSelRects(s: WriteSession, layout: WriteLayout, page: number): StoryBox[];
  cellBoxes(layout: WriteLayout, page: number): StoryBox[];
  tableNavigate(s: WriteSession, layout: WriteLayout, dir: 'left' | 'right' | 'up' | 'down', extend?: boolean): ExecOutcome | null;
  moveCell(s: WriteSession, layout: WriteLayout, dir: number): ExecOutcome | null;
  dragCellSel(s: WriteSession, layout: WriteLayout, page: number, x: number, y: number): ExecOutcome | null;
  hfPage(s: WriteSession, layout: WriteLayout, page: number): StoryHfPage | null;
  noteAt(s: WriteSession, para: number, offset: number): StoryNoteRun | null;
  textboxAt(s: WriteSession, layout: WriteLayout, page: number, x: number, y: number, tol: number): StoryTextbox | null;
  sortInfo(s: WriteSession, separator?: 'tab' | 'comma' | { char: string }): { kind: 'table' | 'paragraphs'; columns: { text: string; guess: 'text' | 'number' | 'date' }[]; header: boolean; mergeError: boolean } | null;
  textToTableGuess(s: WriteSession, separator?: StoryTextSeparator, cols?: number): { separator: StoryTextSeparator; cols: number; rows: number } | null;
}

export declare function bindStories(exports: WebAssembly.Exports, uo: WriteBinding): StoriesBinding;
