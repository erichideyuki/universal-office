//! The integration tests of uo-docx as ONE test binary: each file of this folder is a module.
//! One binary instead of one per file cuts the link time of the suite and the work of
//! `clippy --all-targets` (160 crates before). Run one file with `cargo test -p uo-docx --test all <file>::`.
//! The support modules are declared here once; a test file imports them with `use crate::<name>;`.

#[cfg(feature = "session")]
mod d2a_support;
#[cfg(feature = "session")]
mod d2b_f1_support;
#[cfg(feature = "session")]
mod d2b_f2_support;
#[cfg(feature = "session")]
mod d2b_f3_support;
#[cfg(feature = "session")]
mod d2b_f4_support;
#[cfg(feature = "session")]
mod d2b_f5_support;
#[cfg(feature = "session")]
mod d2b_f6_support;
#[cfg(feature = "session")]
mod d2b_f7_support;
#[cfg(feature = "session")]
mod d2b_f9_support;
#[cfg(any(feature = "compare", feature = "export", feature = "odf"))]
mod d5_support;
// also under d5_support (`d5_support::a`)
#[cfg(any(feature = "compare", feature = "export", feature = "odf"))]
mod d5a_support;
#[cfg(feature = "doc")]
mod doc_support;
mod fonts_support;
#[path = "support/nested_stories.rs"]
mod nested_stories;
#[path = "support/r4b_host.rs"]
mod r4b_host;
mod support;
mod w2_support;
mod w4_support;

mod canon_selftest;
mod compare_limits;
mod data_tables;
mod drawing_contour_unit;
mod drawing_unit;
mod fonts_unit;
mod inline_objects_unit;
mod memory;
mod memory_compare;
mod memory_contour;
mod memory_d1b_fonts;
mod memory_d1b_writer;
mod memory_d2a;
mod memory_d2b_f1;
mod memory_d2b_f2;
mod memory_d2b_f3;
mod memory_d2b_f4;
mod memory_d2b_f5;
mod memory_d2b_f6;
mod memory_d2b_f7;
mod memory_d2b_f9;
mod memory_d3;
mod memory_doc;
mod memory_export;
mod memory_model_size;
mod memory_odf;
mod memory_r1;
mod memory_r2;
mod memory_r3;
mod memory_r4a;
mod memory_r4b;
mod memory_r4c;
mod memory_r5;
mod memory_r6;
mod memory_w1;
mod memory_w2;
mod memory_w3;
mod memory_w4;
mod memory_w5;
mod memory_w6;
mod memory_word;
mod nested_stack;
mod nopanic_compare;
mod nopanic_contour;
mod nopanic_d1b_fonts;
mod nopanic_d1b_writer;
mod nopanic_d2a;
mod nopanic_d2b_f1;
mod nopanic_d2b_f2;
mod nopanic_d2b_f3;
mod nopanic_d2b_f4;
mod nopanic_d2b_f5;
mod nopanic_d2b_f6;
mod nopanic_d2b_f7;
mod nopanic_d2b_f9;
mod nopanic_d3;
mod nopanic_doc;
mod nopanic_export;
mod nopanic_odf;
mod nopanic_r1;
mod nopanic_r2;
mod nopanic_r3;
mod nopanic_r4b;
mod nopanic_r4c;
mod nopanic_r5;
mod nopanic_r6;
mod nopanic_w1;
mod nopanic_w2;
mod nopanic_w3;
mod nopanic_w4;
mod nopanic_w5;
mod nopanic_w6;
mod nopanic_word;
mod parity_compare;
mod parity_contour;
mod parity_custom_tag;
mod parity_d2a_ops;
mod parity_d2a_session;
mod parity_d2b_f1_ops;
mod parity_d2b_f1_session;
mod parity_d2b_f2_ops;
mod parity_d2b_f2_session;
mod parity_d2b_f3_ops;
mod parity_d2b_f3_session;
mod parity_d2b_f4_ops;
mod parity_d2b_f4_session;
mod parity_d2b_f5_ops;
mod parity_d2b_f5_session;
mod parity_d2b_f6_ops;
mod parity_d2b_f6_session;
mod parity_d2b_f7_ops;
mod parity_d2b_f7_session;
mod parity_d2b_f9_ops;
mod parity_d2b_f9_session;
mod parity_d3;
mod parity_d3_semantics;
mod parity_doc;
mod parity_drawing;
mod parity_drawing_runs;
mod parity_export;
mod parity_font_embedding;
mod parity_full;
mod parity_lossless;
mod parity_object_edits;
mod parity_odf;
mod parity_page_background;
mod parity_paragraph;
mod parity_parts;
mod parity_picture_xml;
mod parity_props;
mod parity_props_map;
mod parity_runs_w1;
mod parity_settings_xml;
mod parity_shape_xml;
mod parity_sources;
mod parity_sources_blocks;
mod parity_sources_para;
mod parity_styles;
mod parity_table_styles;
mod parity_table_xml;
mod parity_write_roundtrip;
mod props_map;
mod props_unit;
mod r2_unit;
mod r4c_unit;
mod r5_followups;
mod r5_unit;
mod regex_d1b_core;
mod regex_d1b_fonts;
mod regex_d2b_f1;
mod regex_d2b_f3;
mod regex_d2b_f6;
mod regex_d2b_f7;
mod regex_d2b_f9;
mod regex_d5a_compare;
mod regex_d5a_export;
mod regex_r1;
mod regex_r2;
mod regex_r3;
mod regex_r4b;
mod regex_r4c;
mod regex_r5;
mod regex_r6;
mod regex_templates;
mod regex_w1;
mod regex_w2;
mod regex_w4;
mod regex_w5;
mod regex_w6;
mod session_d2a;
mod session_d2b_f1;
mod session_d2b_f2;
mod session_d2b_f3;
mod session_d2b_f4;
mod session_d2b_f5;
mod session_d2b_f6;
mod session_d2b_f7;
mod session_d2b_f9;
mod sources_unit;
mod word_unit;
