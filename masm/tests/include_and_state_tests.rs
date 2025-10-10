use masm::{assembler::{assemble_to_masi_with_base, assemble_to_masi}, disassembler::load as load_masi, interpreter::run_masi_with_io};
use std::io::{Cursor, Read};

fn run_asm_with_io_state(asm: &str, input: &str) -> (String, String, masm::interpreter::State) {
    let bytes = assemble_to_masi(asm).expect("assemble");
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), &bytes).unwrap();
    let masi = load_masi(tmp.path().to_string_lossy().as_ref()).expect("load masi");

    let mut input_reader = Cursor::new(input.as_bytes().to_vec());
    let mut out_buf: Vec<u8> = Vec::new();
    let mut err_buf: Vec<u8> = Vec::new();
    let state = run_masi_with_io(&masi, &mut out_buf, &mut err_buf, Some(&mut input_reader)).expect("run");
    let mut out_s = String::new();
    let mut err_s = String::new();
    let _ = Cursor::new(out_buf).read_to_string(&mut out_s);
    let _ = Cursor::new(err_buf).read_to_string(&mut err_s);
    (out_s, err_s, state)
}

fn run_asm_with_base(asm: &str, base_dir: &std::path::Path) -> (String, String) {
    let bytes = assemble_to_masi_with_base(asm, &base_dir.to_string_lossy()).expect("assemble with base");
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), &bytes).unwrap();
    let masi = load_masi(tmp.path().to_string_lossy().as_ref()).expect("load masi");

    let mut input_reader = Cursor::new(Vec::<u8>::new());
    let mut out_buf: Vec<u8> = Vec::new();
    let mut err_buf: Vec<u8> = Vec::new();
    let _ = run_masi_with_io(&masi, &mut out_buf, &mut err_buf, Some(&mut input_reader)).expect("run");
    let mut out_s = String::new();
    let mut err_s = String::new();
    let _ = Cursor::new(out_buf).read_to_string(&mut out_s);
    let _ = Cursor::new(err_buf).read_to_string(&mut err_s);
    (out_s, err_s)
}

#[test]
fn test_include_source_file() {
    let tmpdir = tempfile::tempdir().unwrap();
    let inc_path = tmpdir.path().join("inc.masm");
    std::fs::write(&inc_path, b"msg: DB \"INC\", 0\n").unwrap();
    let asm = "#include \"inc.masm\"\nOUT 1 $msg\nHLT\n";
    let (out, err) = run_asm_with_base(asm, tmpdir.path());
    assert_eq!(err, "");
    assert_eq!(out, "INC\n");
}

#[test]
fn test_include_bytes_and_len() {
    let tmpdir = tempfile::tempdir().unwrap();
    let data_path = tmpdir.path().join("data.bin");
    // 8 bytes to form a known u64 value 0x8877665544332211
    let bytes: [u8;8] = [0x11,0x22,0x33,0x44,0x55,0x66,0x77,0x88];
    std::fs::write(&data_path, &bytes).unwrap();
    let asm = format!(
        "#include_bytes data, \"{}\"\n\
         MOV R1 data\n\
         MOV R2 $R1\n\
         OUT 1 R2\n\
         MOV R3 data_len\n\
         MOV R4 $R3\n\
         OUT 1 R4\n\
         HLT\n",
        data_path.file_name().unwrap().to_string_lossy()
    );
    let (out, err) = run_asm_with_base(&asm, tmpdir.path());
    assert_eq!(err, "");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].parse::<u64>().unwrap(), 0x8877665544332211);
    assert_eq!(lines[1].parse::<u64>().unwrap(), 8);
}

#[test]
fn test_include_dq_and_len() {
    let tmpdir = tempfile::tempdir().unwrap();
    let data_path = tmpdir.path().join("table.bin");
    // 16 bytes -> two u64s: 1 and 2
    let mut bytes: Vec<u8> = Vec::new();
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&2u64.to_le_bytes());
    std::fs::write(&data_path, &bytes).unwrap();
    let asm = format!(
        "#include_dq tab, \"{}\"\n\
         MOV R1 tab\n\
         MOV R2 $R1\n\
         OUT 1 R2\n\
         MOV R3 tab\n\
         ADD R3 8\n\
         MOV R3 $R3\n\
         OUT 1 R3\n\
         MOV R4 tab_len\n\
         MOV R4 $R4\n\
         OUT 1 R4\n\
         HLT\n",
        data_path.file_name().unwrap().to_string_lossy()
    );
    let (out, err) = run_asm_with_base(&asm, tmpdir.path());
    assert_eq!(err, "");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines, vec!["1", "2", "16"]);
}

#[test]
fn test_state_directive_emits_labeled_value() {
    let asm = r#"
    buf: RESQ 1
    STATE my_b <BYTE> 65
    pad: RESQ 1
        MOV R1 my_b
        MOV R2 $R1
        OUT 1 R2
        STATE my_q <QWORD> 1234
        MOV R3 my_q
        MOV R4 $R3
        OUT 1 R4
        HLT
    "#;
    let (out, err, _state) = run_asm_with_io_state(asm, "");
    assert_eq!(err, "");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines, vec!["65", "1234"]);
}

#[test]
fn test_oob_read_emits_warning() {
    let asm = r#"
        MOV R1 0x100000
        MOV R2 $R1
        HLT
    "#;
    let (_out, _err, state) = run_asm_with_io_state(asm, "");
    assert!(state.warnings.iter().any(|w| w.contains("read_u64 OOB")));
}

#[test]
fn test_mni_missing_returns_error() {
    // Assemble OK, but runtime should error on missing MNI function
    let asm = r#"
        MNI notamodule.notafunc arg1
        HLT
    "#;
    let bytes = assemble_to_masi(asm).expect("assemble");
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), &bytes).unwrap();
    let masi = load_masi(tmp.path().to_string_lossy().as_ref()).expect("load masi");
    let mut input_reader = Cursor::new(Vec::<u8>::new());
    let mut out_buf: Vec<u8> = Vec::new();
    let mut err_buf: Vec<u8> = Vec::new();
    let res = run_masi_with_io(&masi, &mut out_buf, &mut err_buf, Some(&mut input_reader));
    assert!(res.is_err());
    let msg = res.err().unwrap();
    assert!(msg.contains("MNI: function not found"));
}
