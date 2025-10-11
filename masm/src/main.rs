mod register_map;
mod assembler;
mod disassembler;
mod interpreter;
mod linker;

use std::env;
use std::fs;
use std::path::PathBuf;

fn print_usage() {
    eprintln!(
        "Usage:\n  masm <input.masm> [-o <out.masi>]\n  masm <input.masi> --disasm [-o <out.masm>]\n  masm <input.masi> --dump\n  masm <input.masi> --run\n  masm link <a.masi> <b.masi>... -o <out.masi>\n\nOptions:\n  -o <file>   Output file path (assemble: out.masi, disasm: stdout if omitted)\n  --dump      Dump MASI header/sections/labels\n  --disasm    Disassemble .masi to MASM text\n  --run       Execute a .masi file with the Rust interpreter\n  -h, --help  Show help\n"
    );
}

fn main() {
    let mut args = env::args().skip(1);
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut disasm: bool = false;
    let mut dump: bool = false;
    let mut run_flag: bool = false;
    let mut link_mode: bool = false;
    let mut link_inputs: Vec<String> = Vec::new();

    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                print_usage();
                return;
            }
            "-o" => {
                if let Some(p) = args.next() { output = Some(PathBuf::from(p)); } else { eprintln!("-o requires a file path"); return; }
            }
            "--disasm" | "-x" => { disasm = true; }
            "--dump" | "-t" => { dump = true; }
            "--run" | "-r" => { run_flag = true; }
            _ => {
                let lower = a.to_lowercase();
                if lower == "link" { link_mode = true; }
                else if link_mode {
                    // collect link inputs until -o or end
                    if lower.starts_with("-") { eprintln!("Unexpected option in link inputs: {}", a); print_usage(); return; }
                    link_inputs.push(a);
                }
                else if lower.ends_with(".masm") || lower.ends_with(".masi") { input = Some(PathBuf::from(a)); }
                else { eprintln!("Unknown argument: {}", a); print_usage(); return; }
            }
        }
    }

    if link_mode {
        if link_inputs.len() < 2 { eprintln!("link requires at least two .masi inputs"); return; }
        let out_path = output.unwrap_or_else(|| PathBuf::from("out.masi"));
        match linker::link_files(&link_inputs) {
            Ok(res) => { if let Err(e) = fs::write(&out_path, res.bytes) { eprintln!("Failed to write {}: {}", out_path.display(), e); } else { println!("Linked -> {}", out_path.display()); } }
            Err(e) => eprintln!("Link failed: {}", e),
        }
        return;
    }

    let Some(input_path) = input else { print_usage(); return; };
    if input_path.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("masi")).unwrap_or(false) {
        // Run, disassemble, or dump
        let path_str = input_path.to_string_lossy();
        let masi = match disassembler::load(&path_str) { Ok(m) => m, Err(e) => { eprintln!("Failed to load MASI: {}", e); return; } };
        if run_flag || (!disasm && !dump) {
            if let Err(e) = interpreter::run_masi(&masi) { eprintln!("Run failed: {}", e); }
            return;
        }
        if dump {
            println!("{}", disassembler::dump(&masi));
            return;
        }
        if disasm {
            let asm = disassembler::disassemble(&masi);
            if let Some(out) = output { if let Err(e) = fs::write(&out, asm.as_bytes()) { eprintln!("Failed to write {}: {}", out.display(), e); } }
            else { println!("{}", asm); }
            return;
        }
        // unreachable: covered above
    } else {
        // Assemble
        let out_path = output.unwrap_or_else(|| PathBuf::from("out.masi"));
        let src = match fs::read_to_string(&input_path) { Ok(s) => s, Err(e) => { eprintln!("Failed to read {}: {}", input_path.display(), e); return; } };
        let base = input_path.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| ".".to_string());
        match assembler::assemble_to_masi_with_base(&src, &base) {
            Ok(bytes) => {
                if let Err(e) = fs::write(&out_path, bytes) { eprintln!("Failed to write {}: {}", out_path.display(), e); return; }
                println!("Wrote MASI to {}", out_path.display());
            }
            Err(e) => { eprintln!("Assembly failed: {}", e); }
        }
    }
}
