//! Limits for untrusted decks, on top of `uo_opc::Limits` (`uo-pptx.md` "Security"). The TS has
//! none of these; every value is a declared refusal of hostile input, named in the record.

/// Caps of the presentation layer. Defaults are fixed by the B1b/B6/B8 frontiers with the iOS
/// budgets in mind; the numbers here are the starting point of the architecture pass.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Limits {
    pub max_slides: u32,
    pub max_shapes_per_slide: u32,
    /// `readGroup` nesting (an explicit stack replaces the TS recursion).
    pub max_group_depth: u32,
    pub max_runs_per_body: u32,
    /// Rows × columns of a table (the border grids are `rows × cols`).
    pub max_table_cells: u32,
    /// `parseTimeNode` nesting (an explicit stack replaces the TS recursion).
    pub max_time_depth: u32,
    pub max_time_nodes: u32,
    /// Units of a `<p:iterate>` (letters/words of the target).
    pub max_iterate_units: u32,
    /// Bytes retained by the undo history across distinct part versions (feature `edit`).
    pub max_history_bytes: u64,
    /// Parsed XML kept by the deck's parse cache (M5): 64 MiB on wasm32, 256 MiB native; LRU.
    pub max_xml_cache_bytes: usize,
    /// Slide models kept by the deck (M5): the viewer's neighbours; LRU.
    pub max_cached_slides: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_slides: 10_000,
            max_shapes_per_slide: 100_000,
            max_group_depth: 64,
            max_runs_per_body: 1_000_000,
            max_table_cells: 1_000_000,
            max_time_depth: 256,
            max_time_nodes: 1_000_000,
            max_iterate_units: 1_000_000,
            max_history_bytes: 64 << 20,
            max_xml_cache_bytes: if cfg!(target_arch = "wasm32") { 64 << 20 } else { 256 << 20 },
            max_cached_slides: 16,
        }
    }
}

impl Limits {
    /// The caps the shape reader takes.
    pub fn dml(&self) -> uo_dml_render::Limits {
        uo_dml_render::Limits {
            max_group_depth: self.max_group_depth,
            max_shapes: self.max_shapes_per_slide,
            max_table_cells: self.max_table_cells,
        }
    }
}
