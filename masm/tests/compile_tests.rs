use masm::disassembler::parse_masi_bytes;
use masm::interpreter::compile_source_to_masi_bytes;

#[test]
fn test_compile_and_parse_simple() {
    let asm = r#"
        ; simple program: define data and hlt
        msg: DB "Hello", 0
        OUT 1 $msg
        HLT
    "#;
    let bytes = compile_source_to_masi_bytes(asm).expect("compile");
    let masi = parse_masi_bytes(&bytes).expect("parse masi");
    // Expect code and data to be non-empty and header entry to be within code
    assert!(masi.code.len() > 0, "code should be present");
    assert!(masi.data.len() > 0, "data should be present");
}
