use masm::{
    assembler::assemble_to_masi, disassembler::load as load_masi, interpreter::run_masi_with_io,
};
use std::io::{Cursor, Read};

fn run_asm_with_io(asm: &str, input: &str) -> (String, String) {
    let bytes = assemble_to_masi(asm).expect("assemble");
    // Write to a temp file-like buffer then parse as MASIFile via load() requires a path; instead we construct MASIFile by loading from bytes.
    // But disassembler::load expects a path; to avoid touching disk, we mimic the file layout reader by writing to temp file.
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), &bytes).unwrap();
    let masi = load_masi(tmp.path().to_string_lossy().as_ref()).expect("load masi");

    let mut input_reader = Cursor::new(input.as_bytes().to_vec());
    let mut out_buf: Vec<u8> = Vec::new();
    let mut err_buf: Vec<u8> = Vec::new();
    let _state =
        run_masi_with_io(&masi, &mut out_buf, &mut err_buf, Some(&mut input_reader)).expect("run");
    let mut out_s = String::new();
    let mut err_s = String::new();
    let _ = Cursor::new(out_buf).read_to_string(&mut out_s);
    let _ = Cursor::new(err_buf).read_to_string(&mut err_s);
    (out_s, err_s)
}

#[test]
fn test_mov_add_out_numeric() {
    let asm = r#"
		MOV R1 5
		ADD R1 7
		OUT 1 R1
		HLT
	"#;
    let (out, err) = run_asm_with_io(asm, "");
    assert_eq!(err, "");
    assert_eq!(out.trim(), "12");
}

#[test]
fn test_out_string_from_data_label() {
    let asm = r#"
		msg: DB "Hello", 0
		OUT 1 $msg
		HLT
	"#;
    let (out, _) = run_asm_with_io(asm, "");
    assert_eq!(out, "Hello\n");
}

#[test]
fn test_cmp_je_jne() {
    let asm = r#"
		MOV R1 10
		CMP R1 10
		JE #eq
		OUT 1 0
		JMP #end
	LBL eq
		OUT 1 1
	LBL end
		HLT
	"#;
    let (out, _) = run_asm_with_io(asm, "");
    assert_eq!(out.lines().collect::<Vec<_>>(), vec!["1"]);
}

#[test]
fn test_push_pop_stack() {
    let asm = r#"
		MOV R2 42
		PUSH R2
		MOV R2 0
		POP R3
		OUT 1 R3
		HLT
	"#;
    let (out, _) = run_asm_with_io(asm, "");
    assert_eq!(out.trim(), "42");
}

#[test]
fn test_call_ret_enter_leave() {
    let asm = r#"
		CALL #func
		HLT
	LBL func
		ENTER 16
		MOV R4 7
		ADD R4 8
		OUT 1 R4
		LEAVE
		RET
	"#;
    let (out, _) = run_asm_with_io(asm, "");
    assert_eq!(out.trim(), "15");
}

#[test]
fn test_in_reads_into_memory_and_out_prints() {
    let asm = r#"
		buf: RESB 64
		IN $buf
		OUT 1 $buf
		HLT
	"#;
    let (out, _) = run_asm_with_io(asm, "abc\n");
    // IN stores input with trailing '\n' and a null terminator; OUT prints until '\0'.
    assert_eq!(out, "abc\n\n");
}

#[test]
fn test_cout_single_char() {
    let asm = r#"
		COUT 1 65
		COUT 1 10
		HLT
	"#;
    let (out, _) = run_asm_with_io(asm, "");
    assert_eq!(out, "A\n");
}

#[test]
fn test_out_to_stderr() {
    let asm = r#"
		OUT 2 123
		HLT
	"#;
    let (out, err) = run_asm_with_io(asm, "");
    assert_eq!(out, "");
    assert_eq!(err.trim(), "123");
}

#[test]
fn test_out_string_via_register_indirect() {
    // Ensure OUT handles $RBX (register-indirect) as a string address correctly.
    let asm = r#"
		hello: DB "Hello from temp!", 0
		MOV RBX hello
		; Both should print the same string
		OUT 1 $RBX
		OUT 1 $[RBX]
		HLT
	"#;
    let (out, err) = run_asm_with_io(asm, "");
    assert_eq!(err, "");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines, vec!["Hello from temp!", "Hello from temp!"]);
}

#[test]
fn test_mni_lua_module_tool_set_rax_and_echo() {
    // The assembler will materialize constant strings for module/function/args.
    let asm = r#"
		MNI tool.set_rax 99
		; set_rax returns RAX=99; print it
		OUT 1 RAX
		MNI tool.echo Hello World
		HLT
	"#;
    let (out, _err) = run_asm_with_io(asm, "");
    let lines: Vec<&str> = out.lines().collect();
    // First line is 99 printed
    assert_eq!(lines.get(0).copied(), Some("99"));
}

#[test]
fn test_jmp_unconditional_skips_block() {
    let asm = r#"
		OUT 1 1
		JMP #after
		OUT 1 999
	LBL after
		OUT 1 2
		HLT
	"#;
    let (out, _) = run_asm_with_io(asm, "");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines, vec!["1", "2"]);
}

#[test]
fn test_sub_then_je_on_zero() {
    let asm = r#"
		MOV R1 7
		SUB R1 7
		JE #zero
		OUT 1 0
		JMP #end
	LBL zero
		OUT 1 1
	LBL end
		HLT
	"#;
    let (out, _) = run_asm_with_io(asm, "");
    assert_eq!(out.trim(), "1");
}

#[test]
fn test_direct_memory_db_address_out_string() {
    let asm = r#"
		DB $1000 "ABS"
		OUT 1 $1000
		HLT
	"#;
    let (out, _) = run_asm_with_io(asm, "");
    assert_eq!(out, "ABS\n");
}

#[test]
fn test_cout_reads_byte_from_memory_address() {
    let asm = r#"
		msg: DB "Hi", 0
		; COUT expects numeric address for second operand
		COUT 1 msg     ; 'H'
		MOV R1 msg
		ADD R1 1
		COUT 1 R1      ; 'i'
		COUT 1 10      ; '\n'
		HLT
	"#;
    let (out, _) = run_asm_with_io(asm, "");
    assert_eq!(out, "Hi\n");
}

#[test]
fn test_mov_memory_via_register_indirect_roundtrip_qword() {
    // Initialize with DQ, then read via $R1
    let asm = r#"
		buf: DQ 0x1122334455667788
		MOV R1 buf
		MOV R2 $R1
		OUT 1 R2
		HLT
	"#;
    let (out, _) = run_asm_with_io(asm, "");
    // 0x1122334455667788 = 1234605616436508552 decimal
    assert_eq!(out.trim(), "1234605616436508552");
}

#[test]
fn test_nop_no_effect() {
    let asm = r#"
		OUT 1 10
		NOP
		OUT 1 20
		HLT
	"#;
    let (out, _) = run_asm_with_io(asm, "");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines, vec!["10", "20"]);
}

#[test]
fn test_jne_taken_when_not_equal() {
    let asm = r#"
		CMP 1 2
		JNE #neq
		OUT 1 0
		JMP #end
	LBL neq
		OUT 1 1
	LBL end
		HLT
	"#;
    let (out, _) = run_asm_with_io(asm, "");
    assert_eq!(out.trim(), "1");
}

#[test]
fn test_enter_leave_updates_rsp_rbp() {
    let asm = r#"
		; After ENTER 8: RSP should be increased by 8 from initial 0, RBP set to 0
		ENTER 8
		OUT 1 RSP
		OUT 1 RBP
		LEAVE
		; After LEAVE: both back to 0
		OUT 1 RSP
		OUT 1 RBP
		HLT
	"#;
    let (out, _) = run_asm_with_io(asm, "");
    let lines: Vec<&str> = out.lines().collect();
    // Interpreter ENTER pushes old RBP (stack grows by 1) before allocating size bytes.
    // So after ENTER 8: RSP = 1 + 8 = 9, RBP = 1
    assert_eq!(lines, vec!["9", "1", "1", "0"]);
}

#[test]
fn test_fmov_fadd_and_result_bits() {
    let asm = r#"
		FMOV FPR0 2.5
		FADD FPR0 1.5
		; Move result bits into an integer register and print
		MOV R1 FPR0
		OUT 1 R1
		HLT
	"#;
    let (out, _) = run_asm_with_io(asm, "");
    let got: u64 = out.trim().parse().unwrap();
    assert_eq!(got, (2.5f64 + 1.5f64).to_bits()); // 4.0
}

#[test]
fn test_fmov_load_from_memory_ddbl() {
    let asm = r#"
		val: DDBL 3.25
		FMOV FPR1 $val
		MOV R2 FPR1
		OUT 1 R2
		HLT
	"#;
    let (out, _) = run_asm_with_io(asm, "");
    let got: u64 = out.trim().parse().unwrap();
    assert_eq!(got, 3.25f64.to_bits());
}

#[test]
fn test_fcmp_conditional_jumps_and_unordered() {
    // Less-than branch should be taken
    let asm_lt = r#"
		FMOV FPR0 2.0
		FMOV FPR1 3.0
		FCMP FPR0 FPR1
		FJLT #yes
		OUT 1 0
		JMP #end
	LBL yes
		OUT 1 1
	LBL end
		HLT
	"#;
    let (out_lt, _) = run_asm_with_io(asm_lt, "");
    assert_eq!(out_lt.trim(), "1");

    // Unordered (NaN) should take FJUO
    let asm_uo = r#"
		nan: DQ 0x7FF8000000000001
		FMOV FPR0 $nan
		FMOV FPR1 1.0
		FCMP FPR0 FPR1
		FJUO #u
		OUT 1 0
		JMP #end
	LBL u
		OUT 1 1
	LBL end
		HLT
	"#;
    let (out_uo, _) = run_asm_with_io(asm_uo, "");
    assert_eq!(out_uo.trim(), "1");
}

#[test]
fn test_syscall_write_and_read() {
    // Use SYSCALL write(1, buf, len) and read(0, buf, len)
    let asm = r#"
		DB $2000 "sys\n"
		MOV RAX 1
		MOV RDI 1
		MOV RSI 2000
		MOV RDX 4
		SYSCALL
		; read line from input (max 5)
		MOV RAX 0
		MOV RDI 0
		MOV RSI 3000
		MOV RDX 5
		SYSCALL
		OUT 1 $3000
		HLT
	"#;
    let (out, _) = run_asm_with_io(asm, "in\n");
    let mut lines: Vec<&str> = out.lines().collect();
    if let Some("") = lines.last().copied() {
        lines.pop();
    }
    assert_eq!(lines, vec!["sys", "in"]);
}

#[test]
fn test_macros_with_local_labels() {
    let asm = r#"
		MACRO twice reg val
			ADD reg val
			ADD reg val
		ENDMACRO

		MACRO loop_dec reg
		@@start:
			DEC reg
			JNZ @@start
		ENDMACRO

		MOV R1 0
		twice R1 5
		OUT 1 R1

		MOV R2 3
		loop_dec R2
		OUT 1 R2
		HLT
	"#;
    let (out, _) = run_asm_with_io(asm, "");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines, vec!["10", "0"]);
}

#[test]
fn test_mni_math_and_memory() {
    let asm = r#"
		; allocate 8 bytes and write a string
		MNI Memory.allocate 8 R1
		MOV R2 R1
		DB $4000 "x\0"
		; sqrt(9.0) into FPR1
		FMOV FPR0 9.0
		MNI Math.sqrt FPR0 FPR1
		MOV R3 FPR1
		OUT 1 R3
		HLT
	"#;
    let (out, _) = run_asm_with_io(asm, "");
    let got: u64 = out.trim().parse().unwrap();
    assert_eq!(got, 3.0f64.to_bits());
}
