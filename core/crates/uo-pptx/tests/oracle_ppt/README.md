# PowerPoint-PDF oracle of the Slides PDF (M4 gate prep)

Measures how closely the Rust Slides PDF (`uo_pptx::pdf`) agrees with PowerPoint 365's own PDF export, and splits each
difference between "the TS viewer draws it the same as Rust" (TS legacy: PowerPoint should decide) and "Rust differs from
the TS" (the PDF interpreter). Method, numbers and the ranked difference list: `core/docs/modules/uo-pdf.md`, section
"PowerPoint agreement (M4 prep)".

## Files

| File | What |
|---|---|
| `decks.tsv` | the deck set: every tracked `slides/samples` file (`.pptx` and `.ppt`) and the generated decks (`b1a`, `b5a`, `b5b`, `b6a`, `c1d` all; `b3a` every 5th), with the deck's sha256 and the generator's feature tags |
| `main.rs` | the Rust side (`cargo test -p uo-pptx --release --features ppt --test oracle_ppt`): writes `rust-system/` and `rust-open/` PDFs; does nothing without `UO_PPT_ORACLE_OUT` |
| `results/ppt-pdfs.tsv` | per deck: PowerPoint's PDF digest, size, slide count, hidden slides, export time — or PowerPoint's refusal message |
| `results/pages.tsv` | per page: sizes, pixel agreement (Rust system fonts, Rust open fonts, TS canvas, Rust x TS), the split of the differing pixels, the page objects under them, text similarity and glyph placement, link annotations, the Rust report's per-page missing count, the font names of both PDFs |
| `results/decks.tsv` | per deck: status (`ok`, `ppt-refused`, page count mismatch), mean/min agreement, the Rust report's missing counters |
| `results/features.tsv` | generated pages grouped by generator feature: mean agreement (the lowest first; noisy: whole-page causes dominate) |
| `results/classes.tsv` | the cause classes (`pdf-oracle-classify.py`): pages, decks and differing pixels per known cause, split TS legacy / PDF only |
| `results/baseline.tsv` | vertical and horizontal glyph offsets (Rust - PowerPoint) per PowerPoint font (`pdf-oracle-baseline.py`) |
| `results/summary.json` | the aggregates |

PowerPoint's PDFs, the Rust PDFs, the TS PNGs and the diff pictures are NOT committed: they live in the work folder
(`$UO_PPT_ORACLE_OUT`, default `core/target/w4-oracle/ppt-oracle`). The PowerPoint PDFs are cached there by deck sha256.

## Running

```
node slides/tools/ppt-lab/pdf-oracle.mjs list      # rewrite decks.tsv (only when the deck set changes)
node slides/tools/ppt-lab/pdf-oracle.mjs ppt       # PowerPoint PDFs (COM, Windows + PowerPoint 365; cached)
node slides/tools/ppt-lab/pdf-oracle.mjs rust      # Rust PDFs, both font policies
node slides/tools/ppt-lab/pdf-oracle.mjs ts        # TS canvases (Chromium, frozen 0.9.2 checkout .claude/worktrees/ts-ref-092)
node slides/tools/ppt-lab/pdf-oracle.mjs compare   # pdfium comparison -> results/
```

`UO_PPT_ORACLE_ONLY=id,id` limits the Rust, TS and compare steps; `ppt --only id,id` the PowerPoint step.

PowerPoint rules followed by `slides/tools/ppt-lab/export-pdf.ps1`: powershell started with `windowsHide`, each deck opened
read-only with `WithWindow = msoFalse`, COM paths as `[string]`, the `.ps1` with a UTF-8 BOM and run with
`-ExecutionPolicy Bypass`, only the presentations it opened are closed, PowerPoint quit only if the script started it, no
screen capture. Hidden slides are shown in memory before `SaveAs(ppSaveAsPDF)` so that page i is slide i.

## Fonts

No font file is copied. The `system` policy reads the installed fonts in place (`%WINDIR%\Fonts`, the user's fonts folder,
the Office `CloudFonts` cache) — the same files PowerPoint uses, so text differences are layout, not font substitution; the
`open` policy uses only the repository's open fonts (Liberation, Carlito, Caladea), the shipped default. The PDFs written
under `core/target` embed subsets like any PDF made on the machine, and stay out of git.

## Notes

- PowerPoint refuses most generated decks as corrupt ("O arquivo ou pasta está corrompido e ilegível", or "PowerPoint
  could not open the file"): the generators aim at the TS reader's behaviour (hostile values, lenient structure), not at
  schema validity. Only the decks PowerPoint opens are compared; `results/ppt-pdfs.tsv` lists every refusal.
- The generator no longer rebuilds the committed deck bytes (`gen-decks.mjs --check` differs: zlib of the current Node);
  the feature tags of `decks.tsv` are those of the current generator for the same seed.
