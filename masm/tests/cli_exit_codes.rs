use std::process::Command;

// Helper to build the cargo binary path
fn bin() -> std::path::PathBuf {
    // cargo test sets CARGO_BIN_EXE_<name> env var for integration tests
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_masm") {
        return std::path::PathBuf::from(p);
    }
    // Fallback: assume debug build in target (useful for some IDEs)
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("target");
    p.push("debug");
    p.push("masm");
    p
}

#[test]
fn assemble_nonexistent_file_exits_nonzero() {
    let status = Command::new(bin())
        .arg("this_file_does_not_exist.masm")
        .status()
        .expect("failed to run masm");
    assert!(!status.success(), "expected non-zero exit for missing input");
}

#[test]
fn link_too_few_inputs_exits_nonzero() {
    let status = Command::new(bin())
        .args(["link", "a.masi"]) // only one, should require at least two
        .status()
        .expect("failed to run masm");
    assert!(!status.success(), "expected non-zero exit for invalid link invocation");
}

#[test]
fn disasm_nonexistent_file_exits_nonzero() {
    let status = Command::new(bin())
        .args(["missing.masi", "--disasm"]) // load should fail
        .status()
        .expect("failed to run masm");
    assert!(!status.success(), "expected non-zero exit for missing masi disasm");
}
