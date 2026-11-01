# POC: the .NET Docx engine in WebAssembly

Question: can one C# engine (model, .docx reading, layout, hit-testing, PDF) serve the
browser, Node and native apps, with only a thin painter per platform?

What this POC does:

- `WasmPoc/` compiles `UniversalOffice.Docx` (unchanged) to WebAssembly with the plain
  .NET WebAssembly SDK (`Microsoft.NET.Sdk.WebAssembly`, no Blazor) and exports a few
  methods to JS (`[JSExport]`): add a font, load + lay out a .docx, re-lay out, the layout
  tree as JSON, face and image bytes, hit-test, PDF.
- The layout tree comes out in the field names of the TS `tree.ts` (the .NET tree is a
  port of it), so the browser page paints it with the **existing TS canvas painter**
  (`paintItems` / `paintPageBackground` from `docx-layout/src/canvas.ts`), unchanged.
  Clicking a page asks C# for the hit-test (the engine's line geometry) and draws the caret.
- The same build runs in Node (`bench-node.mjs`).
- Baselines with the same document (`site/assets/samples/demo.docx`, 8 pages, embedded
  Ubuntu fonts, images, floats) and the same open fonts (Carlito, Caladea, Liberation):
  `NativeBench/` (the engine on .NET 10, JIT) and `bench-ts.mjs` (the TS engine in Node).

## Results (Windows desktop, Node 24, Chromium in the app's browser pane)

| | .NET native (JIT) | TS engine (Node) | .NET wasm interpreted (Node) | .NET wasm AOT (Node) |
|---|---|---|---|---|
| Runtime start | — | — | 80–320 ms | 95–405 ms |
| Read .docx (cold) | 73 ms | 49 ms | 250 ms | 120–129 ms |
| First layout (cold) | 266 ms | 221 ms | 950 ms | 313–332 ms |
| Layout, warm (best) | **64 ms** | **97 ms** | **423 ms** | **125–130 ms** |
| Tree → JSON | — | — | 230 ms | 152–159 ms |
| PDF | 51 ms | 36 ms | 97 ms | 45 ms |

In the browser (the app's pane, noisy between reloads because the window was in the
background): interpreted, warm layout 421 ms and first paint 1.8–4.8 s; AOT, warm layout
108–204 ms and first paint 1.0–1.7 s.

Rendering path in the browser: C# tree → JSON 849 KB in 230 ms (reflection-based
`System.Text.Json`), `JSON.parse` 2–5 ms, fonts as CSS `FontFace` + TS `SfntFont` 60–290 ms,
painting 8 pages with the TS painter 48–84 ms. The output matches: same 8 pages, same PDF
size (237 KB) on .NET native and wasm; the pages look right (embedded fonts, images,
floating images, shading, borders, hanging indents).

Download size (`_framework`, what the browser fetches for the engine):

| | Uncompressed | Brotli |
|---|---|---|
| .NET wasm interpreted, engine assembly kept whole (needed by the reflection JSON) | 8.52 MB | 2.76 MB |
| .NET wasm interpreted, fully trimmed (the reflection JSON then fails) | 6.80 MB | 2.24 MB |
| .NET wasm AOT (full trimming made no difference here) | 22.99 MB | 5.15 MB |
| TS engine (docx-core + docx-layout + PDF + painter, esbuild minified) | 1.07 MB | 0.35 MB |

With AOT, `dotnet.native.wasm` alone is 17.7 MB (3.8 MB Brotli): the compiled code of
every method, and Mono AOT still ships the IL assemblies for metadata. The AOT publish took
3.5 minutes. Of the interpreted 2.76 MB, the fixed .NET cost is about 1.5 MB (`dotnet.native.wasm` 953 KB +
`System.Private.CoreLib` 473 KB + runtime JS); the engine assemblies are 0.5–0.7 MB.

## Size reduction

`WasmPoc.csproj` now builds "lean" by default (`-p:Lean=false` for the SDK defaults):
full trimming, the layout JSON from a source generator (`LayoutJsonContext.cs`, no
reflection), invariant time zone, no exception message strings (`UseSystemResourceKeys`),
no EventSource/debugger/hot reload, IL stripped after AOT (`WasmStripILAfterAOT`) and the
native code built with `-Oz`. The layout JSON is byte-for-byte identical to the reflection
version and the PDF has the same size.

| Build | Uncompressed | Brotli | Warm layout (Node) | Tree → JSON |
|---|---|---|---|---|
| Interpreted, SDK defaults | 8.52 MB | 2.76 MB | 423 ms | 230 ms |
| **Interpreted, lean** | 5.41 MB | **1.72 MB** | 465 ms | 232 ms |
| AOT, SDK defaults | 22.99 MB | 5.15 MB | 125–130 ms | 152–159 ms |
| **AOT, lean** | 16.92 MB | **3.75 MB** | 137–142 ms | 92 ms |
| TS engine (reference) | 1.07 MB | 0.35 MB | 97 ms | — |

`-Oz` costs ~10% of speed in both modes. The lean build shows one trim warning
(`ModelOps.DeepEquals` reads fields by reflection); it is not on the layout path, but the
engine would need an annotation or a non-reflective version before shipping trimmed.

What is left in the AOT build is mostly compiled code (`dotnet.native.wasm` 2.7 MB Brotli),
and that comes from everything reachable: CoreLib generics, LINQ, Regex, XML, JSON and the
engine. Further cuts need code changes, not flags: fewer generic instantiations and LINQ in
hot paths, no Regex in the engine, a hand-written binary display list instead of
System.Text.Json, and AOT for the engine only (profile-guided AOT exists in Mono but was
not tried here).

## Conclusions


1. **The architecture works.** The C# engine runs unchanged in the browser and in Node, and
   its layout tree is painted by the existing TS painter: one engine, thin painters. The
   hit-test (caret from C# line geometry) works across the boundary.
2. **Speed: AOT closes most of the gap.** Interpreted, a full re-layout costs ~420 ms (4.4×
   the TS engine); with AOT 125–130 ms, 1.3× the TS engine and 2× native .NET. Cold start
   to first paint drops from 2–5 s to about 1 s in the browser.
3. **Size is the price: ~15× the TS engine with AOT** (5.15 MB vs 0.35 MB Brotli; 2.2–2.8
   MB interpreted). Fine for the desktop/Electron app and for a viewer or PDF service
   loaded on demand; heavy for the npm editor package and phones on slow networks.
4. **The JSON bridge is a bottleneck** (230 ms, and it blocks full trimming). A real version
   would write a compact binary display list (or only the changed pages) without
   reflection.
5. **Node works with the same build**; on a server, plain .NET (NuGet) is still faster.

## Not measured yet

- Mixed mode (AOT only for the engine's hot paths, interpreter for the rest) to trade some
  speed for size, and Jiterpreter tuning.
- A mid-range phone (expect several times the desktop numbers).
- Editing: a keystroke today would mean a full re-layout plus a full tree transfer.

## Run it

AOT needs the `wasm-tools` workload (`dotnet workload install wasm-tools`, elevated).

```shell
dotnet publish WasmPoc -c Release -o out/lean-interp
dotnet publish WasmPoc -c Release -o out/lean-aot -p:Aot=true
node bench-node.mjs out/interp
node bench-node.mjs out/aot
dotnet run -c Release --project NativeBench -- <this folder>
node bench-ts.mjs
node build-web.mjs out/interp
python -m http.server 8131 --directory web-dist
```

`bench-ts.mjs` uses the built `dist/` of `write/packages/docx-core` and `docx-layout`.
