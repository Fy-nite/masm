use crate::register_map::RegisterMap;
use std::collections::HashMap;

// Minimal MASI opcodes, mirrored from Swift
#[allow(dead_code)]
#[repr(u8)]
enum Op {
    Mov = 0x01,
    Add = 0x02,
    Sub = 0x03,
    Jmp = 0x10,
    Cmp = 0x11,
    Je  = 0x12,
    Jne = 0x13,
    Call = 0x20,
    Ret  = 0x21,
    Push = 0x30,
    Pop  = 0x31,
    Out  = 0x40,
    COut = 0x41,
    In   = 0x42,
    Enter= 0x50,
    Leave= 0x51,
    Mni  = 0x60,
    Hlt  = 0xFF,
    Nop  = 0x00,
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
    Call(String),
    Ret,
    Push(String),
    Pop(String),
    Out(String, String),
    COut(String, String),
    In(String),
    Enter(String),
    Leave,
    Mni{ module_ptr: String, function_ptr: String, args: Vec<String>},
    Hlt,
    Nop,
}

#[derive(Default, Debug, Clone)]
struct DataSections {
    directives: Vec<DataDirective>,
    data_label_offsets: HashMap<String, usize>,
    mni_string_labels: HashMap<String, String>, // content -> gen label
}

#[derive(Debug, Clone)]
enum DataDirective {
    Db(Option<String>, Vec<u8>),
    Dw(Option<String>, Vec<u16>),
    Dd(Option<String>, Vec<u32>),
    Dq(Option<String>, Vec<u64>),
    Df(Option<String>, Vec<f32>),
    Ddbl(Option<String>, Vec<f64>),
    Res(Option<String>, usize),
    DirectDb{ address: usize, bytes: Vec<u8>, null_terminated: bool },
}

fn is_register(s: &str, reg_map: &HashMap<String, u16>) -> bool {
    reg_map.contains_key(&s.to_uppercase())
}

fn parse_string_literal(tok: &str) -> Option<Vec<u8>> {
    let bytes = tok.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'"' || *bytes.last().unwrap() != b'"' { return None; }
    let inner = &tok[1..tok.len()-1];
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
            } else { break; }
        } else {
            out.push(c as u32 as u8);
        }
    }
    Some(out)
}

fn parse_data_line(line: &str, data: &mut DataSections) -> bool {
    let mut label_name: Option<String> = None;
    let mut rest = line.trim().to_string();
    if let Some(colon_idx) = rest.find(':') {
        label_name = Some(rest[..colon_idx].trim().to_string());
        rest = rest[colon_idx+1..].trim().to_string();
    }
    let lower = rest.to_lowercase();
    let parse_values = |part: &str| -> Vec<String> { part.split(',').map(|s| s.trim().to_string()).collect() };

    if lower.starts_with("db ") {
        let rhs = rest[3..].trim().to_string();
        if rhs.starts_with('$') {
            let mut parts = rhs.splitn(2, ' ');
            let a = parts.next().unwrap();
            let b = parts.next();
            if let Some(b) = b {
                let addr_str = &a[1..];
                let addr = if addr_str.to_lowercase().starts_with("0x") { u64::from_str_radix(&addr_str[2..], 16).unwrap_or(0) as usize } else { addr_str.parse::<usize>().unwrap_or(0) };
                if let Some(bytes) = parse_string_literal(b) {
                    data.directives.push(DataDirective::DirectDb{ address: addr, bytes, null_terminated: true });
                    return true;
                }
            }
        }
        let mut all: Vec<u8> = Vec::new();
        for tok in parse_values(&rhs) {
            if let Some(sb) = parse_string_literal(&tok) { all.extend_from_slice(&sb); }
            else if tok.to_lowercase().starts_with("0x") { if let Ok(v) = u8::from_str_radix(&tok[2..], 16) { all.push(v); } }
            else if let Ok(v) = tok.parse::<i64>() { all.push(v as u8); }
        }
        data.directives.push(DataDirective::Db(label_name, all));
        return true;
    } else if lower.starts_with("dw ") {
        let rhs = rest[3..].to_string();
        let mut vals: Vec<u16> = Vec::new();
        for tok in parse_values(&rhs) {
            if tok.to_lowercase().starts_with("0x") { if let Ok(v) = u16::from_str_radix(&tok[2..], 16) { vals.push(v); } }
            else if let Ok(v) = tok.parse::<i64>() { vals.push(v as u16); }
        }
        data.directives.push(DataDirective::Dw(label_name, vals));
        return true;
    } else if lower.starts_with("dd ") {
        let rhs = rest[3..].to_string();
        let mut vals: Vec<u32> = Vec::new();
        for tok in parse_values(&rhs) {
            if tok.to_lowercase().starts_with("0x") { if let Ok(v) = u32::from_str_radix(&tok[2..], 16) { vals.push(v); } }
            else if let Ok(v) = tok.parse::<i64>() { vals.push(v as u32); }
        }
        data.directives.push(DataDirective::Dd(label_name, vals));
        return true;
    } else if lower.starts_with("dq ") {
        let rhs = rest[3..].to_string();
        let mut vals: Vec<u64> = Vec::new();
        for tok in parse_values(&rhs) {
            if tok.starts_with('#') { /* not supported */ }
            if tok.to_lowercase().starts_with("0x") { if let Ok(v) = u64::from_str_radix(&tok[2..], 16) { vals.push(v); } }
            else if let Ok(v) = tok.parse::<u64>() { vals.push(v); }
        }
        data.directives.push(DataDirective::Dq(label_name, vals));
        return true;
    } else if lower.starts_with("df ") {
        let rhs = rest[3..].to_string();
        let mut vals: Vec<f32> = Vec::new();
        for tok in parse_values(&rhs) { if let Ok(v) = tok.parse::<f32>() { vals.push(v); } }
        data.directives.push(DataDirective::Df(label_name, vals));
        return true;
    } else if lower.starts_with("ddbl ") {
        let rhs = rest[5..].to_string();
        let mut vals: Vec<f64> = Vec::new();
        for tok in parse_values(&rhs) { if let Ok(v) = tok.parse::<f64>() { vals.push(v); } }
        data.directives.push(DataDirective::Ddbl(label_name, vals));
        return true;
    } else if lower.starts_with("resb ") || lower.starts_with("resw ") || lower.starts_with("resd ") || lower.starts_with("resq ") || lower.starts_with("resf ") || lower.starts_with("resdbl ") {
        let mut parts = rest.splitn(2, ' ');
        let kind = parts.next().unwrap().to_lowercase();
        let count: usize = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let factor = match kind.as_str() { "resb"=>1, "resw"=>2, "resd"=>4, "resq"=>8, "resf"=>4, "resdbl"=>8, _=>1 };
        data.directives.push(DataDirective::Res(label_name, count * factor));
        return true;
    }
    false
}

fn parse_instructions(src: &str) -> (Vec<Instruction>, DataSections) {
    let mut insts: Vec<Instruction> = Vec::new();
    let mut data = DataSections::default();

    for raw in src.lines() {
        let mut line = raw.to_string();
        if let Some(idx) = line.find(';') { line.truncate(idx); }
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }
        if parse_data_line(trimmed, &mut data) { continue; }
        let mut parts = trimmed.splitn(2, ' ');
        let mnemonic = parts.next().unwrap().to_lowercase();
        let operands: Vec<String> = parts.next().map(|s| s.split(' ').map(|t| t.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default();
        match (mnemonic.as_str(), operands.len()) {
            ("lbl", 1) | ("label", 1) => insts.push(Instruction::Label(operands[0].clone())),
            ("mov", 2) => insts.push(Instruction::Mov(operands[0].clone(), operands[1].clone())),
            ("add", 2) => insts.push(Instruction::Add(operands[0].clone(), operands[1].clone())),
            ("sub", 2) => insts.push(Instruction::Sub(operands[0].clone(), operands[1].clone())),
            ("jmp", 1) => insts.push(Instruction::Jmp(operands[0].clone())),
            ("cmp", 2) => insts.push(Instruction::Cmp(operands[0].clone(), operands[1].clone())),
            ("je", 1)  => insts.push(Instruction::Je(operands[0].clone())),
            ("jne", 1) => insts.push(Instruction::Jne(operands[0].clone())),
            ("call", 1)=> insts.push(Instruction::Call(operands[0].clone())),
            ("ret", 0) => insts.push(Instruction::Ret),
            ("push", 1)=> insts.push(Instruction::Push(operands[0].clone())),
            ("pop", 1) => insts.push(Instruction::Pop(operands[0].clone())),
            ("out", 2) => insts.push(Instruction::Out(operands[0].clone(), operands[1].clone())),
            ("cout", 2)=> insts.push(Instruction::COut(operands[0].clone(), operands[1].clone())),
            ("in", 1)  => insts.push(Instruction::In(operands[0].clone())),
            ("hlt", 0) => insts.push(Instruction::Hlt),
            ("nop", 0) => insts.push(Instruction::Nop),
            ("enter",1)=> insts.push(Instruction::Enter(operands[0].clone())),
            ("leave",0)=> insts.push(Instruction::Leave),
            ("mni", n) if n >= 1 => {
                if operands.len() >= 2 && operands[0].starts_with('$') && operands[1].starts_with('$') {
                    let extra = if operands.len() > 2 { operands[2..].to_vec() } else { vec![] };
                    insts.push(Instruction::Mni{ module_ptr: operands[0].clone(), function_ptr: operands[1].clone(), args: extra});
                } else {
                    let name = &operands[0];
                    let mut parts = name.splitn(2, '.');
                    let module = parts.next().unwrap_or(name);
                    let function = parts.next().unwrap_or("main");
                    let raw_args = if operands.len() > 1 { operands[1..].to_vec() } else { vec![] };
                    fn safe_label(s: &str) -> String {
                        let mut out = String::from("__mni_str_");
                        out.push_str(&format!("{}_", uuid_like::short_hash(s)));
                        out.push_str(&s.chars().map(|c| if c.is_ascii_alphanumeric() || c=='_' { c } else { '_' }).collect::<String>());
                        out
                    }
                    let mod_lbl = if let Some(lbl) = data.mni_string_labels.get(module) { lbl.clone() } else {
                        let lbl = safe_label(module);
                        let mut bytes = module.as_bytes().to_vec(); bytes.push(0);
                        data.directives.push(DataDirective::Db(Some(lbl.clone()), bytes));
                        data.mni_string_labels.insert(module.to_string(), lbl.clone());
                        lbl
                    };
                    let fn_lbl = if let Some(lbl) = data.mni_string_labels.get(function) { lbl.clone() } else {
                        let lbl = safe_label(function);
                        let mut bytes = function.as_bytes().to_vec(); bytes.push(0);
                        data.directives.push(DataDirective::Db(Some(lbl.clone()), bytes));
                        data.mni_string_labels.insert(function.to_string(), lbl.clone());
                        lbl
                    };
                    let mut arg_labels: Vec<String> = Vec::new();
                    for a in raw_args.iter() {
                        let lbl = if let Some(l) = data.mni_string_labels.get(a) { l.clone() } else {
                            let l = safe_label(a);
                            let mut bytes = a.as_bytes().to_vec(); bytes.push(0);
                            data.directives.push(DataDirective::Db(Some(l.clone()), bytes));
                            data.mni_string_labels.insert(a.clone(), l.clone());
                            l
                        };
                        arg_labels.push(lbl);
                    }
                    insts.push(Instruction::Mni{ module_ptr: format!("${}", mod_lbl), function_ptr: format!("${}", fn_lbl), args: arg_labels});
                }
            }
            _ => { eprintln!("Error: Unrecognized instruction or wrong number of operands: {}", trimmed); }
        }
    }
    (insts, data)
}

fn write_u16_le(v: u16, out: &mut Vec<u8>) { out.extend_from_slice(&v.to_le_bytes()); }
fn write_u32_le(v: u32, out: &mut Vec<u8>) { out.extend_from_slice(&v.to_le_bytes()); }
fn write_u64_le(v: u64, out: &mut Vec<u8>) { out.extend_from_slice(&v.to_le_bytes()); }

fn encode_operand(s: &str, labels: &HashMap<String, usize>, data_labels: &HashMap<String, usize>, reg_map: &HashMap<String, u16>) -> (u8, u64) {
    if let Some(name) = s.strip_prefix('#') {
        if let Some(off) = labels.get(name) { return (2, *off as u64); }
        return (2, 0);
    }
    if let Some(rest) = s.strip_prefix('$') {
        let up = rest.to_uppercase();
        if let Some(&id) = reg_map.get(&up) { return (4, id as u64); }
        if let Some(&off) = data_labels.get(rest) { return (3, off as u64); }
        if let Some(hex) = rest.strip_prefix("0x") { if let Ok(v) = u64::from_str_radix(hex, 16) { return (3, v); } }
        if let Ok(v) = rest.parse::<u64>() { return (3, v); }
        return (3, 0);
    }
    if let Some(&id) = reg_map.get(&s.to_uppercase()) { return (1, id as u64); }
    if let Some(&off) = data_labels.get(s) { return (0, off as u64); }
    if let Some(hex) = s.strip_prefix("0x") { if let Ok(v) = u64::from_str_radix(hex, 16) { return (0, v); } }
    if let Ok(v) = s.parse::<u64>() { return (0, v); }
    (0, 0)
}

pub fn assemble_to_masi(src: &str) -> Result<Vec<u8>, String> {
    let reg_map = RegisterMap::build_name_to_id();
    let (insts, mut data) = parse_instructions(src);

    // First pass: compute labels and code size to get entry offset
    let mut labels: HashMap<String, usize> = HashMap::new();
    let mut pc: usize = 0;
    let mut entry: Option<usize> = None;
    for ins in &insts {
        match ins {
            Instruction::Label(name) => { labels.insert(name.clone(), pc); },
            Instruction::Ret | Instruction::Hlt | Instruction::Nop | Instruction::Leave => { if entry.is_none() { entry = Some(pc); } pc += 1; },
            Instruction::Enter(_) => { if entry.is_none() { entry = Some(pc); } pc += 1 + (1+8); },
            Instruction::Jmp(_) | Instruction::Je(_) | Instruction::Jne(_) | Instruction::Call(_) => { if entry.is_none() { entry = Some(pc); } pc += 1 + 1 + 8; },
            Instruction::Push(_) | Instruction::Pop(_) | Instruction::In(_) => { if entry.is_none() { entry = Some(pc); } pc += 1 + 1 + 8; },
            Instruction::Out(_, _) | Instruction::COut(_, _) | Instruction::Mov(_, _) | Instruction::Add(_, _) | Instruction::Sub(_, _) | Instruction::Cmp(_, _) => { if entry.is_none() { entry = Some(pc); } pc += 1 + (1+8) + (1+8); },
            Instruction::Mni{ args, .. } => { if entry.is_none() { entry = Some(pc); } pc += 1 + (1+8) + (1+8) + 2 + (args.len() * (1+8)); },
        }
    }

    // Build data section from directives
    let mut data_bytes: Vec<u8> = Vec::new();
    for d in &data.directives {
        match d {
            DataDirective::Db(label, bytes) => {
                if let Some(name) = label { data.data_label_offsets.insert(name.clone(), data_bytes.len()); }
                data_bytes.extend_from_slice(bytes);
            }
            DataDirective::Dw(label, words) => {
                if let Some(name) = label { data.data_label_offsets.insert(name.clone(), data_bytes.len()); }
                for w in words { data_bytes.extend_from_slice(&w.to_le_bytes()); }
            }
            DataDirective::Dd(label, dws) => {
                if let Some(name) = label { data.data_label_offsets.insert(name.clone(), data_bytes.len()); }
                for w in dws { data_bytes.extend_from_slice(&w.to_le_bytes()); }
            }
            DataDirective::Dq(label, qws) => {
                if let Some(name) = label { data.data_label_offsets.insert(name.clone(), data_bytes.len()); }
                for w in qws { data_bytes.extend_from_slice(&w.to_le_bytes()); }
            }
            DataDirective::Df(label, floats) => {
                if let Some(name) = label { data.data_label_offsets.insert(name.clone(), data_bytes.len()); }
                for f in floats { data_bytes.extend_from_slice(&f.to_bits().to_le_bytes()); }
            }
            DataDirective::Ddbl(label, doubles) => {
                if let Some(name) = label { data.data_label_offsets.insert(name.clone(), data_bytes.len()); }
                for f in doubles { data_bytes.extend_from_slice(&f.to_bits().to_le_bytes()); }
            }
            DataDirective::Res(label, bytes) => {
                if let Some(name) = label { data.data_label_offsets.insert(name.clone(), data_bytes.len()); }
                data_bytes.resize(data_bytes.len() + *bytes, 0);
            }
            DataDirective::DirectDb{ address, bytes, null_terminated } => {
                let needed = *address + bytes.len() + if *null_terminated { 1 } else { 0 };
                if data_bytes.len() < needed { data_bytes.resize(needed, 0); }
                for (i, b) in bytes.iter().enumerate() { data_bytes[*address + i] = *b; }
                if *null_terminated { data_bytes[*address + bytes.len()] = 0; }
            }
        }
    }

    // Tables
    let import_table: Vec<u8> = Vec::new();
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
    let export_table: Vec<u8> = Vec::new();

    // Label table: count u16 + entries: nameLen u16 + name + addr u64
    let mut label_table: Vec<u8> = Vec::new();
    write_u16_le(labels.len() as u16, &mut label_table);
    for (name, off) in labels.iter() {
        let nb = name.as_bytes();
        write_u16_le(nb.len() as u16, &mut label_table);
        label_table.extend_from_slice(nb);
        write_u64_le(*off as u64, &mut label_table);
    }

    // Code second pass
    let mut code: Vec<u8> = Vec::new();
    for ins in &insts {
        match ins {
            Instruction::Label(_) => {}
            Instruction::Mov(d, s) => {
                code.push(Op::Mov as u8);
                let (m, v) = encode_operand(d, &labels, &data.data_label_offsets, &reg_map); code.push(m); write_u64_le(v, &mut code);
                let (m, v) = encode_operand(s, &labels, &data.data_label_offsets, &reg_map); code.push(m); write_u64_le(v, &mut code);
            }
            Instruction::Add(d, s) => {
                code.push(Op::Add as u8);
                let (m, v) = encode_operand(d, &labels, &data.data_label_offsets, &reg_map); code.push(m); write_u64_le(v, &mut code);
                let (m, v) = encode_operand(s, &labels, &data.data_label_offsets, &reg_map); code.push(m); write_u64_le(v, &mut code);
            }
            Instruction::Sub(d, s) => {
                code.push(Op::Sub as u8);
                let (m, v) = encode_operand(d, &labels, &data.data_label_offsets, &reg_map); code.push(m); write_u64_le(v, &mut code);
                let (m, v) = encode_operand(s, &labels, &data.data_label_offsets, &reg_map); code.push(m); write_u64_le(v, &mut code);
            }
            Instruction::Cmp(a, b) => {
                code.push(Op::Cmp as u8);
                let (m, v) = encode_operand(a, &labels, &data.data_label_offsets, &reg_map); code.push(m); write_u64_le(v, &mut code);
                let (m, v) = encode_operand(b, &labels, &data.data_label_offsets, &reg_map); code.push(m); write_u64_le(v, &mut code);
            }
            Instruction::Jmp(t) => { code.push(Op::Jmp as u8); let (m, v) = encode_operand(t, &labels, &data.data_label_offsets, &reg_map); code.push(m); write_u64_le(v, &mut code); }
            Instruction::Je(t)  => { code.push(Op::Je  as u8); let (m, v) = encode_operand(t, &labels, &data.data_label_offsets, &reg_map); code.push(m); write_u64_le(v, &mut code); }
            Instruction::Jne(t) => { code.push(Op::Jne as u8); let (m, v) = encode_operand(t, &labels, &data.data_label_offsets, &reg_map); code.push(m); write_u64_le(v, &mut code); }
            Instruction::Call(t)=> { code.push(Op::Call as u8); let (m, v) = encode_operand(t, &labels, &data.data_label_offsets, &reg_map); code.push(m); write_u64_le(v, &mut code); }
            Instruction::Ret   => { code.push(Op::Ret as u8); }
            Instruction::Push(v)=> { code.push(Op::Push as u8); let (m, v) = encode_operand(v, &labels, &data.data_label_offsets, &reg_map); code.push(m); write_u64_le(v, &mut code); }
            Instruction::Pop(d) => { code.push(Op::Pop  as u8); let (m, v) = encode_operand(d, &labels, &data.data_label_offsets, &reg_map); code.push(m); write_u64_le(v, &mut code); }
            Instruction::Out(p, v)=> {
                code.push(Op::Out as u8);
                let (m, vv) = encode_operand(p, &labels, &data.data_label_offsets, &reg_map); code.push(m); write_u64_le(vv, &mut code);
                let (m, vv) = encode_operand(v, &labels, &data.data_label_offsets, &reg_map); code.push(m); write_u64_le(vv, &mut code);
            }
            Instruction::COut(p, v)=> {
                code.push(Op::COut as u8);
                let (m, vv) = encode_operand(p, &labels, &data.data_label_offsets, &reg_map); code.push(m); write_u64_le(vv, &mut code);
                let (m, vv) = encode_operand(v, &labels, &data.data_label_offsets, &reg_map); code.push(m); write_u64_le(vv, &mut code);
            }
            Instruction::In(d) => {
                code.push(Op::In as u8);
                let (m, v) = encode_operand(d, &labels, &data.data_label_offsets, &reg_map); code.push(m); write_u64_le(v, &mut code);
            }
            Instruction::Hlt => { code.push(Op::Hlt as u8); }
            Instruction::Nop => { code.push(Op::Nop as u8); }
            Instruction::Enter(sz)=> { code.push(Op::Enter as u8); let (m, v) = encode_operand(sz, &labels, &data.data_label_offsets, &reg_map); code.push(m); write_u64_le(v, &mut code); }
            Instruction::Leave   => { code.push(Op::Leave as u8); }
            Instruction::Mni{ module_ptr, function_ptr, args } => {
                code.push(Op::Mni as u8);
                let (m, v) = encode_operand(module_ptr, &labels, &data.data_label_offsets, &reg_map); code.push(m); write_u64_le(v, &mut code);
                let (m, v) = encode_operand(function_ptr, &labels, &data.data_label_offsets, &reg_map); code.push(m); write_u64_le(v, &mut code);
                write_u16_le(args.len() as u16, &mut code);
                for a in args { let (m, v) = encode_operand(&format!("${}", a), &labels, &data.data_label_offsets, &reg_map); code.push(m); write_u64_le(v, &mut code); }
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
    use std::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;
    pub fn short_hash(s: &str) -> String {
        let mut h = DefaultHasher::new();
        s.hash(&mut h);
        format!("{:08x}", h.finish())
    }
}
