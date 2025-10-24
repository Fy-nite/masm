#![allow(dead_code)]
mod assembler;
mod disassembler;
mod interpreter;
mod linker;
mod register_map;
//#[cfg(feature = "raylib_mni")]
//pub mod mni_raylib;

use interpreter::{CliDebugger, set_thread_debugger, set_debug_print};
use std::env;
use std::fs;
use std::path::PathBuf;
#[cfg(feature = "ratatui_debug")]
mod ratatui_debugger;
#[cfg(feature = "ratatui_debug")]
use ratatui_debugger::RatatuiDebugger;

fn print_usage() {
    eprintln!(
        "Usage:\n  masm <input.masm> [-o <out.masi>]\n  masm <input.masi> --disasm [-o <out.masm>]\n  masm <input.masi> --dump\n  masm <input.masi> --run [--stdin-from <file>]\n  masm <input.masi> --debug [--stdin-from <file>]\n  masm link <a.masi> <b.masi>... -o <out.masi>\n\nOptions:\n  -o <file>          Output file path (assemble: out.masi, disasm: stdout if omitted)\n  --dump             Dump MASI header/sections/labels\n  --disasm          Disassemble .masi to MASM text\n  --run             Execute a .masi file with the Rust interpreter\n  --debug, -g      Run with interactive debugger\n  --debug-mni       Enable MNI function execution time instrumentation\n  --stdin-from <f> Read program input from file (for IN/syscall read), not the console\n  -h, --help       Show help\n"
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
    let mut debug_mode: bool = false;
    let mut debug_mni: bool = false;
    let mut stdin_from: Option<PathBuf> = None;
    let mut link_inputs: Vec<String> = Vec::new();

    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                print_usage();
                return;
            }
            "-o" => {
                if let Some(p) = args.next() {
                    output = Some(PathBuf::from(p));
                } else {
                    eprintln!("-o requires a file path");
                    std::process::exit(1);
                }
            }
            "--disasm" | "-x" => {
                disasm = true;
            }
            "--dump" | "-t" => {
                dump = true;
            }
            "--run" | "-r" => {
                run_flag = true;
            }
            "--debug" | "-g" => {
                debug_mode = true;
                run_flag = true;
            }
            "--debug-mni" => {
                debug_mni = true;
            }
            "--stdin-from" => {
                if let Some(p) = args.next() {
                    stdin_from = Some(PathBuf::from(p));
                } else {
                    eprintln!("--stdin-from requires a file path");
                    std::process::exit(1);
                }
            }
            _ => {
                let lower = a.to_lowercase();
                if lower == "link" {
                    link_mode = true;
                } else if link_mode {
                    // collect link inputs until -o or end
                    if lower.starts_with("-") {
                        eprintln!("Unexpected option in link inputs: {}", a);
                        print_usage();
                        std::process::exit(1);
                    }
                    link_inputs.push(a);
                } else if lower.ends_with(".masm") || lower.ends_with(".masi") {
                    input = Some(PathBuf::from(a));
                } else {
                    eprintln!("Unknown argument: {}", a);
                    print_usage();
                    std::process::exit(1);
                }
            }
        }
    }

    if link_mode {
        if link_inputs.len() < 2 {
            eprintln!("link requires at least two .masi inputs");
            std::process::exit(1);
        }
        let out_path = output.unwrap_or_else(|| PathBuf::from("out.masi"));
        match linker::link_files(&link_inputs) {
            Ok(res) => {
                if let Err(e) = fs::write(&out_path, res.bytes) {
                    eprintln!("Failed to write {}: {}", out_path.display(), e);
                    std::process::exit(1);
                } else {
                    println!("Linked -> {}", out_path.display());
                }
            }
            Err(e) => {
                eprintln!("Link failed: {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    let Some(input_path) = input else {
        print_usage();
        std::process::exit(1);
    };
    if input_path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("masi"))
        .unwrap_or(false)
    {
        // Run, disassemble, or dump
        let path_str = input_path.to_string_lossy();
        let masi = match disassembler::load(&path_str) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("Failed to load MASI: {}", e);
                std::process::exit(1);
            }
        };
        if run_flag || (!disasm && !dump) {
            // Enable MNI timing instrumentation if requested
            if debug_mni {
                set_debug_print(true);
            }
            if debug_mode {
                #[cfg(feature = "ratatui_debug")]
                {
                    set_thread_debugger(Some(Box::new(RatatuiDebugger::new())));
                }
                #[cfg(not(feature = "ratatui_debug"))]
                {
                    set_thread_debugger(Some(Box::new(interpreter::TuiDebugger::new())));
                }
            }
            // Route IO via run_masi_with_io to support --stdin-from
            use std::io::{self, BufRead};
            let mut out = io::stdout();
            let mut err = io::stderr();
            let mut file_reader_opt;
            let input_reader: Option<&mut dyn BufRead> = if let Some(path) = stdin_from.as_ref() {
                match std::fs::File::open(path) {
                    Ok(f) => {
                        file_reader_opt = Some(io::BufReader::new(f));
                        file_reader_opt.as_mut().map(|br| br as &mut dyn BufRead)
                    }
                    Err(e) => {
                        eprintln!("Failed to open {}: {}", path.display(), e);
                        std::process::exit(1);
                    }
                }
            } else {
                None
            };
            match interpreter::run_masi_with_io(&masi, &mut out, &mut err, input_reader) {
                Ok(state) => {
                    // Print any runtime warnings collected
                    for w in state.warnings.iter() {
                        eprintln!("{}", w);
                    }
                }
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
            return;
        }
        if dump {
            println!("{}", disassembler::dump(&masi));
            return;
        }
        if disasm {
            let asm = disassembler::disassemble(&masi);
            if let Some(out) = output {
                if let Err(e) = fs::write(&out, asm.as_bytes()) {
                    eprintln!("error: failed to write {}: {}", out.display(), e);
                    std::process::exit(1);
                }
            } else {
                println!("{}", asm);
            }
            return;
        }
        // unreachable: covered above
    } else {
        // Assemble
        let src = match fs::read_to_string(&input_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: failed to read {}: {}", input_path.display(), e);
                std::process::exit(1);
            }
        };
        let base = input_path
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());
        match assembler::assemble_to_masi_with_base(&src, &base) {
            Ok(bytes) => {
                // If run or debug mode requested, run directly; otherwise save to file
                if run_flag || debug_mode {
                    // Enable MNI timing instrumentation if requested
                    if debug_mni {
                        set_debug_print(true);
                    }
                    match disassembler::parse_masi_bytes(&bytes) {
                        Ok(masi) => {
                            if debug_mode {
                                #[cfg(feature = "ratatui_debug")]
                                {
                                    set_thread_debugger(Some(Box::new(RatatuiDebugger::new())));
                                }
                                #[cfg(not(feature = "ratatui_debug"))]
                                {
                                    set_thread_debugger(Some(Box::new(CliDebugger::new())));
                                }
                            }
                            use std::io::{self, BufRead};
                            let mut out = io::stdout();
                            let mut err = io::stderr();
                            let mut file_reader_opt;
                            let input_reader: Option<&mut dyn BufRead> = if let Some(path) =
                                stdin_from.as_ref()
                            {
                                match std::fs::File::open(path) {
                                    Ok(f) => {
                                        file_reader_opt = Some(io::BufReader::new(f));
                                        file_reader_opt.as_mut().map(|br| br as &mut dyn BufRead)
                                    }
                                    Err(e) => {
                                        eprintln!("Failed to open {}: {}", path.display(), e);
                                        std::process::exit(1);
                                    }
                                }
                            } else {
                                None
                            };
                            match interpreter::run_masi_with_io(
                                &masi,
                                &mut out,
                                &mut err,
                                input_reader,
                            ) {
                                Ok(state) => {
                                    for w in state.warnings.iter() {
                                        eprintln!("{}", w);
                                    }
                                }
                                Err(e) => {
                                    eprintln!("{}", e);
                                    std::process::exit(1);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("error: failed to load assembled MASI: {}", e);
                            std::process::exit(1);
                        }
                    }
                } else {
                    let out_path = output.unwrap_or_else(|| PathBuf::from("out.masi"));
                    if let Err(e) = fs::write(&out_path, bytes) {
                        eprintln!("error: failed to write {}: {}", out_path.display(), e);
                        std::process::exit(1);
                    }
                    println!("Wrote MASI to {}", out_path.display());
                }
            }
            Err(e) => {
                eprintln!("error: assembly failed: {}", e);
                std::process::exit(1);
            }
        }
    }
}
