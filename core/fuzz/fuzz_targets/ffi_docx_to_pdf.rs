#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| uo_opc_fuzz::ffi_docx_to_pdf(data));
