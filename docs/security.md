# Security — untrusted documents

Maintenance notes on opening third-party documents (`.docx`/`.doc`, `.xlsx`/`.xls`, `.pptx`/`.ppt`, `.odt`) in the
web components and on the server. Written on 2026-11-06 from a reading of the code (branch `rust-core`, whose viewers
are the same as `main`'s); every finding names its file and line.

## Is an antivirus required before showing a file in a viewer?

**No, not for showing it in the browser** (`<universal-document-viewer>`, `<universal-slides-viewer>`,
`<universal-sheet>`, `<universal-write>`, `<universal-slides>`). What makes an Office file dangerous is what **Office**
does with it, and the components do none of it:

- **VBA macros** (`.docm`/`.pptm`/`.xlsm`) are never run (Write only preserves `vbaProject.bin`);
- **embedded OLE objects**: only the preview picture stored in the file is drawn; the object is never activated
  (Slides: `slides-core/src/read.ts`, `p:oleObj`);
- **DDE / executable fields**: fields are text computed by the engine;
- **memory-corruption exploits of Office's native parsers**: the parsers are TypeScript (and safe Rust in the new
  core) inside the browser sandbox — a malformed file ends in an exception or an error, not in code execution;
- **network**: the components fetch no external resource named by the file (linked images/media are not
  downloaded); the only `fetch` calls are the document itself (`src` attribute) and the demos'.

## Real risks and what mitigates them

1. **Hyperlinks with a dangerous scheme** — a file can carry `Target="javascript:…"` (or `data:`, `file:`…) in a link.
   A click may only open `http:`, `https:` and `mailto:`.
   - Write already filters: `write/packages/viewer/src/universal-document-viewer.ts` (`SAFE_LINK`) and
     `write/packages/editor/src/external-link.ts` (`EXTERNAL_LINK`).
   - **Slides viewer does NOT filter**: `slides/packages/viewer/src/universal-slides-viewer.ts:1161`
     (`window.open(hit, '_blank', 'noopener')` with the file's target). **Open finding.**
   - **Sheet does NOT filter**: `sheet/packages/sheet/src/universal-sheet.ts:1559`
     (`window.open(hl.target, '_blank', 'noopener')`). **Open finding.**
   - Fix: Write's rule (`/^(https?|mailto):/i`), preferably as one helper in `shared/ui`.
2. **Denial of service** — a zip bomb, a huge image, a document built to burn CPU or memory: it freezes the tab (or
   the process, on a server). An antivirus does not catch it. Mitigation: size, decompression and work limits; the
   Rust core applies limits to every untrusted input (`uo-opc` `Limits`, `DocxLimits`, `LayoutLimits`, `PdfLimits`,
   work budgets). On a server, run conversions isolated, with time and memory caps.
3. **Phishing** — a plain `https://` link to a malicious site. Not an antivirus matter; it is the user's (or an
   allow-list of domains in the host application).

## When an antivirus does make sense

- the platform **stores the original and lets other people download it** to open in Office (macros, OLE and Office
  exploits matter again) — scanning on upload is defence in depth;
- a **compliance policy** requires every upload to be scanned;
- **server-side processing** (conversion, extraction): isolation and limits come first; the antivirus is extra.

## Content Security Policy of the host page

WebAssembly needs `'wasm-unsafe-eval'` in a CSP that restricts `script-src` (it allows compiling WebAssembly only;
`eval()`/`new Function()` stay blocked). Supported from Chrome/Edge 97, Firefox 102 and Safari/iOS 16.0; **Safari 15
does not know it** (a strict policy there needs `'unsafe-eval'`). Today only the optional spell checker is WebAssembly
(plus `worker-src blob:` for its Worker — `write` and `@universal-office/spellcheck` READMEs). With 0.10 the Rust
engine is WebAssembly too; the plan and the plain-JS `wasm2js` fallback for hosts that cannot allow it are in
`core/docs/modules/uo-bindings.md` (TS/Rust switch; "No-WebAssembly fallback").

## Sanitizer and importer findings (.NET)

The security review of the Editing API port (D3, `core/docs/modules/uo-docx-d3.md` §9) found points that hold for the
**published C#** (`UniversalOffice.Docx` 0.9.x), not only for the port: **S1** — a `javascript:` hyperlink without
`TargetMode="External"` survives the sanitizer; **S6** — the importer recurses per chained style, list or note, so a
hostile document overflows the stack and kills a server process (the Rust port bounds the depth at 256). S2–S5, S7
and S8 are in the same record. `main` is frozen for the port; fixing the published C# is the maintainer's decision.
