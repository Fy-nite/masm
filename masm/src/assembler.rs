use crate::register_map::RegisterMap;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

// Minimal MASI opcodes, mirrored from Swift
#[repr(u8)]
enum Op {
    Mov = 0x01,
    Add = 0x02,
    Sub = 0x03,
    Jmp = 0x10,
    Cmp = 0x11,
    Je = 0x12,
    Jne = 0x13,
    Jl = 0x14,
    Jle = 0x15,
    Jg = 0x16,
    Jge = 0x17,
    Call = 0x20,
    Ret = 0x21,
    Push = 0x30,
    Pop = 0x31,
    Out = 0x40,
    COut = 0x41,
    In = 0x42,
    Enter = 0x50,
    Leave = 0x51,
    Mni = 0x60,
    Syscall = 0x90,
    // Floating point block
    FMov = 0x70,
    FAdd = 0x71,
    FSub = 0x72,
    FMul = 0x73,
    FDiv = 0x74,
    FCmp = 0x75,
    FJe = 0x76,
    FJne = 0x77,
    FJlt = 0x78,
    FJle = 0x79,
    FJgt = 0x7A,
    FJge = 0x7B,
    FJuo = 0x7C,
    Hlt = 0xFF,
    Nop = 0x00,
}

#[derive(Debug, Clone)]
enum Instruction {
    Label(String),
    Mov(String, String),
    Add(String, String),
    Sub(String, String),
    Jmp(String),
    Cmp(String, String),
    Je(String),
    Jne(String),
    Jl(String),
    Jle(String),
    Jg(String),
    Jge(String),
    // Floating point
    FMov(String, String),
    FAdd(String, String),
    FSub(String, String),
    FMul(String, String),
    FDiv(String, String),
    FCmp(String, String),
    FJe(String),
    FJne(String),
    FJlt(String),
    FJle(String),
    FJgt(String),
    FJge(String),
    FJuo(String),
    Call(String),
    Ret,
    Push(String),
    Pop(String),
    Out(String, String),
    COut(String, String),
    In(String),
    Enter(String),
    Leave,
    Mni {
        module_ptr: String,
        function_ptr: String,
        args: Vec<String>,
    },
    Syscall,
    Hlt,
    Nop,
}

#[derive(Default, Debug, Clone)]
struct DataSections {
    directives: Vec<DataDirective>,
    data_label_offsets: HashMap<String, usize>,
    mni_string_labels: HashMap<String, String>, // content -> gen label
    // New: export/import support
    export_symbols: Vec<String>, // raw names (may start with '#' for code or '$' for data)
    import_kinds: HashMap<String, u8>, // name -> kind (0=code, 1=data)
    relocations: Vec<Reloc>,     // collected during encode
}

#[derive(Debug, Clone)]
struct Reloc {
    name: String,  // symbol name (without prefix)
    kind: u8,      // 0=code, 1=data
    section: u8,   // 0=code for now
    offset: usize, // where to write u64 in section
}

#[derive(Debug, Clone)]
enum DataValue {
    Imm(u64),
    Sym(String),
}

#[derive(Debug, Clone)]
enum DataDirective {
    Db(Option<String>, Vec<u8>),
    Dw(Option<String>, Vec<u16>),
    Dd(Option<String>, Vec<u32>),
    Dq(Option<String>, Vec<DataValue>),
    Df(Option<String>, Vec<f32>),
    Ddbl(Option<String>, Vec<f64>),
    Res(Option<String>, usize),
    DirectDb {
        address: usize,
        bytes: Vec<u8>,
        null_terminated: bool,
    },
}

fn is_register(s: &str, reg_map: &HashMap<String, u16>) -> bool {
    reg_map.contains_key(&s.to_uppercase())
}

fn parse_string_literal(tok: &str) -> Option<Vec<u8>> {
    let bytes = tok.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'"' || *bytes.last().unwrap() != b'"' {
        return None;
    }
    let inner = &tok[1..tok.len() - 1];
    let mut out: Vec<u8> = Vec::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(n) = chars.next() {
                match n {
                    'n' => out.push(10),
                    'r' => out.push(13),
                    't' => out.push(9),
                    '\\' => out.push(92),
                    '"' => out.push(34),
                    '0' => out.push(0),
                    other => out.push(other as u32 as u8),
                }
            } else {
                break;
            }
        } else {
            out.push(c as u32 as u8);
        }
    }
    Some(out)
}

fn parse_data_line(line: &str, data: &mut DataSections) -> bool {
    let mut label_name: Option<String> = None;
    let mut rest = line.trim().to_string();
    // Only treat a leading "label:" as a label definition; ignore ':' inside strings or after whitespace
    {
        let t = rest.as_str();
        // Find first ':' that appears before any whitespace and outside of quotes
        let mut in_str = false;
        let mut idx_colon: Option<usize> = None;
        for (i, ch) in t.char_indices() {
            match ch {
                '"' => {
                    in_str = !in_str;
                }
                ':' if !in_str => {
                    idx_colon = Some(i);
                    break;
                }
                ' ' | '\t' => {
                    if !in_str {
                        break;
                    }
                }
                _ => {}
            }
        }
        if let Some(ci) = idx_colon {
            // Ensure no whitespace in the label prefix
            let prefix = t[..ci].trim();
            if !prefix.is_empty() && !prefix.contains(' ') && !prefix.contains('\t') {
                label_name = Some(prefix.to_string());
                rest = t[ci + 1..].trim().to_string();
            }
        }
    }
    let lower = rest.to_lowercase();
    // Handle simple preprocessor export/import directives here so they don't become instructions
    if lower.starts_with("#export ") {
        let name = rest[8..].trim().to_string();
        if !name.is_empty() {
            data.export_symbols.push(name);
        }
        return true;
    }
    if lower.starts_with("#import ") {
        // Forms:
        //   #import code foo
        //   #import data bar
        //   #import foo           (defaults to code)
        let args = rest[8..].trim();
        let mut parts = args.split_whitespace();
        if let Some(a) = parts.next() {
            let (kind, name) = match a.to_ascii_lowercase().as_str() {
                "code" => (0u8, parts.next().unwrap_or("").to_string()),
                "data" => (1u8, parts.next().unwrap_or("").to_string()),
                _ => (0u8, a.to_string()),
            };
            if !name.is_empty() {
                data.import_kinds.insert(name, kind);
            } else if !a.is_empty() && (a == "code" || a == "data") == false {
                // Already handled in default case above
            }
        }
        return true;
    }
    let parse_values =
        |part: &str| -> Vec<String> { part.split(',').map(|s| s.trim().to_string()).collect() };

    if lower.starts_with("state ") {
        // Syntax: STATE name <TYPE> value
        // TYPE: BYTE, WORD, DWORD, QWORD, FLOAT, DOUBLE
        let rest = rest[6..].trim().to_string();
        // Split into name, type, value(s)
        // name <TYPE> value
        let mut parts = rest.split_whitespace();
        if let Some(name) = parts.next() {
            let after_name = rest[name.len()..].trim();
            if let Some(start) = after_name.find('<') {
                if let Some(end) = after_name[start + 1..].find('>') {
                    let ty = &after_name[start + 1..start + 1 + end];
                    let value_str = after_name[start + 1 + end + 1..].trim();
                    match ty.to_ascii_uppercase().as_str() {
                        "BYTE" => {
                            if let Ok(v) = value_str.parse::<i64>() {
                                data.directives
                                    .push(DataDirective::Db(Some(name.to_string()), vec![v as u8]));
                                return true;
                            }
                        }
                        "WORD" => {
                            if let Ok(v) = value_str.parse::<i64>() {
                                data.directives.push(DataDirective::Dw(
                                    Some(name.to_string()),
                                    vec![v as u16],
                                ));
                                return true;
                            }
                        }
                        "DWORD" => {
                            if let Ok(v) = value_str.parse::<i64>() {
                                data.directives.push(DataDirective::Dd(
                                    Some(name.to_string()),
                                    vec![v as u32],
                                ));
                                return true;
                            }
                        }
                        "QWORD" => {
                            if let Ok(v) = value_str.parse::<i128>() {
                                data.directives.push(DataDirective::Dq(
                                    Some(name.to_string()),
                                    vec![DataValue::Imm(v as u64)],
                                ));
                                return true;
                            }
                        }
                        "STRING" => {
                            // value_str expected to be a string literal like "hello" or empty
                            if let Some(bytes) = parse_string_literal(value_str) {
                                // create a generated label for this string and store bytes as a Db entry
                                let mut gen_label = String::from("__str_");
                                gen_label.push_str(&uuid_like::short_hash(
                                    &bytes
                                        .iter()
                                        .map(|b| format!("{:02x}", b))
                                        .collect::<String>(),
                                ));
                                // ensure null-terminated
                                let mut b2 = bytes.clone();
                                b2.push(0);
                                // push the bytes as a Db with the generated label
                                data.directives
                                    .push(DataDirective::Db(Some(gen_label.clone()), b2));
                                // now push a Dq with a symbolic reference to the generated label
                                data.directives.push(DataDirective::Dq(
                                    Some(name.to_string()),
                                    vec![DataValue::Sym(gen_label)],
                                ));
                                return true;
                            }
                        }
                        "PTR" => {
                            // PTR is an alias for an address-sized state variable; initialize to 0 unless a value is provided
                            if value_str.is_empty() {
                                data.directives.push(DataDirective::Dq(
                                    Some(name.to_string()),
                                    vec![DataValue::Imm(0u64)],
                                ));
                                return true;
                            } else if let Ok(v) = value_str.parse::<i128>() {
                                data.directives.push(DataDirective::Dq(
                                    Some(name.to_string()),
                                    vec![DataValue::Imm(v as u64)],
                                ));
                                return true;
                            }
                        }
                        "FLOAT" => {
                            if let Ok(v) = value_str.parse::<f32>() {
                                data.directives
                                    .push(DataDirective::Df(Some(name.to_string()), vec![v]));
                                return true;
                            }
                        }
                        "DOUBLE" => {
                            if let Ok(v) = value_str.parse::<f64>() {
                                data.directives
                                    .push(DataDirective::Ddbl(Some(name.to_string()), vec![v]));
                                return true;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        return false;
    } else if lower.starts_with("db ") {
        let rhs = rest[3..].trim().to_string();
        if rhs.starts_with('$') {
            let mut parts = rhs.splitn(2, ' ');
            let a = parts.next().unwrap();
            let b = parts.next();
            if let Some(b) = b {
                let addr_str = &a[1..];
                let addr = if addr_str.to_lowercase().starts_with("0x") {
                    u64::from_str_radix(&addr_str[2..], 16).unwrap_or(0) as usize
                } else {
                    addr_str.parse::<usize>().unwrap_or(0)
                };
                if let Some(bytes) = parse_string_literal(b) {
                    data.directives.push(DataDirective::DirectDb {
                        address: addr,
                        bytes,
                        null_terminated: true,
                    });
                    return true;
                }
            }
        }
        let mut all: Vec<u8> = Vec::new();
        for tok in parse_values(&rhs) {
            if let Some(sb) = parse_string_literal(&tok) {
                all.extend_from_slice(&sb);
            } else if tok.to_lowercase().starts_with("0x") {
                if let Ok(v) = u8::from_str_radix(&tok[2..], 16) {
                    all.push(v);
                }
            } else if let Ok(v) = tok.parse::<i64>() {
                all.push(v as u8);
            }
        }
        data.directives.push(DataDirective::Db(label_name, all));
        return true;
    } else if lower.starts_with("dw ") {
        let rhs = rest[3..].to_string();
        let mut vals: Vec<u16> = Vec::new();
        for tok in parse_values(&rhs) {
            if tok.to_lowercase().starts_with("0x") {
                if let Ok(v) = u16::from_str_radix(&tok[2..], 16) {
                    vals.push(v);
                }
            } else if let Ok(v) = tok.parse::<i64>() {
                vals.push(v as u16);
            }
        }
        data.directives.push(DataDirective::Dw(label_name, vals));
        return true;
    } else if lower.starts_with("dd ") {
        let rhs = rest[3..].to_string();
        let mut vals: Vec<u32> = Vec::new();
        for tok in parse_values(&rhs) {
            if tok.to_lowercase().starts_with("0x") {
                if let Ok(v) = u32::from_str_radix(&tok[2..], 16) {
                    vals.push(v);
                }
            } else if let Ok(v) = tok.parse::<i64>() {
                vals.push(v as u32);
            }
        }
        data.directives.push(DataDirective::Dd(label_name, vals));
        return true;
    } else if lower.starts_with("dq ") {
        let rhs = rest[3..].to_string();
        let mut vals: Vec<DataValue> = Vec::new();
        for tok in parse_values(&rhs) {
            if tok.starts_with('#') { /* not supported */
            } else if tok.to_lowercase().starts_with("0x") {
                if let Ok(v) = u64::from_str_radix(&tok[2..], 16) {
                    vals.push(DataValue::Imm(v));
                }
            } else if let Ok(v) = tok.parse::<u64>() {
                vals.push(DataValue::Imm(v));
            } else {
                // treat as symbolic reference
                vals.push(DataValue::Sym(tok));
            }
        }
        data.directives.push(DataDirective::Dq(label_name, vals));
        return true;
    } else if lower.starts_with("df ") {
        let rhs = rest[3..].to_string();
        let mut vals: Vec<f32> = Vec::new();
        for tok in parse_values(&rhs) {
            if let Ok(v) = tok.parse::<f32>() {
                vals.push(v);
            }
        }
        data.directives.push(DataDirective::Df(label_name, vals));
        return true;
    } else if lower.starts_with("ddbl ") {
        let rhs = rest[5..].to_string();
        let mut vals: Vec<f64> = Vec::new();
        for tok in parse_values(&rhs) {
            if let Ok(v) = tok.parse::<f64>() {
                vals.push(v);
            }
        }
        data.directives.push(DataDirective::Ddbl(label_name, vals));
        return true;
    } else if lower.starts_with("resb ")
        || lower.starts_with("resw ")
        || lower.starts_with("resd ")
        || lower.starts_with("resq ")
        || lower.starts_with("resf ")
        || lower.starts_with("resdbl ")
    {
        let mut parts = rest.splitn(2, ' ');
        let kind = parts.next().unwrap().to_lowercase();
        let count: usize = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let factor = match kind.as_str() {
            "resb" => 1,
            "resw" => 2,
            "resd" => 4,
            "resq" => 8,
            "resf" => 4,
            "resdbl" => 8,
            _ => 1,
        };
        data.directives
            .push(DataDirective::Res(label_name, count * factor));
        return true;
    }
    false
}

fn expand_macros(src: &str) -> String {
    // Very small macro expander: MACRO name args... ... ENDMACRO, with @@local labels uniquified.
    #[derive(Clone)]
    struct MacroDef {
        name: String,
        params: Vec<String>,
        body: Vec<String>,
    }
    let mut defs: HashMap<String, MacroDef> = HashMap::new();
    let mut out_lines: Vec<String> = Vec::new();
    let lines: Vec<String> = src.lines().map(|s| s.to_string()).collect();
    let mut i = 0;
    while i < lines.len() {
        let mut line = lines[i].clone();
        if let Some(idx) = line.find(';') {
            line.truncate(idx);
        }
        let trimmed = line.trim();
        if trimmed.to_lowercase().starts_with("macro ") {
            // Parse header: MACRO name [args...]
            let head = trimmed[6..].trim();
            let mut parts = head.split_whitespace();
            let name = parts.next().unwrap_or("").to_lowercase();
            let params: Vec<String> = parts
                .map(|s| s.trim_matches(&[','][..]).to_string())
                .filter(|s| !s.is_empty())
                .collect();
            i += 1;
            let mut body: Vec<String> = Vec::new();
            while i < lines.len() {
                let mut ln = lines[i].clone();
                if let Some(idx) = ln.find(';') {
                    ln.truncate(idx);
                }
                if ln.trim().eq_ignore_ascii_case("endmacro") {
                    break;
                }
                body.push(lines[i].clone());
                i += 1;
            }
            // Skip ENDMACRO line
            while i < lines.len() {
                if lines[i].trim().eq_ignore_ascii_case("endmacro") {
                    break;
                }
                i += 1;
            }
            if i < lines.len() {
                i += 1;
            }
            defs.insert(name.clone(), MacroDef { name, params, body });
            continue;
        }
        // Not a macro def; emit as-is for now; expansion pass later while building
        out_lines.push(lines[i].clone());
        i += 1;
    }
    // Second pass: expand invocations
    let mut expanded: Vec<String> = Vec::new();
    let mut counter: u64 = 0;
    for raw in out_lines {
        let mut ln = raw.clone();
        if let Some(idx) = ln.find(';') {
            ln.truncate(idx);
        }
        let trimmed = ln.trim();
        if trimmed.is_empty() {
            expanded.push(raw);
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let head = parts.next().unwrap();
        let name = head.to_lowercase();
        if let Some(def) = defs.get(&name) {
            let args: Vec<String> = parts
                .map(|s| s.trim_matches(&[','][..]).to_string())
                .collect();
            if args.len() != def.params.len() {
                expanded.push(raw);
                continue;
            }
            counter += 1;
            let uniq = format!("..@{}@{}@", def.name, counter);
            for body_line in &def.body {
                let mut line = body_line.clone();
                // Parameter substitution (simple textual)
                for (p, a) in def.params.iter().zip(args.iter()) {
                    line = line.replace(p, a);
                }
                // Local labels @@label -> uniquified
                let mut tmp = String::new();
                let mut chars = line.chars().peekable();
                while let Some(ch) = chars.next() {
                    if ch == '@' && chars.peek() == Some(&'@') {
                        chars.next(); // consume second @
                        tmp.push_str(&uniq);
                    } else {
                        tmp.push(ch);
                    }
                }
                // If the expanded line is a bare label (e.g., "..@macro@1@label:"), convert to "LBL <name>"
                let trimmed = tmp.trim();
                if trimmed.ends_with(':') && !trimmed.contains(' ') {
                    let name = &trimmed[..trimmed.len() - 1];
                    expanded.push(format!("LBL {}", name));
                } else {
                    expanded.push(tmp);
                }
            }
        } else {
            expanded.push(raw);
        }
    }
    expanded.join("\n")
}

fn expand_includes(src: &str, base_dir: Option<&Path>) -> Result<String, String> {
    // Supports: #include "path"
    // Recursively expand includes; resolves relative paths against base_dir or current_dir.
    fn resolve_path(p: &str, base: Option<&Path>) -> PathBuf {
        let path = Path::new(p);
        if path.is_absolute() {
            return path.to_path_buf();
        }
        if let Some(b) = base {
            return b.join(path);
        }
        if let Ok(cwd) = std::env::current_dir() {
            return cwd.join(path);
        }
        path.to_path_buf()
    }
    fn expand_recursive(src: &str, base: Option<&Path>, depth: usize) -> Result<String, String> {
        if depth > 32 {
            return Err(format!(
                "error[EASM004]: include depth exceeds limit (depth={})",
                depth
            ));
        }
        let mut out: String = String::new();
        for raw in src.lines() {
            let mut line = raw.to_string();
            if let Some(idx) = line.find(';') {
                line.truncate(idx);
            }
            let trimmed = line.trim();
            if trimmed.to_lowercase().starts_with("#include ") {
                // Expect #include "path"
                let rest = trimmed[9..].trim();
                let path_str = rest.trim_matches(|c| c == '"' || c == '<' || c == '>' || c == ' ');
                let abs = resolve_path(path_str, base);
                let content = fs::read_to_string(&abs)
                    .map_err(|e| format!("Failed to read include '{}': {}", abs.display(), e))?;
                let next_base = abs.parent();
                let expanded = expand_recursive(&content, next_base, depth + 1)?;
                out.push_str(&expanded);
                out.push('\n');
            } else {
                out.push_str(raw);
                out.push('\n');
            }
        }
        Ok(out)
    }
    expand_recursive(src, base_dir, 0)
}

fn parse_instructions(src: &str, base_dir: Option<&Path>) -> (Vec<Instruction>, DataSections) {
    // Includes should be expanded by the caller; only handle macros here.
    let src = expand_macros(src);
    let mut insts: Vec<Instruction> = Vec::new();
    let mut data = DataSections::default();

    for raw in src.lines() {
        let mut line = raw.to_string();
        if let Some(idx) = line.find(';') {
            line.truncate(idx);
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Binary include helpers: #include_bytes label, "path" | #include_dq label, "path"
        let lower = trimmed.to_lowercase();
        if lower.starts_with("#include_bytes ") || lower.starts_with("#include_dq ") {
            let is_dq = lower.starts_with("#include_dq ");
            // Prefix lengths (including trailing space): "#include_bytes " = 15, "#include_dq " = 12
            let rest = if is_dq {
                &trimmed[12..]
            } else {
                &trimmed[15..]
            };
            // Expect: label, "path"
            let mut parts = rest.splitn(2, ',');
            let label = parts
                .next()
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            let path_part = parts.next().map(|s| s.trim()).unwrap_or("");
            let path_str = path_part.trim_matches(|c| c == '"' || c == '<' || c == '>' || c == ' ');
            let base = base_dir
                .and_then(|p| Some(p.to_path_buf()))
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("."));
            let abs = if Path::new(path_str).is_absolute() {
                PathBuf::from(path_str)
            } else {
                base.join(path_str)
            };
            match fs::read(&abs) {
                Ok(bytes) => {
                    if is_dq {
                        // Emit as DQ words (pad to 8-bytes)
                        let mut qws: Vec<DataValue> = Vec::new();
                        let mut buf = bytes.clone();
                        while buf.len() % 8 != 0 {
                            buf.push(0);
                        }
                        for chunk in buf.chunks(8) {
                            let mut arr = [0u8; 8];
                            arr.copy_from_slice(chunk);
                            qws.push(DataValue::Imm(u64::from_le_bytes(arr)));
                        }
                        data.directives
                            .push(DataDirective::Dq(Some(label.clone()), qws));
                    } else {
                        let bytes_clone = bytes.clone();
                        let byte_len = bytes_clone.len() as u64;
                        data.directives
                            .push(DataDirective::Db(Some(label.clone()), bytes_clone));
                        // Always emit <label>_len as DQ of total byte length (from in-memory bytes)
                        data.directives.push(DataDirective::Dq(
                            Some(format!("{}_len", label)),
                            vec![DataValue::Imm(byte_len)],
                        ));
                        continue;
                    }
                    // Always emit <label>_len as DQ of total byte length (from in-memory bytes)
                    let byte_len = bytes.len() as u64;
                    data.directives.push(DataDirective::Dq(
                        Some(format!("{}_len", label)),
                        vec![DataValue::Imm(byte_len)],
                    ));
                }
                Err(e) => {
                    eprintln!("Failed to read include data '{}': {}", abs.display(), e);
                }
            }
            continue;
        }
        if parse_data_line(trimmed, &mut data) {
            continue;
        }
        let mut parts = trimmed.splitn(2, ' ');
        let mnemonic = parts.next().unwrap().to_lowercase();
        let operands: Vec<String> = parts
            .next()
            .map(|s| {
                s.split(' ')
                    .map(|t| t.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        match (mnemonic.as_str(), operands.len()) {
            ("lbl", 1) | ("label", 1) => insts.push(Instruction::Label(operands[0].clone())),
            ("mov", 2) => insts.push(Instruction::Mov(operands[0].clone(), operands[1].clone())),
            ("add", 2) => insts.push(Instruction::Add(operands[0].clone(), operands[1].clone())),
            ("sub", 2) => insts.push(Instruction::Sub(operands[0].clone(), operands[1].clone())),
            ("inc", 1) => insts.push(Instruction::Add(operands[0].clone(), "1".to_string())),
            ("dec", 1) => insts.push(Instruction::Sub(operands[0].clone(), "1".to_string())),
            ("jmp", 1) => insts.push(Instruction::Jmp(operands[0].clone())),
            ("cmp", 2) => insts.push(Instruction::Cmp(operands[0].clone(), operands[1].clone())),
            ("je", 1) => insts.push(Instruction::Je(operands[0].clone())),
            ("jeq", 1) => insts.push(Instruction::Je(operands[0].clone())),
            ("jz", 1) => insts.push(Instruction::Je(operands[0].clone())),
            ("jne", 1) => insts.push(Instruction::Jne(operands[0].clone())),
            ("jnz", 1) => insts.push(Instruction::Jne(operands[0].clone())),
            ("jl", 1) => insts.push(Instruction::Jl(operands[0].clone())),
            ("jle", 1) => insts.push(Instruction::Jle(operands[0].clone())),
            ("jg", 1) => insts.push(Instruction::Jg(operands[0].clone())),
            ("jge", 1) => insts.push(Instruction::Jge(operands[0].clone())),
            // Floating point
            ("fmov", 2) => insts.push(Instruction::FMov(operands[0].clone(), operands[1].clone())),
            ("fadd", 2) => insts.push(Instruction::FAdd(operands[0].clone(), operands[1].clone())),
            ("fsub", 2) => insts.push(Instruction::FSub(operands[0].clone(), operands[1].clone())),
            ("fmul", 2) => insts.push(Instruction::FMul(operands[0].clone(), operands[1].clone())),
            ("fdiv", 2) => insts.push(Instruction::FDiv(operands[0].clone(), operands[1].clone())),
            ("fcmp", 2) => insts.push(Instruction::FCmp(operands[0].clone(), operands[1].clone())),
            ("fje", 1) => insts.push(Instruction::FJe(operands[0].clone())),
            ("fjne", 1) => insts.push(Instruction::FJne(operands[0].clone())),
            ("fjlt", 1) => insts.push(Instruction::FJlt(operands[0].clone())),
            ("fjle", 1) => insts.push(Instruction::FJle(operands[0].clone())),
            ("fjgt", 1) => insts.push(Instruction::FJgt(operands[0].clone())),
            ("fjge", 1) => insts.push(Instruction::FJge(operands[0].clone())),
            ("fjuo", 1) => insts.push(Instruction::FJuo(operands[0].clone())),
            ("call", 1) => insts.push(Instruction::Call(operands[0].clone())),
            ("ret", 0) => insts.push(Instruction::Ret),
            ("push", 1) => insts.push(Instruction::Push(operands[0].clone())),
            ("pop", 1) => insts.push(Instruction::Pop(operands[0].clone())),
            ("out", 2) => insts.push(Instruction::Out(operands[0].clone(), operands[1].clone())),
            ("cout", 2) => insts.push(Instruction::COut(operands[0].clone(), operands[1].clone())),
            ("in", 1) => insts.push(Instruction::In(operands[0].clone())),
            ("hlt", 0) => insts.push(Instruction::Hlt),
            ("nop", 0) => insts.push(Instruction::Nop),
            ("enter", 1) => insts.push(Instruction::Enter(operands[0].clone())),
            ("leave", 0) => insts.push(Instruction::Leave),
            ("mni", n) if n >= 1 => {
                if operands.len() >= 2
                    && operands[0].starts_with('$')
                    && operands[1].starts_with('$')
                {
                    let extra = if operands.len() > 2 {
                        operands[2..].to_vec()
                    } else {
                        vec![]
                    };
                    insts.push(Instruction::Mni {
                        module_ptr: operands[0].clone(),
                        function_ptr: operands[1].clone(),
                        args: extra,
                    });
                } else {
                    let name = &operands[0];
                    let mut parts = name.splitn(2, '.');
                    let module = parts.next().unwrap_or(name);
                    let function = parts.next().unwrap_or("main");
                    let raw_args = if operands.len() > 1 {
                        operands[1..].to_vec()
                    } else {
                        vec![]
                    };
                    fn safe_label(s: &str) -> String {
                        let mut out = String::from("__mni_str_");
                        out.push_str(&format!("{}_", uuid_like::short_hash(s)));
                        out.push_str(
                            &s.chars()
                                .map(|c| {
                                    if c.is_ascii_alphanumeric() || c == '_' {
                                        c
                                    } else {
                                        '_'
                                    }
                                })
                                .collect::<String>(),
                        );
                        out
                    }
                    let mod_lbl = if let Some(lbl) = data.mni_string_labels.get(module) {
                        lbl.clone()
                    } else {
                        let lbl = safe_label(module);
                        let mut bytes = module.as_bytes().to_vec();
                        bytes.push(0);
                        data.directives
                            .push(DataDirective::Db(Some(lbl.clone()), bytes));
                        data.mni_string_labels
                            .insert(module.to_string(), lbl.clone());
                        lbl
                    };
                    let fn_lbl = if let Some(lbl) = data.mni_string_labels.get(function) {
                        lbl.clone()
                    } else {
                        let lbl = safe_label(function);
                        let mut bytes = function.as_bytes().to_vec();
                        bytes.push(0);
                        data.directives
                            .push(DataDirective::Db(Some(lbl.clone()), bytes));
                        data.mni_string_labels
                            .insert(function.to_string(), lbl.clone());
                        lbl
                    };
                    let mut arg_labels: Vec<String> = Vec::new();
                    for a in raw_args.iter() {
                        let lbl = if let Some(l) = data.mni_string_labels.get(a) {
                            l.clone()
                        } else {
                            let l = safe_label(a);
                            let mut bytes = a.as_bytes().to_vec();
                            bytes.push(0);
                            data.directives
                                .push(DataDirective::Db(Some(l.clone()), bytes));
                            data.mni_string_labels.insert(a.clone(), l.clone());
                            l
                        };
                        arg_labels.push(lbl);
                    }
                    insts.push(Instruction::Mni {
                        module_ptr: format!("${}", mod_lbl),
                        function_ptr: format!("${}", fn_lbl),
                        args: arg_labels,
                    });
                }
            }
            ("syscall", 0) => insts.push(Instruction::Syscall),
            _ => {
                eprintln!(
                    "error[EASM001]: unrecognized instruction or operand count\n  --> <input>: ?\n   = help: got: '{}'",
                    trimmed
                );
            }
        }
    }
    (insts, data)
}

fn write_u16_le(v: u16, out: &mut Vec<u8>) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn write_u32_le(v: u32, out: &mut Vec<u8>) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn write_u64_le(v: u64, out: &mut Vec<u8>) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn encode_operand(
    s: &str,
    labels: &HashMap<String, usize>,
    data_labels: &HashMap<String, usize>,
    reg_map: &HashMap<String, u16>,
) -> (u8, u64) {
    // Floats as immediate: detect standard float literal forms
    if s.contains('.') || s.contains('e') || s.contains('E') {
        if let Ok(f) = s.parse::<f64>() {
            return (0, f.to_bits());
        }
    }
    if let Some(name) = s.strip_prefix('#') {
        if let Some(off) = labels.get(name) {
            return (2, *off as u64);
        }
        return (2, 0);
    }
    if let Some(rest) = s.strip_prefix('$') {
        let up = rest.to_uppercase();
        if let Some(&id) = reg_map.get(&up) {
            return (4, id as u64);
        }
        if let Some(&off) = data_labels.get(rest) {
            return (3, off as u64);
        }
        if let Some(hex) = rest.strip_prefix("0x") {
            if let Ok(v) = u64::from_str_radix(hex, 16) {
                return (3, v);
            }
        }
        if let Ok(v) = rest.parse::<u64>() {
            return (3, v);
        }
        return (3, 0);
    }
    if let Some(&id) = reg_map.get(&s.to_uppercase()) {
        return (1, id as u64);
    }
    // Plain code label name (from macros or convenience): treat as jump target
    if let Some(&off) = labels.get(s) {
        return (2, off as u64);
    }
    if let Some(&off) = data_labels.get(s) {
        return (0, off as u64);
    }
    if let Some(hex) = s.strip_prefix("0x") {
        if let Ok(v) = u64::from_str_radix(hex, 16) {
            return (0, v);
        }
    }
    if let Ok(v) = s.parse::<u64>() {
        return (0, v);
    }
    (0, 0)
}

// Helper: write an operand into code, generating relocations for unresolved imports.
fn emit_operand_into(
    code: &mut Vec<u8>,
    operand: &str,
    labels: &HashMap<String, usize>,
    data_labels: &HashMap<String, usize>,
    reg_map: &HashMap<String, u16>,
    import_kinds: &HashMap<String, u8>,
    relocs: &mut Vec<Reloc>,
) {
    let (mode, val, reloc): (u8, u64, Option<(String, u8)>) = {
        if let Some(name) = operand.strip_prefix('#') {
            if !labels.contains_key(name) {
                (2, 0, Some((name.to_string(), 0)))
            } else {
                let (m, v) = encode_operand(operand, labels, data_labels, reg_map);
                (m, v, None)
            }
        } else if let Some(rest) = operand.strip_prefix('$') {
            let up = rest.to_uppercase();
            if reg_map.contains_key(&up) {
                let (m, v) = encode_operand(operand, labels, data_labels, reg_map);
                (m, v, None)
            } else if !data_labels.contains_key(rest) {
                // If it's a numeric literal (hex or dec), treat as direct memory address, not import
                if let Some(hex) = rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")) {
                    if let Ok(v) = u64::from_str_radix(hex, 16) {
                        (3, v, None)
                    } else {
                        (3, 0, Some((rest.to_string(), 1)))
                    }
                } else if rest.chars().all(|c| c.is_ascii_digit()) {
                    if let Ok(v) = rest.parse::<u64>() {
                        (3, v, None)
                    } else {
                        (3, 0, Some((rest.to_string(), 1)))
                    }
                } else {
                    (3, 0, Some((rest.to_string(), 1)))
                }
            } else {
                let (m, v) = encode_operand(operand, labels, data_labels, reg_map);
                (m, v, None)
            }
        } else {
            if labels.contains_key(operand)
                || data_labels.contains_key(operand)
                || reg_map.contains_key(&operand.to_uppercase())
                || operand.starts_with("0x")
                || operand.chars().all(|c| c.is_ascii_digit())
            {
                let (m, v) = encode_operand(operand, labels, data_labels, reg_map);
                (m, v, None)
            } else if let Some(k) = import_kinds.get(operand).copied() {
                let mode = if k == 0 { 2 } else { 3 };
                (mode, 0, Some((operand.to_string(), k)))
            } else {
                let (m, v) = encode_operand(operand, labels, data_labels, reg_map);
                (m, v, None)
            }
        }
    };
    code.push(mode);
    let patch_off = code.len();
    write_u64_le(val, code);
    if let Some((name, kind)) = reloc {
        relocs.push(Reloc {
            name,
            kind,
            section: 0,
            offset: patch_off,
        });
    }
}

pub fn assemble_to_masi(src: &str) -> Result<Vec<u8>, String> {
    let reg_map = RegisterMap::build_name_to_id();
    // Expand includes first to error early on missing files
    let expanded = expand_includes(src, None)?;
    let (insts, mut data) = parse_instructions(&expanded, None);
    let mut diag_errors: Vec<String> = Vec::new();

    // First pass: compute labels and code size to get entry offset
    let mut labels: HashMap<String, usize> = HashMap::new();
    let mut pc: usize = 0;
    let mut entry: Option<usize> = None;
    for ins in &insts {
        match ins {
            Instruction::Label(name) => {
                if labels.contains_key(name) {
                    diag_errors.push(format!("error[EASM002]: duplicate code label '{}'", name));
                }
                labels.insert(name.clone(), pc);
            }
            Instruction::Ret | Instruction::Hlt | Instruction::Nop | Instruction::Leave => {
                if entry.is_none() {
                    entry = Some(pc);
                }
                pc += 1;
            }
            Instruction::Syscall => {
                if entry.is_none() {
                    entry = Some(pc);
                }
                pc += 1;
            }
            Instruction::Enter(_) => {
                if entry.is_none() {
                    entry = Some(pc);
                }
                pc += 1 + (1 + 8);
            }
            Instruction::Jmp(_)
            | Instruction::Je(_)
            | Instruction::Jne(_)
            | Instruction::Jl(_)
            | Instruction::Jle(_)
            | Instruction::Jg(_)
            | Instruction::Jge(_)
            | Instruction::Call(_)
            | Instruction::FJe(_)
            | Instruction::FJne(_)
            | Instruction::FJlt(_)
            | Instruction::FJle(_)
            | Instruction::FJgt(_)
            | Instruction::FJge(_)
            | Instruction::FJuo(_) => {
                if entry.is_none() {
                    entry = Some(pc);
                }
                pc += 1 + 1 + 8;
            }
            Instruction::Push(_) | Instruction::Pop(_) | Instruction::In(_) => {
                if entry.is_none() {
                    entry = Some(pc);
                }
                pc += 1 + 1 + 8;
            }
            Instruction::Out(_, _)
            | Instruction::COut(_, _)
            | Instruction::Mov(_, _)
            | Instruction::Add(_, _)
            | Instruction::Sub(_, _)
            | Instruction::Cmp(_, _)
            | Instruction::FMov(_, _)
            | Instruction::FAdd(_, _)
            | Instruction::FSub(_, _)
            | Instruction::FMul(_, _)
            | Instruction::FDiv(_, _)
            | Instruction::FCmp(_, _) => {
                if entry.is_none() {
                    entry = Some(pc);
                }
                pc += 1 + (1 + 8) + (1 + 8);
            }
            Instruction::Mni { args, .. } => {
                if entry.is_none() {
                    entry = Some(pc);
                }
                pc += 1 + (1 + 8) + (1 + 8) + 2 + (args.len() * (1 + 8));
            }
        }
    }

    // Build data section from directives
    let mut data_bytes: Vec<u8> = Vec::new();
    for d in &data.directives {
        match d {
            DataDirective::Db(label, bytes) => {
                if let Some(name) = label {
                    if data.data_label_offsets.contains_key(name) {
                        diag_errors
                            .push(format!("error[EASM002]: duplicate data label '{}'", name));
                    }
                    data.data_label_offsets
                        .insert(name.clone(), data_bytes.len());
                }
                data_bytes.extend_from_slice(bytes);
            }
            DataDirective::Dw(label, words) => {
                if let Some(name) = label {
                    if data.data_label_offsets.contains_key(name) {
                        diag_errors
                            .push(format!("error[EASM002]: duplicate data label '{}'", name));
                    }
                    data.data_label_offsets
                        .insert(name.clone(), data_bytes.len());
                }
                for w in words {
                    data_bytes.extend_from_slice(&w.to_le_bytes());
                }
            }
            DataDirective::Dd(label, dws) => {
                if let Some(name) = label {
                    if data.data_label_offsets.contains_key(name) {
                        diag_errors
                            .push(format!("error[EASM002]: duplicate data label '{}'", name));
                    }
                    data.data_label_offsets
                        .insert(name.clone(), data_bytes.len());
                }
                for w in dws {
                    data_bytes.extend_from_slice(&w.to_le_bytes());
                }
            }
            DataDirective::Dq(label, qws) => {
                if let Some(name) = label {
                    if data.data_label_offsets.contains_key(name) {
                        diag_errors
                            .push(format!("error[EASM002]: duplicate data label '{}'", name));
                    }
                    data.data_label_offsets
                        .insert(name.clone(), data_bytes.len());
                }
                for dv in qws.iter() {
                    match dv {
                        &DataValue::Imm(v) => {
                            data_bytes.extend_from_slice(&v.to_le_bytes());
                        }
                        &DataValue::Sym(ref s) => {
                            if let Some(off) = data.data_label_offsets.get(s) {
                                data_bytes.extend_from_slice(&(*off as u64).to_le_bytes());
                            } else {
                                data_bytes.extend_from_slice(&0u64.to_le_bytes());
                            }
                        }
                    }
                }
            }
            DataDirective::Df(label, floats) => {
                if let Some(name) = label {
                    if data.data_label_offsets.contains_key(name) {
                        diag_errors
                            .push(format!("error[EASM002]: duplicate data label '{}'", name));
                    }
                    data.data_label_offsets
                        .insert(name.clone(), data_bytes.len());
                }
                for f in floats {
                    data_bytes.extend_from_slice(&f.to_bits().to_le_bytes());
                }
            }
            DataDirective::Ddbl(label, doubles) => {
                if let Some(name) = label {
                    if data.data_label_offsets.contains_key(name) {
                        diag_errors
                            .push(format!("error[EASM002]: duplicate data label '{}'", name));
                    }
                    data.data_label_offsets
                        .insert(name.clone(), data_bytes.len());
                }
                for f in doubles {
                    data_bytes.extend_from_slice(&f.to_bits().to_le_bytes());
                }
            }
            DataDirective::Res(label, bytes) => {
                if let Some(name) = label {
                    if data.data_label_offsets.contains_key(name) {
                        diag_errors
                            .push(format!("error[EASM002]: duplicate data label '{}'", name));
                    }
                    data.data_label_offsets
                        .insert(name.clone(), data_bytes.len());
                }
                data_bytes.resize(data_bytes.len() + *bytes, 0);
            }
            DataDirective::DirectDb {
                address,
                bytes,
                null_terminated,
            } => {
                let needed = *address + bytes.len() + if *null_terminated { 1 } else { 0 };
                if data_bytes.len() < needed {
                    data_bytes.resize(needed, 0);
                }
                for (i, b) in bytes.iter().enumerate() {
                    data_bytes[*address + i] = *b;
                }
                if *null_terminated {
                    data_bytes[*address + bytes.len()] = 0;
                }
            }
        }
    }

    if !diag_errors.is_empty() {
        return Err(diag_errors.join("\n"));
    }

    // Validate exports reference known symbols (EASM003)
    for raw in &data.export_symbols {
        let (kind, name) = if let Some(n) = raw.strip_prefix('#') {
            (0u8, n.to_string())
        } else if let Some(n) = raw.strip_prefix('$') {
            (1u8, n.to_string())
        } else {
            (0u8, raw.clone())
        };
        match kind {
            0 => {
                if !labels.contains_key(&name) {
                    return Err(format!(
                        "error[EASM003]: export of unknown code symbol '{}'",
                        name
                    ));
                }
            }
            1 => {
                if !data.data_label_offsets.contains_key(&name) {
                    return Err(format!(
                        "error[EASM003]: export of unknown data symbol '{}'",
                        name
                    ));
                }
            }
            _ => {}
        }
    }

    // Tables (we will fill import/export below)
    let mut import_table: Vec<u8> = Vec::new();
    // locals: count u16 + entries: nameLen u16 + name + offset u64
    let mut locals: Vec<u8> = Vec::new();
    write_u16_le(data.data_label_offsets.len() as u16, &mut locals);
    for (name, off) in data.data_label_offsets.iter() {
        let nb = name.as_bytes();
        write_u16_le(nb.len() as u16, &mut locals);
        locals.extend_from_slice(nb);
        write_u64_le(*off as u64, &mut locals);
    }
    let const_table: Vec<u8> = Vec::new();
    let mut export_table: Vec<u8> = Vec::new();

    // Label table: count u16 + entries: nameLen u16 + name + addr u64
    let mut label_table: Vec<u8> = Vec::new();
    write_u16_le(labels.len() as u16, &mut label_table);
    for (name, off) in labels.iter() {
        let nb = name.as_bytes();
        write_u16_le(nb.len() as u16, &mut label_table);
        label_table.extend_from_slice(nb);
        write_u64_le(*off as u64, &mut label_table);
    }

    // Code second pass (with relocations for imports)
    let mut code: Vec<u8> = Vec::new();
    data.relocations.clear();

    fn emit_operand_into(
        code: &mut Vec<u8>,
        operand: &str,
        labels: &HashMap<String, usize>,
        data_labels: &HashMap<String, usize>,
        reg_map: &HashMap<String, u16>,
        import_kinds: &HashMap<String, u8>,
        relocs: &mut Vec<Reloc>,
    ) {
        let (mode, val, reloc): (u8, u64, Option<(String, u8)>) = {
            if let Some(name) = operand.strip_prefix('#') {
                if !labels.contains_key(name) {
                    (2, 0, Some((name.to_string(), 0)))
                } else {
                    let (m, v) = encode_operand(operand, labels, data_labels, reg_map);
                    (m, v, None)
                }
            } else if let Some(rest) = operand.strip_prefix('$') {
                let up = rest.to_uppercase();
                if reg_map.contains_key(&up) {
                    let (m, v) = encode_operand(operand, labels, data_labels, reg_map);
                    (m, v, None)
                } else if !data_labels.contains_key(rest) {
                    // If it's a numeric literal (hex or dec), treat as direct memory address, not import
                    if let Some(hex) = rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")) {
                        if let Ok(v) = u64::from_str_radix(hex, 16) {
                            (3, v, None)
                        } else {
                            (3, 0, Some((rest.to_string(), 1)))
                        }
                    } else if rest.chars().all(|c| c.is_ascii_digit()) {
                        if let Ok(v) = rest.parse::<u64>() {
                            (3, v, None)
                        } else {
                            (3, 0, Some((rest.to_string(), 1)))
                        }
                    } else {
                        (3, 0, Some((rest.to_string(), 1)))
                    }
                } else {
                    let (m, v) = encode_operand(operand, labels, data_labels, reg_map);
                    (m, v, None)
                }
            } else {
                if labels.contains_key(operand)
                    || data_labels.contains_key(operand)
                    || reg_map.contains_key(&operand.to_uppercase())
                    || operand.starts_with("0x")
                    || operand.chars().all(|c| c.is_ascii_digit())
                {
                    let (m, v) = encode_operand(operand, labels, data_labels, reg_map);
                    (m, v, None)
                } else if let Some(k) = import_kinds.get(operand).copied() {
                    let mode = if k == 0 { 2 } else { 3 };
                    (mode, 0, Some((operand.to_string(), k)))
                } else {
                    let (m, v) = encode_operand(operand, labels, data_labels, reg_map);
                    (m, v, None)
                }
            }
        };
        code.push(mode);
        let patch_off = code.len();
        write_u64_le(val, code);
        if let Some((name, kind)) = reloc {
            relocs.push(Reloc {
                name,
                kind,
                section: 0,
                offset: patch_off,
            });
        }
    }
    for ins in &insts {
        match ins {
            Instruction::Label(_) => {}
            Instruction::Mov(d, s) => {
                code.push(Op::Mov as u8);
                emit_operand_into(
                    &mut code,
                    d,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                emit_operand_into(
                    &mut code,
                    s,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::FMov(d, s) => {
                code.push(Op::FMov as u8);
                emit_operand_into(
                    &mut code,
                    d,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                emit_operand_into(
                    &mut code,
                    s,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Add(d, s) => {
                code.push(Op::Add as u8);
                emit_operand_into(
                    &mut code,
                    d,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                emit_operand_into(
                    &mut code,
                    s,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::FAdd(d, s) => {
                code.push(Op::FAdd as u8);
                emit_operand_into(
                    &mut code,
                    d,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                emit_operand_into(
                    &mut code,
                    s,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Sub(d, s) => {
                code.push(Op::Sub as u8);
                emit_operand_into(
                    &mut code,
                    d,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                emit_operand_into(
                    &mut code,
                    s,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::FSub(d, s) => {
                code.push(Op::FSub as u8);
                emit_operand_into(
                    &mut code,
                    d,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                emit_operand_into(
                    &mut code,
                    s,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::FMul(d, s) => {
                code.push(Op::FMul as u8);
                emit_operand_into(
                    &mut code,
                    d,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                emit_operand_into(
                    &mut code,
                    s,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::FDiv(d, s) => {
                code.push(Op::FDiv as u8);
                emit_operand_into(
                    &mut code,
                    d,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                emit_operand_into(
                    &mut code,
                    s,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Cmp(a, b) => {
                code.push(Op::Cmp as u8);
                emit_operand_into(
                    &mut code,
                    a,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                emit_operand_into(
                    &mut code,
                    b,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::FCmp(a, b) => {
                code.push(Op::FCmp as u8);
                emit_operand_into(
                    &mut code,
                    a,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                emit_operand_into(
                    &mut code,
                    b,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Jmp(t) => {
                code.push(Op::Jmp as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Je(t) => {
                code.push(Op::Je as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Jne(t) => {
                code.push(Op::Jne as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Jl(t) => {
                code.push(Op::Jl as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Jle(t) => {
                code.push(Op::Jle as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Jg(t) => {
                code.push(Op::Jg as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Jge(t) => {
                code.push(Op::Jge as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::FJe(t) => {
                code.push(Op::FJe as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::FJne(t) => {
                code.push(Op::FJne as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::FJlt(t) => {
                code.push(Op::FJlt as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::FJle(t) => {
                code.push(Op::FJle as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::FJgt(t) => {
                code.push(Op::FJgt as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::FJge(t) => {
                code.push(Op::FJge as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::FJuo(t) => {
                code.push(Op::FJuo as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Call(t) => {
                code.push(Op::Call as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Ret => {
                code.push(Op::Ret as u8);
            }
            Instruction::Push(v) => {
                code.push(Op::Push as u8);
                emit_operand_into(
                    &mut code,
                    v,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Pop(d) => {
                code.push(Op::Pop as u8);
                emit_operand_into(
                    &mut code,
                    d,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Out(p, v) => {
                code.push(Op::Out as u8);
                emit_operand_into(
                    &mut code,
                    p,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                emit_operand_into(
                    &mut code,
                    v,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::COut(p, v) => {
                code.push(Op::COut as u8);
                emit_operand_into(
                    &mut code,
                    p,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                emit_operand_into(
                    &mut code,
                    v,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::In(d) => {
                code.push(Op::In as u8);
                emit_operand_into(
                    &mut code,
                    d,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Hlt => {
                code.push(Op::Hlt as u8);
            }
            Instruction::Nop => {
                code.push(Op::Nop as u8);
            }
            Instruction::Enter(sz) => {
                code.push(Op::Enter as u8);
                emit_operand_into(
                    &mut code,
                    sz,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Leave => {
                code.push(Op::Leave as u8);
            }
            Instruction::Mni {
                module_ptr,
                function_ptr,
                args,
            } => {
                code.push(Op::Mni as u8);
                emit_operand_into(
                    &mut code,
                    module_ptr,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                emit_operand_into(
                    &mut code,
                    function_ptr,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                write_u16_le(args.len() as u16, &mut code);
                for a in args {
                    let tmp = format!("${}", a);
                    emit_operand_into(
                        &mut code,
                        &tmp,
                        &labels,
                        &data.data_label_offsets,
                        &reg_map,
                        &data.import_kinds,
                        &mut data.relocations,
                    );
                }
            }
            Instruction::Syscall => {
                code.push(Op::Syscall as u8);
            }
        }
    }

    // Build export table: format
    //   count u16
    //   entries: kind u8 (0=code,1=data) + nameLen u16 + name bytes + offset u64
    let mut exports: Vec<(u8, String, u64)> = Vec::new();
    for raw in &data.export_symbols {
        let (kind, name) = if let Some(n) = raw.strip_prefix('#') {
            (0u8, n.to_string())
        } else if let Some(n) = raw.strip_prefix('$') {
            (1u8, n.to_string())
        } else {
            (0u8, raw.clone())
        };
        match kind {
            0 => {
                if let Some(off) = labels.get(&name) {
                    exports.push((0, name.clone(), *off as u64));
                }
            }
            1 => {
                if let Some(off) = data.data_label_offsets.get(&name) {
                    exports.push((1, name.clone(), *off as u64));
                }
            }
            _ => {}
        }
    }
    write_u16_le(exports.len() as u16, &mut export_table);
    for (k, n, off) in exports {
        export_table.push(k);
        let nb = n.as_bytes();
        write_u16_le(nb.len() as u16, &mut export_table);
        export_table.extend_from_slice(nb);
        write_u64_le(off, &mut export_table);
    }

    // Build import table with relocations:
    //   count u16
    //   for each imported name: kind u8 + nameLen u16 + name + refCount u16 + refs[(section u8, offset u64)]
    // Group relocations by (name,kind)
    use std::collections::BTreeMap;
    let mut grouped: BTreeMap<(String, u8), Vec<(u8, usize)>> = BTreeMap::new();
    for r in &data.relocations {
        grouped
            .entry((r.name.clone(), r.kind))
            .or_default()
            .push((r.section, r.offset));
    }
    write_u16_le(grouped.len() as u16, &mut import_table);
    for ((name, kind), refs) in grouped {
        import_table.push(kind);
        let nb = name.as_bytes();
        write_u16_le(nb.len() as u16, &mut import_table);
        import_table.extend_from_slice(nb);
        write_u16_le(refs.len() as u16, &mut import_table);
        for (sec, off) in refs {
            import_table.push(sec);
            write_u64_le(off as u64, &mut import_table);
        }
    }

    // Header (16): 'MASI', version u16(1), reserved u16(0), entry u64
    let mut header: Vec<u8> = Vec::new();
    header.extend_from_slice(b"MASI");
    write_u16_le(1, &mut header);
    write_u16_le(0, &mut header);
    write_u64_le(entry.unwrap_or(0) as u64, &mut header);

    // Compose file: [Header]
    // chunks: import, locals, labels, const, data, export, code
    fn chunk(mut dst: &mut Vec<u8>, d: &[u8]) {
        write_u32_le(d.len() as u32, &mut dst);
        dst.extend_from_slice(d);
    }

    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&header);
    chunk(&mut out, &import_table);
    chunk(&mut out, &locals);
    chunk(&mut out, &label_table);
    chunk(&mut out, &const_table);
    chunk(&mut out, &data_bytes);
    chunk(&mut out, &export_table);
    chunk(&mut out, &code);

    Ok(out)
}

// Optional API: allow includes to resolve relative to a specific base directory
pub fn assemble_to_masi_with_base(src: &str, base_dir: &str) -> Result<Vec<u8>, String> {
    let reg_map = RegisterMap::build_name_to_id();
    let base = Path::new(base_dir);
    // Expand includes first to error early on missing files
    let expanded = expand_includes(src, Some(base))?;
    let (insts, mut data) = parse_instructions(&expanded, Some(base));
    let mut diag_errors: Vec<String> = Vec::new();

    // First pass: compute labels and code size to get entry offset
    let mut labels: HashMap<String, usize> = HashMap::new();
    let mut pc: usize = 0;
    let mut entry: Option<usize> = None;
    for ins in &insts {
        match ins {
            Instruction::Label(name) => {
                if labels.contains_key(name) {
                    diag_errors.push(format!("error[EASM002]: duplicate code label '{}'", name));
                }
                labels.insert(name.clone(), pc);
            }
            Instruction::Ret | Instruction::Hlt | Instruction::Nop | Instruction::Leave => {
                if entry.is_none() {
                    entry = Some(pc);
                }
                pc += 1;
            }
            Instruction::Syscall => {
                if entry.is_none() {
                    entry = Some(pc);
                }
                pc += 1;
            }
            Instruction::Enter(_) => {
                if entry.is_none() {
                    entry = Some(pc);
                }
                pc += 1 + (1 + 8);
            }
            Instruction::Jmp(_)
            | Instruction::Je(_)
            | Instruction::Jne(_)
            | Instruction::Jl(_)
            | Instruction::Jle(_)
            | Instruction::Jg(_)
            | Instruction::Jge(_)
            | Instruction::Call(_)
            | Instruction::FJe(_)
            | Instruction::FJne(_)
            | Instruction::FJlt(_)
            | Instruction::FJle(_)
            | Instruction::FJgt(_)
            | Instruction::FJge(_)
            | Instruction::FJuo(_) => {
                if entry.is_none() {
                    entry = Some(pc);
                }
                pc += 1 + 1 + 8;
            }
            Instruction::Push(_) | Instruction::Pop(_) | Instruction::In(_) => {
                if entry.is_none() {
                    entry = Some(pc);
                }
                pc += 1 + 1 + 8;
            }
            Instruction::Out(_, _)
            | Instruction::COut(_, _)
            | Instruction::Mov(_, _)
            | Instruction::Add(_, _)
            | Instruction::Sub(_, _)
            | Instruction::Cmp(_, _)
            | Instruction::FMov(_, _)
            | Instruction::FAdd(_, _)
            | Instruction::FSub(_, _)
            | Instruction::FMul(_, _)
            | Instruction::FDiv(_, _)
            | Instruction::FCmp(_, _) => {
                if entry.is_none() {
                    entry = Some(pc);
                }
                pc += 1 + (1 + 8) + (1 + 8);
            }
            Instruction::Mni { args, .. } => {
                if entry.is_none() {
                    entry = Some(pc);
                }
                pc += 1 + (1 + 8) + (1 + 8) + 2 + (args.len() * (1 + 8));
            }
        }
    }

    // Build data section from directives
    let mut data_bytes: Vec<u8> = Vec::new();
    for d in &data.directives {
        match d {
            DataDirective::Db(label, bytes) => {
                if let Some(name) = label {
                    if data.data_label_offsets.contains_key(name) {
                        diag_errors
                            .push(format!("error[EASM002]: duplicate data label '{}'", name));
                    }
                    data.data_label_offsets
                        .insert(name.clone(), data_bytes.len());
                }
                data_bytes.extend_from_slice(bytes);
            }
            DataDirective::Dw(label, words) => {
                if let Some(name) = label {
                    if data.data_label_offsets.contains_key(name) {
                        diag_errors
                            .push(format!("error[EASM002]: duplicate data label '{}'", name));
                    }
                    data.data_label_offsets
                        .insert(name.clone(), data_bytes.len());
                }
                for w in words {
                    data_bytes.extend_from_slice(&w.to_le_bytes());
                }
            }
            DataDirective::Dd(label, dws) => {
                if let Some(name) = label {
                    if data.data_label_offsets.contains_key(name) {
                        diag_errors
                            .push(format!("error[EASM002]: duplicate data label '{}'", name));
                    }
                    data.data_label_offsets
                        .insert(name.clone(), data_bytes.len());
                }
                for w in dws {
                    data_bytes.extend_from_slice(&w.to_le_bytes());
                }
            }
            DataDirective::Dq(label, qws) => {
                if let Some(name) = label {
                    if data.data_label_offsets.contains_key(name) {
                        diag_errors
                            .push(format!("error[EASM002]: duplicate data label '{}'", name));
                    }
                    data.data_label_offsets
                        .insert(name.clone(), data_bytes.len());
                }
                for dv in qws.iter() {
                    match dv {
                        &DataValue::Imm(v) => {
                            data_bytes.extend_from_slice(&v.to_le_bytes());
                        }
                        &DataValue::Sym(ref s) => {
                            if let Some(off) = data.data_label_offsets.get(s) {
                                data_bytes.extend_from_slice(&(*off as u64).to_le_bytes());
                            } else {
                                data_bytes.extend_from_slice(&0u64.to_le_bytes());
                            }
                        }
                    }
                }
            }
            DataDirective::Df(label, floats) => {
                if let Some(name) = label {
                    if data.data_label_offsets.contains_key(name) {
                        diag_errors
                            .push(format!("error[EASM002]: duplicate data label '{}'", name));
                    }
                    data.data_label_offsets
                        .insert(name.clone(), data_bytes.len());
                }
                for f in floats {
                    data_bytes.extend_from_slice(&f.to_bits().to_le_bytes());
                }
            }
            DataDirective::Ddbl(label, doubles) => {
                if let Some(name) = label {
                    if data.data_label_offsets.contains_key(name) {
                        diag_errors
                            .push(format!("error[EASM002]: duplicate data label '{}'", name));
                    }
                    data.data_label_offsets
                        .insert(name.clone(), data_bytes.len());
                }
                for f in doubles {
                    data_bytes.extend_from_slice(&f.to_bits().to_le_bytes());
                }
            }
            DataDirective::Res(label, bytes) => {
                if let Some(name) = label {
                    if data.data_label_offsets.contains_key(name) {
                        diag_errors
                            .push(format!("error[EASM002]: duplicate data label '{}'", name));
                    }
                    data.data_label_offsets
                        .insert(name.clone(), data_bytes.len());
                }
                data_bytes.resize(data_bytes.len() + *bytes, 0);
            }
            DataDirective::DirectDb {
                address,
                bytes,
                null_terminated,
            } => {
                let needed = *address + bytes.len() + if *null_terminated { 1 } else { 0 };
                if data_bytes.len() < needed {
                    data_bytes.resize(needed, 0);
                }
                for (i, b) in bytes.iter().enumerate() {
                    data_bytes[*address + i] = *b;
                }
                if *null_terminated {
                    data_bytes[*address + bytes.len()] = 0;
                }
            }
        }
    }

    if !diag_errors.is_empty() {
        return Err(diag_errors.join("\n"));
    }

    // Validate exports reference known symbols (EASM003)
    for raw in &data.export_symbols {
        let (kind, name) = if let Some(n) = raw.strip_prefix('#') {
            (0u8, n.to_string())
        } else if let Some(n) = raw.strip_prefix('$') {
            (1u8, n.to_string())
        } else {
            (0u8, raw.clone())
        };
        match kind {
            0 => {
                if !labels.contains_key(&name) {
                    return Err(format!(
                        "error[EASM003]: export of unknown code symbol '{}'",
                        name
                    ));
                }
            }
            1 => {
                if !data.data_label_offsets.contains_key(&name) {
                    return Err(format!(
                        "error[EASM003]: export of unknown data symbol '{}'",
                        name
                    ));
                }
            }
            _ => {}
        }
    }

    // Tables
    let mut import_table: Vec<u8> = Vec::new();
    // locals: count u16 + entries: nameLen u16 + name + offset u64
    let mut locals: Vec<u8> = Vec::new();
    write_u16_le(data.data_label_offsets.len() as u16, &mut locals);
    for (name, off) in data.data_label_offsets.iter() {
        let nb = name.as_bytes();
        write_u16_le(nb.len() as u16, &mut locals);
        locals.extend_from_slice(nb);
        write_u64_le(*off as u64, &mut locals);
    }
    let const_table: Vec<u8> = Vec::new();
    let mut export_table: Vec<u8> = Vec::new();

    // Label table: count u16 + entries: nameLen u16 + name + addr u64
    let mut label_table: Vec<u8> = Vec::new();
    write_u16_le(labels.len() as u16, &mut label_table);
    for (name, off) in labels.iter() {
        let nb = name.as_bytes();
        write_u16_le(nb.len() as u16, &mut label_table);
        label_table.extend_from_slice(nb);
        write_u64_le(*off as u64, &mut label_table);
    }

    // Code second pass (with relocations)
    let mut code: Vec<u8> = Vec::new();
    data.relocations.clear();
    for ins in &insts {
        match ins {
            Instruction::Label(_) => {}
            Instruction::Mov(d, s) => {
                code.push(Op::Mov as u8);
                emit_operand_into(
                    &mut code,
                    d,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                emit_operand_into(
                    &mut code,
                    s,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::FMov(d, s) => {
                code.push(Op::FMov as u8);
                emit_operand_into(
                    &mut code,
                    d,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                emit_operand_into(
                    &mut code,
                    s,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Add(d, s) => {
                code.push(Op::Add as u8);
                emit_operand_into(
                    &mut code,
                    d,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                emit_operand_into(
                    &mut code,
                    s,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::FAdd(d, s) => {
                code.push(Op::FAdd as u8);
                emit_operand_into(
                    &mut code,
                    d,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                emit_operand_into(
                    &mut code,
                    s,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Sub(d, s) => {
                code.push(Op::Sub as u8);
                emit_operand_into(
                    &mut code,
                    d,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                emit_operand_into(
                    &mut code,
                    s,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::FSub(d, s) => {
                code.push(Op::FSub as u8);
                emit_operand_into(
                    &mut code,
                    d,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                emit_operand_into(
                    &mut code,
                    s,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::FMul(d, s) => {
                code.push(Op::FMul as u8);
                emit_operand_into(
                    &mut code,
                    d,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                emit_operand_into(
                    &mut code,
                    s,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::FDiv(d, s) => {
                code.push(Op::FDiv as u8);
                emit_operand_into(
                    &mut code,
                    d,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                emit_operand_into(
                    &mut code,
                    s,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Cmp(a, b) => {
                code.push(Op::Cmp as u8);
                emit_operand_into(
                    &mut code,
                    a,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                emit_operand_into(
                    &mut code,
                    b,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::FCmp(a, b) => {
                code.push(Op::FCmp as u8);
                emit_operand_into(
                    &mut code,
                    a,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                emit_operand_into(
                    &mut code,
                    b,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Jmp(t) => {
                code.push(Op::Jmp as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Je(t) => {
                code.push(Op::Je as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Jne(t) => {
                code.push(Op::Jne as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Jl(t) => {
                code.push(Op::Jl as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Jle(t) => {
                code.push(Op::Jle as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Jg(t) => {
                code.push(Op::Jg as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Jge(t) => {
                code.push(Op::Jge as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::FJe(t) => {
                code.push(Op::FJe as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::FJne(t) => {
                code.push(Op::FJne as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::FJlt(t) => {
                code.push(Op::FJlt as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::FJle(t) => {
                code.push(Op::FJle as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::FJgt(t) => {
                code.push(Op::FJgt as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::FJge(t) => {
                code.push(Op::FJge as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::FJuo(t) => {
                code.push(Op::FJuo as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Call(t) => {
                code.push(Op::Call as u8);
                emit_operand_into(
                    &mut code,
                    t,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Ret => {
                code.push(Op::Ret as u8);
            }
            Instruction::Push(v) => {
                code.push(Op::Push as u8);
                emit_operand_into(
                    &mut code,
                    v,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Pop(d) => {
                code.push(Op::Pop as u8);
                emit_operand_into(
                    &mut code,
                    d,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Out(p, v) => {
                code.push(Op::Out as u8);
                emit_operand_into(
                    &mut code,
                    p,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                emit_operand_into(
                    &mut code,
                    v,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::COut(p, v) => {
                code.push(Op::COut as u8);
                emit_operand_into(
                    &mut code,
                    p,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                emit_operand_into(
                    &mut code,
                    v,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::In(d) => {
                code.push(Op::In as u8);
                emit_operand_into(
                    &mut code,
                    d,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Hlt => {
                code.push(Op::Hlt as u8);
            }
            Instruction::Nop => {
                code.push(Op::Nop as u8);
            }
            Instruction::Enter(sz) => {
                code.push(Op::Enter as u8);
                emit_operand_into(
                    &mut code,
                    sz,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
            }
            Instruction::Leave => {
                code.push(Op::Leave as u8);
            }
            Instruction::Mni {
                module_ptr,
                function_ptr,
                args,
            } => {
                code.push(Op::Mni as u8);
                emit_operand_into(
                    &mut code,
                    module_ptr,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                emit_operand_into(
                    &mut code,
                    function_ptr,
                    &labels,
                    &data.data_label_offsets,
                    &reg_map,
                    &data.import_kinds,
                    &mut data.relocations,
                );
                write_u16_le(args.len() as u16, &mut code);
                for a in args {
                    let tmp = format!("${}", a);
                    emit_operand_into(
                        &mut code,
                        &tmp,
                        &labels,
                        &data.data_label_offsets,
                        &reg_map,
                        &data.import_kinds,
                        &mut data.relocations,
                    );
                }
            }
            Instruction::Syscall => {
                code.push(Op::Syscall as u8);
            }
        }
    }

    // Header (16): 'MASI', version u16(1), reserved u16(0), entry u64
    let mut header: Vec<u8> = Vec::new();
    header.extend_from_slice(b"MASI");
    write_u16_le(1, &mut header);
    write_u16_le(0, &mut header);
    write_u64_le(entry.unwrap_or(0) as u64, &mut header);

    // Compose file: [Header]
    // chunks: import, locals, labels, const, data, export, code
    fn chunk(mut dst: &mut Vec<u8>, d: &[u8]) {
        write_u32_le(d.len() as u32, &mut dst);
        dst.extend_from_slice(d);
    }

    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&header);
    // Build export/import tables same as above
    // Exports
    let mut exports: Vec<(u8, String, u64)> = Vec::new();
    for raw in &data.export_symbols {
        let (kind, name) = if let Some(n) = raw.strip_prefix('#') {
            (0u8, n.to_string())
        } else if let Some(n) = raw.strip_prefix('$') {
            (1u8, n.to_string())
        } else {
            (0u8, raw.clone())
        };
        match kind {
            0 => {
                if let Some(off) = labels.get(&name) {
                    exports.push((0, name.clone(), *off as u64));
                }
            }
            1 => {
                if let Some(off) = data.data_label_offsets.get(&name) {
                    exports.push((1, name.clone(), *off as u64));
                }
            }
            _ => {}
        }
    }
    write_u16_le(exports.len() as u16, &mut export_table);
    for (k, n, off) in exports {
        export_table.push(k);
        let nb = n.as_bytes();
        write_u16_le(nb.len() as u16, &mut export_table);
        export_table.extend_from_slice(nb);
        write_u64_le(off, &mut export_table);
    }
    // Imports (relocations)
    use std::collections::BTreeMap as _Btm;
    let mut grouped: _Btm<(String, u8), Vec<(u8, usize)>> = _Btm::new();
    for r in &data.relocations {
        grouped
            .entry((r.name.clone(), r.kind))
            .or_default()
            .push((r.section, r.offset));
    }
    write_u16_le(grouped.len() as u16, &mut import_table);
    for ((name, kind), refs) in grouped {
        import_table.push(kind);
        let nb = name.as_bytes();
        write_u16_le(nb.len() as u16, &mut import_table);
        import_table.extend_from_slice(nb);
        write_u16_le(refs.len() as u16, &mut import_table);
        for (sec, off) in refs {
            import_table.push(sec);
            write_u64_le(off as u64, &mut import_table);
        }
    }

    chunk(&mut out, &import_table);
    chunk(&mut out, &locals);
    chunk(&mut out, &label_table);
    chunk(&mut out, &const_table);
    chunk(&mut out, &data_bytes);
    chunk(&mut out, &export_table);
    chunk(&mut out, &code);

    Ok(out)
}

mod uuid_like {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    pub fn short_hash(s: &str) -> String {
        let mut h = DefaultHasher::new();
        s.hash(&mut h);
        format!("{:08x}", h.finish())
    }
}
