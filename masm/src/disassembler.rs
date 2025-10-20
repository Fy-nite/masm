use crate::register_map::RegisterMap;
use std::collections::HashMap;

fn read_u16_le(data: &[u8], off: &mut usize) -> u16 {
    let mut v = 0u16;
    v |= data[*off + 0] as u16;
    v |= (data[*off + 1] as u16) << 8;
    *off += 2;
    v
}
fn read_u32_le(data: &[u8], off: &mut usize) -> u32 {
    let mut v = 0u32;
    for i in 0..4 {
        v |= (data[*off + i] as u32) << (8 * i);
    }
    *off += 4;
    v
}
fn read_u64_le(data: &[u8], off: &mut usize) -> u64 {
    let mut v = 0u64;
    for i in 0..8 {
        v |= (data[*off + i] as u64) << (8 * i);
    }
    *off += 8;
    v
}

#[derive(Clone)]
pub struct MASIFile {
    pub version: u16,
    pub entry: u64,
    pub label_map: HashMap<u64, String>, // code offset -> name
    pub code: Vec<u8>,
    pub section_sizes: HashMap<String, usize>,
    pub data_label_map: HashMap<u64, String>, // data offset -> name
    pub data: Vec<u8>,
    pub exports: Vec<ExportSym>,
    pub imports: Vec<ImportSym>,
}

#[derive(Clone)]
pub struct ExportSym {
    pub kind: u8,
    pub name: String,
    pub offset: u64,
}
#[derive(Clone)]
#[allow(dead_code)]
pub struct ImportRef {
    pub section: u8,
    pub offset: u64,
}
#[derive(Clone)]
pub struct ImportSym {
    pub kind: u8,
    pub name: String,
    pub refs: Vec<ImportRef>,
}

pub fn load(path: &str) -> Result<MASIFile, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    parse_masi_bytes(&bytes)
}

/// Parse MASI bytes from a buffer and return the MASIFile structure.
pub fn parse_masi_bytes(bytes: &[u8]) -> Result<MASIFile, String> {
    if bytes.len() < 16 {
        return Err("File too small".into());
    }
    if &bytes[0..4] != b"MASI" {
        return Err("Bad magic".into());
    }
    let mut off = 4usize;
    let version = read_u16_le(&bytes, &mut off);
    let _reserved = read_u16_le(&bytes, &mut off);
    let entry = read_u64_le(&bytes, &mut off);
    let read_chunk = |off: &mut usize| -> Vec<u8> {
        let sz = read_u32_le(&bytes, off) as usize;
        let start = *off;
        let end = start + sz;
        *off = end;
        bytes[start..end].to_vec()
    };
    let import_table = read_chunk(&mut off);
    let local_var_table = read_chunk(&mut off);
    let label_table = read_chunk(&mut off);
    let const_table = read_chunk(&mut off);
    let data_table = read_chunk(&mut off);
    let export_table = read_chunk(&mut off);
    let code = read_chunk(&mut off);

    // labels: count u16, entries nameLen u16 + name + addr u64
    let mut lt_off = 0usize;
    let mut code_offset_to_name: HashMap<u64, String> = HashMap::new();
    if label_table.len() >= 2 {
        let count = read_u16_le(&label_table, &mut lt_off) as usize;
        for _ in 0..count {
            if lt_off + 2 > label_table.len() {
                break;
            }
            let name_len = read_u16_le(&label_table, &mut lt_off) as usize;
            if lt_off + name_len + 8 > label_table.len() {
                break;
            }
            let name = String::from_utf8(label_table[lt_off..lt_off + name_len].to_vec())
                .unwrap_or_else(|_| "label_?".into());
            lt_off += name_len;
            let addr = read_u64_le(&label_table, &mut lt_off);
            code_offset_to_name.insert(addr, name);
        }
    }

    // locals as data labels
    let mut dl_off = 0usize;
    let mut data_offset_to_name: HashMap<u64, String> = HashMap::new();
    if local_var_table.len() >= 2 {
        let count = read_u16_le(&local_var_table, &mut dl_off) as usize;
        for _ in 0..count {
            if dl_off + 2 > local_var_table.len() {
                break;
            }
            let name_len = read_u16_le(&local_var_table, &mut dl_off) as usize;
            if dl_off + name_len + 8 > local_var_table.len() {
                break;
            }
            let name = String::from_utf8(local_var_table[dl_off..dl_off + name_len].to_vec())
                .unwrap_or_else(|_| "data_?".into());
            dl_off += name_len;
            let offs = read_u64_le(&local_var_table, &mut dl_off);
            data_offset_to_name.insert(offs, name);
        }
    }

    // Parse export_table: count u16, entries (kind u8, nameLen u16, name, offset u64)
    let mut ex_off = 0usize;
    let mut exports: Vec<ExportSym> = Vec::new();
    if export_table.len() >= 2 {
        let count = read_u16_le(&export_table, &mut ex_off) as usize;
        for _ in 0..count {
            if ex_off + 3 > export_table.len() {
                break;
            }
            let kind = export_table[ex_off];
            ex_off += 1;
            let name_len = read_u16_le(&export_table, &mut ex_off) as usize;
            if ex_off + name_len + 8 > export_table.len() {
                break;
            }
            let name = String::from_utf8(export_table[ex_off..ex_off + name_len].to_vec())
                .unwrap_or_default();
            ex_off += name_len;
            let offv = read_u64_le(&export_table, &mut ex_off);
            exports.push(ExportSym {
                kind,
                name,
                offset: offv,
            });
        }
    }

    // Parse import_table: count u16, entries (kind u8, nameLen u16, name, refCount u16, refs[section u8, off u64])
    let mut im_off = 0usize;
    let mut imports: Vec<ImportSym> = Vec::new();
    if import_table.len() >= 2 {
        let count = read_u16_le(&import_table, &mut im_off) as usize;
        for _ in 0..count {
            if im_off + 3 > import_table.len() {
                break;
            }
            let kind = import_table[im_off];
            im_off += 1;
            let name_len = read_u16_le(&import_table, &mut im_off) as usize;
            if im_off + name_len + 2 > import_table.len() {
                break;
            }
            let name = String::from_utf8(import_table[im_off..im_off + name_len].to_vec())
                .unwrap_or_default();
            im_off += name_len;
            let rcount = read_u16_le(&import_table, &mut im_off) as usize;
            let mut refs: Vec<ImportRef> = Vec::new();
            for _ in 0..rcount {
                if im_off + 1 + 8 > import_table.len() {
                    break;
                }
                let section = import_table[im_off];
                im_off += 1;
                let offv = read_u64_le(&import_table, &mut im_off);
                refs.push(ImportRef {
                    section,
                    offset: offv,
                });
            }
            imports.push(ImportSym { kind, name, refs });
        }
    }

    let mut sizes = HashMap::new();
    sizes.insert("import".into(), import_table.len());
    sizes.insert("locals".into(), local_var_table.len());
    sizes.insert("labels".into(), label_table.len());
    sizes.insert("const".into(), const_table.len());
    sizes.insert("data".into(), data_table.len());
    sizes.insert("export".into(), export_table.len());
    sizes.insert("code".into(), code.len());

    Ok(MASIFile {
        version,
        entry,
        label_map: code_offset_to_name,
        code,
        section_sizes: sizes,
        data_label_map: data_offset_to_name,
        data: data_table,
        exports,
        imports,
    })
}

fn fmt_op(mode: u8, value: u64, masi: &MASIFile, reg_map_rev: &HashMap<u16, String>) -> String {
    match mode {
        0 => {
            if let Some(name) = masi.data_label_map.get(&value) {
                return format!("${}", name);
            }
            format!("{}", value)
        }
        1 => {
            let id = value as u16;
            if let Some(n) = reg_map_rev.get(&id) {
                n.clone()
            } else {
                format!("REG{}", id)
            }
        }
        2 => {
            if let Some(name) = masi.label_map.get(&value) {
                format!("#{}", name)
            } else {
                format!("#0x{:X}", value)
            }
        }
        3 => {
            if let Some(name) = masi.data_label_map.get(&value) {
                format!("${}", name)
            } else {
                format!("$0x{:X}", value)
            }
        }
        4 => {
            let id = value as u16;
            if let Some(n) = reg_map_rev.get(&id) {
                format!("${}", n)
            } else {
                format!("$REG{}", id)
            }
        }
        _ => format!("{}", value),
    }
}

pub fn disassemble(masi: &MASIFile) -> String {
    let reg_rev = RegisterMap::build_id_to_name();
    let mut out: Vec<String> = Vec::new();
    let mut pc = 0usize;
    let code = &masi.code;
    while pc < code.len() {
        if let Some(name) = masi.label_map.get(&(pc as u64)) {
            out.push(format!("LBL {}", name));
        }
        let opcode = code[pc];
        pc += 1;
        let read_op = |pc: &mut usize| -> (u8, u64) {
            let mode = code[*pc];
            *pc += 1;
            let mut off = *pc;
            let val = read_u64_le(code, &mut off);
            *pc = off;
            (mode, val)
        };
        match opcode {
            0x01 => {
                let d = read_op(&mut pc);
                let s = read_op(&mut pc);
                out.push(format!(
                    "MOV {} {}",
                    fmt_op(d.0, d.1, masi, &reg_rev),
                    fmt_op(s.0, s.1, masi, &reg_rev)
                ));
            }
            0x02 => {
                let d = read_op(&mut pc);
                let s = read_op(&mut pc);
                out.push(format!(
                    "ADD {} {}",
                    fmt_op(d.0, d.1, masi, &reg_rev),
                    fmt_op(s.0, s.1, masi, &reg_rev)
                ));
            }
            0x03 => {
                let d = read_op(&mut pc);
                let s = read_op(&mut pc);
                out.push(format!(
                    "SUB {} {}",
                    fmt_op(d.0, d.1, masi, &reg_rev),
                    fmt_op(s.0, s.1, masi, &reg_rev)
                ));
            }
            0x70 => {
                let d = read_op(&mut pc);
                let s = read_op(&mut pc);
                out.push(format!(
                    "FMOV {} {}",
                    fmt_op(d.0, d.1, masi, &reg_rev),
                    fmt_op(s.0, s.1, masi, &reg_rev)
                ));
            }
            0x71 => {
                let d = read_op(&mut pc);
                let s = read_op(&mut pc);
                out.push(format!(
                    "FADD {} {}",
                    fmt_op(d.0, d.1, masi, &reg_rev),
                    fmt_op(s.0, s.1, masi, &reg_rev)
                ));
            }
            0x72 => {
                let d = read_op(&mut pc);
                let s = read_op(&mut pc);
                out.push(format!(
                    "FSUB {} {}",
                    fmt_op(d.0, d.1, masi, &reg_rev),
                    fmt_op(s.0, s.1, masi, &reg_rev)
                ));
            }
            0x73 => {
                let d = read_op(&mut pc);
                let s = read_op(&mut pc);
                out.push(format!(
                    "FMUL {} {}",
                    fmt_op(d.0, d.1, masi, &reg_rev),
                    fmt_op(s.0, s.1, masi, &reg_rev)
                ));
            }
            0x74 => {
                let d = read_op(&mut pc);
                let s = read_op(&mut pc);
                out.push(format!(
                    "FDIV {} {}",
                    fmt_op(d.0, d.1, masi, &reg_rev),
                    fmt_op(s.0, s.1, masi, &reg_rev)
                ));
            }
            0x75 => {
                let a = read_op(&mut pc);
                let b = read_op(&mut pc);
                out.push(format!(
                    "FCMP {} {}",
                    fmt_op(a.0, a.1, masi, &reg_rev),
                    fmt_op(b.0, b.1, masi, &reg_rev)
                ));
            }
            0x76 => {
                let t = read_op(&mut pc);
                out.push(format!("FJE {}", fmt_op(t.0, t.1, masi, &reg_rev)));
            }
            0x77 => {
                let t = read_op(&mut pc);
                out.push(format!("FJNE {}", fmt_op(t.0, t.1, masi, &reg_rev)));
            }
            0x78 => {
                let t = read_op(&mut pc);
                out.push(format!("FJLT {}", fmt_op(t.0, t.1, masi, &reg_rev)));
            }
            0x79 => {
                let t = read_op(&mut pc);
                out.push(format!("FJLE {}", fmt_op(t.0, t.1, masi, &reg_rev)));
            }
            0x7A => {
                let t = read_op(&mut pc);
                out.push(format!("FJGT {}", fmt_op(t.0, t.1, masi, &reg_rev)));
            }
            0x7B => {
                let t = read_op(&mut pc);
                out.push(format!("FJGE {}", fmt_op(t.0, t.1, masi, &reg_rev)));
            }
            0x7C => {
                let t = read_op(&mut pc);
                out.push(format!("FJUO {}", fmt_op(t.0, t.1, masi, &reg_rev)));
            }
            0x10 => {
                let t = read_op(&mut pc);
                out.push(format!("JMP {}", fmt_op(t.0, t.1, masi, &reg_rev)));
            }
            0x11 => {
                let a = read_op(&mut pc);
                let b = read_op(&mut pc);
                out.push(format!(
                    "CMP {} {}",
                    fmt_op(a.0, a.1, masi, &reg_rev),
                    fmt_op(b.0, b.1, masi, &reg_rev)
                ));
            }
            0x12 => {
                let t = read_op(&mut pc);
                out.push(format!("JE {}", fmt_op(t.0, t.1, masi, &reg_rev)));
            }
            0x13 => {
                let t = read_op(&mut pc);
                out.push(format!("JNE {}", fmt_op(t.0, t.1, masi, &reg_rev)));
            }
            0x14 => {
                let t = read_op(&mut pc);
                out.push(format!("JL {}", fmt_op(t.0, t.1, masi, &reg_rev)));
            }
            0x15 => {
                let t = read_op(&mut pc);
                out.push(format!("JLE {}", fmt_op(t.0, t.1, masi, &reg_rev)));
            }
            0x16 => {
                let t = read_op(&mut pc);
                out.push(format!("JG {}", fmt_op(t.0, t.1, masi, &reg_rev)));
            }
            0x17 => {
                let t = read_op(&mut pc);
                out.push(format!("JGE {}", fmt_op(t.0, t.1, masi, &reg_rev)));
            }
            0x20 => {
                let t = read_op(&mut pc);
                out.push(format!("CALL {}", fmt_op(t.0, t.1, masi, &reg_rev)));
            }
            0x21 => {
                out.push("RET".into());
            }
            0x30 => {
                let v = read_op(&mut pc);
                out.push(format!("PUSH {}", fmt_op(v.0, v.1, masi, &reg_rev)));
            }
            0x31 => {
                let d = read_op(&mut pc);
                out.push(format!("POP {}", fmt_op(d.0, d.1, masi, &reg_rev)));
            }
            0x40 => {
                let p = read_op(&mut pc);
                let v = read_op(&mut pc);
                out.push(format!(
                    "OUT {} {}",
                    fmt_op(p.0, p.1, masi, &reg_rev),
                    fmt_op(v.0, v.1, masi, &reg_rev)
                ));
            }
            0x41 => {
                let p = read_op(&mut pc);
                let v = read_op(&mut pc);
                out.push(format!(
                    "COUT {} {}",
                    fmt_op(p.0, p.1, masi, &reg_rev),
                    fmt_op(v.0, v.1, masi, &reg_rev)
                ));
            }
            0x42 => {
                let d = read_op(&mut pc);
                out.push(format!("IN {}", fmt_op(d.0, d.1, masi, &reg_rev)));
            }
            0x50 => {
                let s = read_op(&mut pc);
                out.push(format!("ENTER {}", fmt_op(s.0, s.1, masi, &reg_rev)));
            }
            0x51 => {
                out.push("LEAVE".into());
            }
            0x60 => {
                let m = read_op(&mut pc);
                let f = read_op(&mut pc);
                let argc = read_u16_le(code, &mut pc) as usize;
                let mut line = format!(
                    "MNI {} {}",
                    fmt_op(m.0, m.1, masi, &reg_rev),
                    fmt_op(f.0, f.1, masi, &reg_rev)
                );
                for _ in 0..argc {
                    let a = read_op(&mut pc);
                    line.push(' ');
                    line.push_str(&fmt_op(a.0, a.1, masi, &reg_rev));
                }
                out.push(line);
            }
            0x00 => {
                out.push("NOP".into());
            }
            0xFF => {
                out.push("HLT".into());
            }
            _ => {
                out.push(format!("; DB 0x{:02X} ; Unknown opcode", opcode));
            }
        }
    }
    out.join("\n")
}

pub fn dump(masi: &MASIFile) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("MASI Dump".into());
    lines.push(format!("- Version: {}", masi.version));
    lines.push(format!("- Entry: 0x{:016X}", masi.entry));
    lines.push("- Sections:".into());
    for key in [
        "import", "locals", "labels", "const", "data", "export", "code",
    ] {
        if let Some(sz) = masi.section_sizes.get(key) {
            lines.push(format!("  - {}: {} bytes", key, sz));
        }
    }
    if !masi.label_map.is_empty() {
        let mut kv: Vec<_> = masi.label_map.iter().collect();
        kv.sort_by_key(|(k, _)| *k);
        lines.push("- Labels:".into());
        for (off, name) in kv {
            lines.push(format!("  - 0x{:X}: {}", off, name));
        }
    }
    if !masi.exports.is_empty() {
        lines.push("- Exports:".into());
        for e in &masi.exports {
            let k = if e.kind == 0 { "code" } else { "data" };
            lines.push(format!("  - {} {} @ 0x{:X}", k, e.name, e.offset));
        }
    }
    if !masi.imports.is_empty() {
        lines.push("- Imports:".into());
        for im in &masi.imports {
            let k = if im.kind == 0 { "code" } else { "data" };
            lines.push(format!("  - {} {} ({} refs)", k, im.name, im.refs.len()));
        }
    }
    if masi.section_sizes.get("data").copied().unwrap_or(0) > 0 {
        let mut kv: Vec<_> = masi.data_label_map.iter().collect();
        kv.sort_by_key(|(k, _)| *k);
        lines.push("- Data labels:".into());
        for (off, name) in kv {
            lines.push(format!("  - 0x{:X}: {}", off, name));
        }
        lines.push(format!(
            "- Data section size: {} bytes",
            masi.section_sizes.get("data").unwrap()
        ));
    }
    lines.join("\n")
}
