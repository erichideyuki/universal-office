//! Spike: o núcleo de fontes do motor (sfnt.ts + gpos.ts) em Rust, com a mesma carga de
//! trabalho dos benchmarks em TS e C# (parse das fontes; medição do texto com kerning).
pub mod fx;
pub mod gpos;
pub mod linebreak;
pub mod linedata;
pub mod memcount;
pub mod read;
pub mod sfnt;

// contagem de memória (feature "count", padrão): desligada nas medições de tempo nativas —
// no x86 cada alocação pagaria três operações atômicas
#[cfg(feature = "count")]
#[global_allocator]
static ALLOC: memcount::Counting = memcount::Counting;

use sfnt::SfntFont;

/// Mede o texto como o layout faz por glifo: advance do code point + kerning do par
/// (glifo anterior → atual). Soma em twips.
pub fn measure(font: &SfntFont, text: &[u32], size_pt: f64) -> f64 {
    let mut total = 0.0;
    let mut prev: Option<u32> = None;
    for &cp in text {
        let g = font.glyph_id(cp);
        total += font.advance_twips(cp, size_pt);
        if let Some(p) = prev {
            total += font.kern_glyphs_twips(p, g, size_pt);
        }
        prev = Some(g);
    }
    total
}

/// Interface WebAssembly sem wasm-bindgen: o JS escreve bytes na memória por `alloc` e chama
/// as funções exportadas (C ABI). Estado único do módulo (o JS é de uma thread só).
#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        static FONT_BYTES: RefCell<Vec<Vec<u8>>> = RefCell::new(Vec::new());
        static FONTS: RefCell<Vec<SfntFont>> = RefCell::new(Vec::new());
        static TEXT: RefCell<Vec<u32>> = RefCell::new(Vec::new());
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn alloc(len: usize) -> *mut u8 {
        let mut v = Vec::<u8>::with_capacity(len);
        let p = v.as_mut_ptr();
        std::mem::forget(v);
        p
    }

    /// Guarda os bytes de uma fonte (já copiados para `ptr` pelo JS); devolve o índice.
    #[unsafe(no_mangle)]
    pub extern "C" fn add_font(ptr: *mut u8, len: usize) -> i32 {
        let bytes = unsafe { Vec::from_raw_parts(ptr, len, len) };
        FONT_BYTES.with(|b| {
            let mut b = b.borrow_mut();
            b.push(bytes);
            b.len() as i32 - 1
        })
    }

    /// Texto em UTF-8 → code points (uma vez; a medição itera os code points).
    #[unsafe(no_mangle)]
    pub extern "C" fn set_text(ptr: *mut u8, len: usize) {
        let bytes = unsafe { Vec::from_raw_parts(ptr, len, len) };
        let s = String::from_utf8_lossy(&bytes);
        TEXT.with(|t| *t.borrow_mut() = s.chars().map(|c| c as u32).collect());
    }

    /// Lê todas as fontes `iters` vezes (a última leitura fica para medir); -1 se alguma falhar.
    #[unsafe(no_mangle)]
    pub extern "C" fn parse_all(iters: u32) -> i32 {
        FONT_BYTES.with(|b| {
            let b = b.borrow();
            let mut parsed = Vec::new();
            for _ in 0..iters {
                parsed.clear();
                for bytes in b.iter() {
                    match SfntFont::new(bytes.clone()) {
                        Ok(f) => parsed.push(f),
                        Err(_) => return -1,
                    }
                }
            }
            FONTS.with(|f| *f.borrow_mut() = parsed);
            0
        })
    }

    /// Mede o texto com cada fonte, `iters` vezes; devolve a soma de uma passada (paridade).
    #[unsafe(no_mangle)]
    pub extern "C" fn measure_all(size_pt: f64, iters: u32) -> f64 {
        FONTS.with(|f| {
            TEXT.with(|t| {
                let (f, t) = (f.borrow(), t.borrow());
                let mut sum = 0.0;
                for i in 0..iters {
                    let mut s = 0.0;
                    for font in f.iter() {
                        s += measure(font, &t, size_pt);
                    }
                    if i == 0 {
                        sum = s;
                    }
                }
                sum
            })
        })
    }

    // ── spike 2: quebra de linha ──
    thread_local! {
        static CORPUS: RefCell<Option<crate::linedata::Corpus>> = const { RefCell::new(None) };
        static KEPT: RefCell<Vec<crate::linedata::Line>> = const { RefCell::new(Vec::new()) };
    }

    /// Lê lines.bin (copiado para `ptr` pelo JS); devolve o número de linhas esperadas.
    #[unsafe(no_mangle)]
    pub extern "C" fn lines_load(ptr: *mut u8, len: usize) -> u32 {
        let bytes = unsafe { Vec::from_raw_parts(ptr, len, len) };
        let c = crate::linedata::read_corpus(&bytes);
        let n = c.expected.len() as u32;
        CORPUS.with(|x| *x.borrow_mut() = Some(c));
        n
    }

    /// Uma passada da réplica; `keep` guarda as linhas (para medir a memória retida).
    /// Devolve as linhas diferentes da captura do TS.
    #[unsafe(no_mangle)]
    pub extern "C" fn lines_replay(keep: u32) -> u32 {
        CORPUS.with(|c| {
            let c = c.borrow();
            let c = c.as_ref().unwrap();
            let out = crate::linebreak::replay(c);
            let bad = crate::linedata::mismatches(&out, &c.expected) as u32;
            if keep != 0 {
                KEPT.with(|k| *k.borrow_mut() = out);
            }
            bad
        })
    }

    /// Só a quebra, `iters` vezes (sem conferir), para medir o tempo.
    #[unsafe(no_mangle)]
    pub extern "C" fn lines_bench(iters: u32) -> u32 {
        CORPUS.with(|c| {
            let c = c.borrow();
            let c = c.as_ref().unwrap();
            let mut n = 0;
            for _ in 0..iters {
                n = crate::linebreak::replay(c).len() as u32;
            }
            n
        })
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn lines_drop_kept() {
        KEPT.with(|k| *k.borrow_mut() = Vec::new());
    }

    /// Contadores do alocador: [total alocado, vivo, pico] em bytes.
    #[unsafe(no_mangle)]
    pub extern "C" fn mem_stats(out: *mut f64) {
        let (t, l, p) = crate::memcount::stats();
        let o = unsafe { std::slice::from_raw_parts_mut(out, 3) };
        o[0] = t as f64;
        o[1] = l as f64;
        o[2] = p as f64;
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn mem_reset_peak() {
        crate::memcount::reset_peak();
    }

    /// Paridade por fonte: [pares de kerning, classe 0, kernInLatin, unitsPerEm, medida do texto].
    #[unsafe(no_mangle)]
    pub extern "C" fn font_info(i: usize, size_pt: f64, out: *mut f64) {
        FONTS.with(|f| {
            TEXT.with(|t| {
                let (f, t) = (f.borrow(), t.borrow());
                let font = &f[i];
                let o = unsafe { std::slice::from_raw_parts_mut(out, 5) };
                o[0] = font.kern_pairs() as f64;
                o[1] = font.kern_class0_len() as f64;
                o[2] = if font.kern_in_latin { 1.0 } else { 0.0 };
                o[3] = font.metrics.units_per_em as f64;
                o[4] = measure(font, &t, size_pt);
            })
        })
    }
}
