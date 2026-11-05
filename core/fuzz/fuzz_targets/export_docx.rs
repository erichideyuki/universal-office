#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| uo_opc_fuzz::export_docx(data));
