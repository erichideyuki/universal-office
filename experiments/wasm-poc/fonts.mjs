// As fontes abertas da suíte (as mesmas para o motor .NET em WebAssembly e o motor TS):
// Carlito (Calibri), Caladea (Cambria), Liberation Sans/Serif/Mono (Arial, Times, Courier).
export const FONT_FILES = [];
for (const [file, family] of [
  ['carlito', 'Carlito'], ['caladea', 'Caladea'], ['liberationsans', 'Liberation Sans'],
  ['liberationserif', 'Liberation Serif'], ['liberationmono', 'Liberation Mono'],
]) {
  for (const [v, bold, italic] of [['regular', false, false], ['bold', true, false], ['italic', false, true], ['bolditalic', true, true]]) {
    FONT_FILES.push({ path: `${file}-${v}.ttf`, family, bold, italic });
  }
}
