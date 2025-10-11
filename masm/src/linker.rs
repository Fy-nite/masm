use crate::disassembler::{self, MASIFile};

fn write_u16_le(v: u16, out: &mut Vec<u8>) { out.extend_from_slice(&v.to_le_bytes()); }
fn write_u32_le(v: u32, out: &mut Vec<u8>) { out.extend_from_slice(&v.to_le_bytes()); }
fn write_u64_le(v: u64, out: &mut Vec<u8>) { out.extend_from_slice(&v.to_le_bytes()); }

pub struct LinkedOutput { pub bytes: Vec<u8> }

pub fn link_files(paths: &[String]) -> Result<LinkedOutput, String> {
    if paths.is_empty() { return Err("no input files".into()); }
    let mut files: Vec<MASIFile> = Vec::new();
    for p in paths { files.push(disassembler::load(p)?); }

    // Build export map: (kind,name) -> (file_index, offset)
    use std::collections::HashMap;
    let mut export_map: HashMap<(u8,String), (usize,u64)> = HashMap::new();
    for (i,f) in files.iter().enumerate() {
        for ex in &f.exports { export_map.insert((ex.kind, ex.name.clone()), (i, ex.offset)); }
    }

    // Concatenate code; track start offsets per file
    let mut code: Vec<u8> = Vec::new();
    let mut code_base: Vec<usize> = Vec::new();
    for f in &files { code_base.push(code.len()); code.extend_from_slice(&f.code); }

    // Concatenate data similarly
    let mut data: Vec<u8> = Vec::new();
    let mut data_base: Vec<usize> = Vec::new();
    for f in &files { data_base.push(data.len()); data.extend_from_slice(&f.data); }

    // Resolve imports: for each file's import refs, look up target in export_map and patch code/data
    for (i, f) in files.iter().enumerate() {
        let cbase = code_base[i];
        for im in &f.imports {
            if let Some(&(ei, off)) = export_map.get(&(im.kind, im.name.clone())) {
                let target_off = match im.kind { 0 => code_base[ei] as u64 + off, 1 => data_base[ei] as u64 + off, _ => off };
                for r in &im.refs {
                    // section 0 = code (only one we emit relocations for now)
                    if r.section == 0 {
                        let patch = (cbase as u64 + r.offset) as usize;
                        if patch + 8 <= code.len() { let bytes = target_off.to_le_bytes(); code[patch..patch+8].copy_from_slice(&bytes); }
                    }
                }
            } else {
                return Err(format!("unresolved import: {} {}", if im.kind==0 {"code"} else {"data"}, im.name));
            }
        }
    }

    // Build label table by merging each file's label_map with offset
    let mut label_table: Vec<u8> = Vec::new();
    let mut label_entries: Vec<(String,u64)> = Vec::new();
    for (i,f) in files.iter().enumerate() { let base = code_base[i] as u64; for (off, name) in &f.label_map { label_entries.push((name.clone(), base + *off)); } }
    write_u16_le(label_entries.len() as u16, &mut label_table);
    for (name, off) in label_entries { let nb = name.as_bytes(); write_u16_le(nb.len() as u16, &mut label_table); label_table.extend_from_slice(nb); write_u64_le(off, &mut label_table); }

    // Local data labels combined
    let mut locals: Vec<u8> = Vec::new();
    let mut local_entries: Vec<(String,u64)> = Vec::new();
    for (i,f) in files.iter().enumerate() { let base = data_base[i] as u64; for (off, name) in &f.data_label_map { local_entries.push((name.clone(), base + *off)); } }
    write_u16_le(local_entries.len() as u16, &mut locals);
    for (name, off) in local_entries { let nb = name.as_bytes(); write_u16_le(nb.len() as u16, &mut locals); locals.extend_from_slice(nb); write_u64_le(off, &mut locals); }

    // Exports: re-base each file's exports
    let mut export_table: Vec<u8> = Vec::new();
    let mut ex_entries: Vec<(u8,String,u64)> = Vec::new();
    for (i,f) in files.iter().enumerate() { for e in &f.exports { let base = if e.kind==0 { code_base[i] } else { data_base[i] } as u64; ex_entries.push((e.kind, e.name.clone(), base + e.offset)); } }
    write_u16_le(ex_entries.len() as u16, &mut export_table);
    for (k,n,off) in ex_entries { export_table.push(k); let nb = n.as_bytes(); write_u16_le(nb.len() as u16, &mut export_table); export_table.extend_from_slice(nb); write_u64_le(off, &mut export_table); }

    // Header
    let mut header: Vec<u8> = Vec::new(); header.extend_from_slice(b"MASI"); write_u16_le(1, &mut header); write_u16_le(0, &mut header);
    let entry = if files.is_empty() { 0 } else { code_base[0] as u64 + files[0].entry };
    write_u64_le(entry, &mut header);
    // Compose chunks: import (empty after link), locals, labels, const (empty), data, export, code
    let mut out: Vec<u8> = Vec::new(); out.extend_from_slice(&header);
    let empty: Vec<u8> = Vec::new();
    let mut chunk = |d: &[u8]| { write_u32_le(d.len() as u32, &mut out); out.extend_from_slice(d); };
    chunk(&empty); // imports removed
    chunk(&locals);
    chunk(&label_table);
    chunk(&empty); // const
    chunk(&data);
    chunk(&export_table);
    chunk(&code);

    Ok(LinkedOutput { bytes: out })
}
