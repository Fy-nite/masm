use crate::disassembler::{self, MASIFile};
use crate::register_map::RegisterMap;
use std::collections::HashMap;
use std::io::{self, Write};
use std::fs;
use std::path::Path;
use mlua::{Lua, Value as LuaValue, Table as LuaTable, Function as LuaFunction};

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

#[derive(Default, Clone)]
pub struct State {
    pub regs: HashMap<u16, u64>,
    pub flags: (bool, bool, bool, bool), // ZF, SF, CF, OF
    pub rip: u64,
    pub stack: Vec<u64>,
    pub memory: Vec<u8>,
}

pub struct MniCtx {
    pub state: State,
    pub args: Vec<String>,
}

type MniFunc = Box<dyn Fn(&mut MniCtx) + 'static>;

pub struct ModuleRegistry {
    funcs: HashMap<String, HashMap<String, MniFunc>>,
}

impl ModuleRegistry {
    pub fn new() -> Self { Self { funcs: HashMap::new() } }
    pub fn register<F: Fn(&mut MniCtx) + 'static>(&mut self, module: &str, name: &str, f: F) {
        self.funcs.entry(module.to_string()).or_default().insert(name.to_string(), Box::new(f));
    }
    pub fn lookup(&self, module: &str, name: &str) -> Option<&MniFunc> { self.funcs.get(module)?.get(name) }
}

fn read_u16_le(data: &[u8], off: &mut usize) -> u16 { let mut v=0u16; v |= data[*off] as u16; v |= (data[*off+1] as u16) << 8; *off+=2; v }
fn read_u64_le(data: &[u8], off: &mut usize) -> u64 { let mut v=0u64; for i in 0..8 { v |= (data[*off+i] as u64) << (8*i); } *off+=8; v }

fn read_u64_from_memory(addr: u64, state: &State) -> u64 {
    let a = addr as usize;
    if a + 8 > state.memory.len() { return 0; }
    let mut v = 0u64; for i in 0..8 { v |= (state.memory[a+i] as u64) << (8*i); } v
}
fn write_u64_to_memory(addr: u64, value: u64, state: &mut State) {
    let a = addr as usize; if a > usize::MAX - 8 { return; }
    if state.memory.len() < a + 8 { state.memory.resize(a + 8, 0); }
    let mut v = value; for i in 0..8 { state.memory[a+i] = (v & 0xFF) as u8; v >>= 8; }
}

fn get_operand(code: &[u8], pc: &mut usize, state: &mut State) -> u64 {
    let mode = code[*pc]; *pc += 1; let val = read_u64_le(code, pc);
    match mode {
        0 => val,
        1 => { let id = val as u16; *state.regs.get(&id).unwrap_or(&0) },
        2 => val,
        3 => read_u64_from_memory(val, state),
        4 => { let id = val as u16; let addr = *state.regs.get(&id).unwrap_or(&0); read_u64_from_memory(addr, state) },
        _ => val,
    }
}

fn set_operand(code: &[u8], pc: &mut usize, state: &mut State, value: u64) {
    let mode = code[*pc]; *pc += 1; let val = read_u64_le(code, pc);
    match mode {
        1 => { let id = val as u16; state.regs.insert(id, value); }
        3 => { write_u64_to_memory(val, value, state); }
        4 => { let id = val as u16; let addr = *state.regs.get(&id).unwrap_or(&0); write_u64_to_memory(addr, value, state); }
        _ => {}
    }
}

fn update_add_flags(a: u64, b: u64, r: u64, state: &mut State) { // ZF,SF,CF,OF
    let zf = r == 0; let sf = (r as i64) < 0; let cf = r < a;
    let sa = a as i64; let sb = b as i64; let sr = r as i64;
    let of = (sa > 0 && sb > 0 && sr < 0) || (sa < 0 && sb < 0 && sr > 0);
    state.flags = (zf, sf, cf, of);
}
fn update_sub_flags(a: u64, b: u64, r: u64, state: &mut State) {
    let zf = r == 0; let sf = (r as i64) < 0; let cf = a < b;
    let sa = a as i64; let sb = b as i64; let sr = r as i64;
    let of = (sa >= 0 && sb < 0 && sr < 0) || (sa < 0 && sb >= 0 && sr >= 0);
    state.flags = (zf, sf, cf, of);
}

fn read_c_string(addr: u64, mem: &[u8]) -> Option<String> {
    let start = addr as usize; if start >= mem.len() { return None; }
    let mut i = start; let mut bytes: Vec<u8> = Vec::new();
    while i < mem.len() { let b = mem[i]; i += 1; if b == 0 { break; } bytes.push(b); }
    String::from_utf8(bytes).ok()
}

fn load_lua_modules(registry: &mut ModuleRegistry) -> Result<(), String> {
    let modules_dir = Path::new("modules");
    if !modules_dir.is_dir() { return Ok(()); }
    let lua = Lua::new();
    for entry in fs::read_dir(modules_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("lua")).unwrap_or(false) {
            let src = fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let chunk = lua.load(&src);
            let val = chunk.eval::<LuaValue>().map_err(|e| e.to_string())?;
            // Two contracts supported:
            // 1) return a table { MNI_MODULE = "name", MNI_FUNCTIONS = { fname = function(args, regs) -> nil | string | { out=string, regs={name=int}, store={...} } } }
            // 2) global variables MNI_MODULE and MNI_FUNCTIONS in the script environment
            let (module_name, funcs_table): (Option<String>, Option<LuaTable>) = match val {
                LuaValue::Table(t) => {
                    let m: Option<String> = t.get("MNI_MODULE").map_err(|e| e.to_string())?;
                    let f: Option<LuaTable> = t.get("MNI_FUNCTIONS").map_err(|e| e.to_string())?;
                    (m, f)
                }
                _ => {
                    let globals = lua.globals();
                    let m: Option<String> = globals.get("MNI_MODULE").map_err(|e| e.to_string())?;
                    let f: Option<LuaTable> = globals.get("MNI_FUNCTIONS").map_err(|e| e.to_string())?;
                    (m, f)
                }
            };
            if let (Some(mod_name), Some(funcs)) = (module_name, funcs_table) {
                for pair in funcs.pairs::<String, LuaFunction>() {
                    let (fname, _func) = pair.map_err(|e| e.to_string())?;
                    // Only register if not already present
                    let already = registry.lookup(&mod_name, &fname).is_some();
                    if already {
                        println!("[Lua MNI] Skipping already-registered {}.{}", mod_name, fname);
                        continue;
                    }
                    println!("[Lua MNI] Registering {}.{}", mod_name, fname);
                    let lua = Lua::new();
                    let chunk2 = lua.load(&src);
                    let _ = chunk2.eval::<LuaValue>();
                    let globals2 = lua.globals();
                    let funcs2: Option<LuaTable> = globals2.get("MNI_FUNCTIONS").ok();
                    let lua_func: Option<LuaFunction> = if let Some(ft) = funcs2 { ft.get(fname.as_str()).ok() } else { None };
                    if let Some(lf) = lua_func {
                        registry.register(&mod_name, &fname, move |ctx: &mut MniCtx| {
                            let args_arr = lua.create_table().unwrap();
                            for (i, a) in ctx.args.iter().enumerate() { let _ = args_arr.set((i+1) as i64, a.as_str()); }
                            let regs_tbl = lua.create_table().unwrap();
                            for (id, val) in ctx.state.regs.iter() {
                                if let Some(name) = RegisterMap::build_id_to_name().get(id) { let _ = regs_tbl.set(name.as_str(), *val as i64); }
                            }
                            let ret: mlua::Result<LuaValue> = lf.call((args_arr, regs_tbl));
                            if let Ok(val) = ret {
                                match val {
                                    LuaValue::Nil => {}
                                    LuaValue::String(s) => { if let Ok(st) = s.to_str() { println!("{}", st); } }
                                    LuaValue::Table(t) => {
                                        if let Ok(Some(out)) = t.get::<Option<String>>("out") { println!("{}", out); }
                                        if let Ok(Some(upd)) = t.get::<Option<LuaTable>>("regs") {
                                            for pair in upd.pairs::<String, i64>() { if let Ok((rname, ival)) = pair { if let Some(id) = RegisterMap::build_name_to_id().get(&rname) { ctx.state.regs.insert(*id, ival as u64); } } }
                                        }
                                        if let Ok(Some(store)) = t.get::<Option<LuaTable>>("store") {
                                            let mut dest: Option<u64> = None;
                                            if let Ok(Some(addr)) = store.get::<Option<u64>>("addr") { dest = Some(addr); }
                                            else if let Ok(Some(rname)) = store.get::<Option<String>>("reg") { if let Some(id) = RegisterMap::build_name_to_id().get(&rname) { dest = Some(*ctx.state.regs.get(id).unwrap_or(&0)); } }
                                            if let Some(base) = dest { let base_usize = base as usize; if let Ok(Some(s)) = store.get::<Option<String>>("string") { let mut bytes = s.into_bytes(); bytes.push(0); if ctx.state.memory.len() < base_usize + bytes.len() { ctx.state.memory.resize(base_usize + bytes.len(), 0); } ctx.state.memory[base_usize..base_usize+bytes.len()].copy_from_slice(&bytes); }
                                            else if let Ok(Some(arr)) = store.get::<Option<LuaTable>>("bytes") { let len = arr.len().unwrap_or(0) as usize; let mut bytes: Vec<u8> = Vec::with_capacity(len); for i in 1..=len { if let Ok(v) = arr.get::<i64>(i as i64) { bytes.push(v as u8); } } if ctx.state.memory.len() < base_usize + bytes.len() { ctx.state.memory.resize(base_usize + bytes.len(), 0); } ctx.state.memory[base_usize..base_usize+bytes.len()].copy_from_slice(&bytes); } }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        });
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn run_path(path: &str) -> Result<(), String> {
    let masi = disassembler::load(path)?;
    run_masi(&masi)
}

pub fn run_masi(masi: &MASIFile) -> Result<(), String> {
    let mut state = State::default();
    state.memory = masi.data.clone();
    let mut registry = ModuleRegistry::new();
    // // debug.echo: prints C-string at RDI
    // let name_to_id = RegisterMap::build_name_to_id();
    // let rdi = *name_to_id.get("RDI").unwrap_or(&0);
    // registry.register("debug", "echo", move |ctx: &mut MniCtx| {
    //     let ptr = *ctx.state.regs.get(&rdi).unwrap_or(&0);
    //     if let Some(s) = read_c_string(ptr, &ctx.state.memory) { println!("{}", s); }
    // });
    // // tool.set_rax <value>: sets RAX to the provided integer string
    // let name_to_id2 = RegisterMap::build_name_to_id();
    // let rax = *name_to_id2.get("RAX").unwrap_or(&0);
    // // registry.register("tool", "set_rax", move |ctx: &mut MniCtx| {
    // //     if let Some(first) = ctx.args.get(0) {
    // //         if let Ok(v) = first.parse::<u64>() { ctx.state.regs.insert(rax, v); }
    // //     }
    // // });

    // Load Lua modules if present
    let _ = load_lua_modules(&mut registry);

    let code = &masi.code;
    let mut pc: usize = masi.entry as usize;
    while pc < code.len() {
        state.rip = pc as u64;
        let byte = code[pc]; pc += 1;
        match byte {
            x if x == Op::Nop as u8 => { continue; }
            x if x == Op::Hlt as u8 => { return Ok(()); }
            x if x == Op::Mov as u8 => {
                // dest then source
                let dest_pos = pc;
                let _mode = code[pc]; pc += 1; let _id_skip = read_u64_le(code, &mut pc);
                let src_val = get_operand(code, &mut pc, &mut state);
                pc = dest_pos; set_operand(code, &mut pc, &mut state, src_val);
            }
            x if x == Op::Add as u8 => {
                let dest_mode = code[pc]; pc += 1; let dest_id64 = read_u64_le(code, &mut pc);
                let src_val = get_operand(code, &mut pc, &mut state);
                if dest_mode == 1 { let id = dest_id64 as u16; let a = *state.regs.get(&id).unwrap_or(&0); let r = a.wrapping_add(src_val); state.regs.insert(id, r); update_add_flags(a, src_val, r, &mut state); }
            }
            x if x == Op::Sub as u8 => {
                let dest_mode = code[pc]; pc += 1; let dest_id64 = read_u64_le(code, &mut pc);
                let src_val = get_operand(code, &mut pc, &mut state);
                if dest_mode == 1 { let id = dest_id64 as u16; let a = *state.regs.get(&id).unwrap_or(&0); let r = a.wrapping_sub(src_val); state.regs.insert(id, r); update_sub_flags(a, src_val, r, &mut state); }
            }
            x if x == Op::Cmp as u8 => {
                let a = get_operand(code, &mut pc, &mut state);
                let b = get_operand(code, &mut pc, &mut state);
                let r = a.wrapping_sub(b); update_sub_flags(a, b, r, &mut state);
            }
            x if x == Op::Jmp as u8 => { let t = get_operand(code, &mut pc, &mut state); pc = t as usize; }
            x if x == Op::Je  as u8 => { let t = get_operand(code, &mut pc, &mut state); if state.flags.0 { pc = t as usize; } }
            x if x == Op::Jne as u8 => { let t = get_operand(code, &mut pc, &mut state); if !state.flags.0 { pc = t as usize; } }
            x if x == Op::Call as u8 => { let t = get_operand(code, &mut pc, &mut state); state.stack.push(pc as u64); pc = t as usize; }
            x if x == Op::Ret as u8 => { if let Some(ret) = state.stack.pop() { pc = ret as usize; } }
            x if x == Op::Push as u8 => { let v = get_operand(code, &mut pc, &mut state); state.stack.push(v); }
            x if x == Op::Pop  as u8 => { let dest_mode = code[pc]; pc += 1; let dest_id64 = read_u64_le(code, &mut pc); if dest_mode == 1 { if let Some(v) = state.stack.pop() { state.regs.insert(dest_id64 as u16, v); } } }
            x if x == Op::Enter as u8 => {
                let size = get_operand(code, &mut pc, &mut state);
                let name_to_id = RegisterMap::build_name_to_id();
                let rbp = *name_to_id.get("RBP").unwrap_or(&0); let rsp = *name_to_id.get("RSP").unwrap_or(&0);
                let cur_rbp = *state.regs.get(&rbp).unwrap_or(&0); state.stack.push(cur_rbp);
                let cur_rsp = *state.regs.get(&rsp).unwrap_or(&(state.stack.len() as u64));
                state.regs.insert(rbp, cur_rsp);
                state.regs.insert(rsp, cur_rsp.wrapping_add(size));
            }
            x if x == Op::Leave as u8 => {
                let name_to_id = RegisterMap::build_name_to_id();
                let rbp = *name_to_id.get("RBP").unwrap_or(&0); let rsp = *name_to_id.get("RSP").unwrap_or(&0);
                let frame_top = *state.regs.get(&rbp).unwrap_or(&0);
                state.regs.insert(rsp, frame_top);
                if let Some(v) = state.stack.pop() { state.regs.insert(rbp, v); }
            }
            x if x == Op::Mni as u8 => {
                // Read module/function as raw (mode,val) pairs (addresses expected when mode=3)
                let m_mode = code[pc]; pc += 1; let m_val = read_u64_le(code, &mut pc);
                let f_mode = code[pc]; pc += 1; let f_val = read_u64_le(code, &mut pc);
                let argc = read_u16_le(code, &mut pc) as usize;
                let mut argv: Vec<String> = Vec::new();
                for _ in 0..argc {
                    let mode = code[pc]; pc += 1; let val = read_u64_le(code, &mut pc);
                    match mode {
                        0 => argv.push(format!("{}", val)),
                        1 => { let id = val as u16; let name = RegisterMap::build_id_to_name().remove(&id).unwrap_or_else(|| format!("REG{}", id)); argv.push(name); }
                        3 => { if let Some(s) = read_c_string(val, &state.memory) { argv.push(s); } else { argv.push(format!("$0x{:X}", val)); } }
                        4 => { let id = val as u16; let name = RegisterMap::build_id_to_name().remove(&id).unwrap_or_else(|| format!("REG{}", id)); argv.push(format!("${}", name)); }
                        _ => argv.push(format!("{}", val)),
                    }
                }
                // Decode module/function names
                let mod_name = match m_mode { 3 => read_c_string(m_val, &state.memory), 1 => Some(RegisterMap::build_id_to_name().remove(&(m_val as u16)).unwrap_or_else(|| format!("REG{}", m_val as u16))), 0 => Some(format!("{}", m_val)), 4 => Some(format!("${}", RegisterMap::build_id_to_name().remove(&(m_val as u16)).unwrap_or_else(|| format!("REG{}", m_val as u16)))), _ => None };
                let fn_name  = match f_mode { 3 => read_c_string(f_val, &state.memory), 1 => Some(RegisterMap::build_id_to_name().remove(&(f_val as u16)).unwrap_or_else(|| format!("REG{}", f_val as u16))), 0 => Some(format!("{}", f_val)), 4 => Some(format!("${}", RegisterMap::build_id_to_name().remove(&(f_val as u16)).unwrap_or_else(|| format!("REG{}", f_val as u16)))), _ => None };
                println!("[DEBUG] MNI lookup: module={:?}, function={:?}", mod_name, fn_name);
                println!("[DEBUG] Registered MNI functions:");
                for (modn, fmap) in &registry.funcs {
                    for fname in fmap.keys() {
                        println!("  {:?}.{:?}", modn, fname);
                    }
                }
                if let (Some(mn), Some(fn_)) = (mod_name, fn_name) {
                    let mn_lc = mn.trim().to_lowercase();
                    let fn_lc = fn_.trim().to_lowercase();
                    if let Some(func) = registry.lookup(&mn_lc, &fn_lc) {
                        let mut ctx = MniCtx { state: state.clone(), args: argv };
                        func(&mut ctx);
                        state = ctx.state;
                        let rax_id = RegisterMap::build_name_to_id().get("RAX").copied().unwrap_or(0);
                        let rax_val = state.regs.get(&rax_id).copied().unwrap_or(0);
                        println!("[DEBUG] RAX after MNI: {}", rax_val);
                    } else {
                        let _ = writeln!(io::stderr(), "MNI: function not found");
                    }
                } else {
                    let _ = writeln!(io::stderr(), "MNI: function not found");
                }
            }
            x if x == Op::Out as u8 => {
                // OUT port value: print string only if value is a memory address (mode 3 or 4), else print numeric value
                let port_mode = code[pc]; pc += 1; let port_val = read_u64_le(code, &mut pc);
                let val_mode = code[pc]; pc += 1; let val_val = read_u64_le(code, &mut pc);
                let to_err = port_mode == 2;
                match val_mode {
                    3 | 4 => {
                        // Print string at address
                        if let Some(s) = read_c_string(val_val, &state.memory) {
                            if to_err { let _ = writeln!(io::stderr(), "{}", s); } else { println!("{}", s); }
                        } else {
                            if to_err { let _ = writeln!(io::stderr(), "{}", val_val); } else { println!("{}", val_val); }
                        }
                    }
                    1 => {
                        // Print value in register
                        let reg_val = state.regs.get(&(val_val as u16)).copied().unwrap_or(0);
                        if to_err { let _ = writeln!(io::stderr(), "{}", reg_val); } else { println!("{}", reg_val); }
                    }
                    _ => {
                        // Print numeric value
                        if to_err { let _ = writeln!(io::stderr(), "{}", val_val); } else { println!("{}", val_val); }
                    }
                }
            }
            x if x == Op::COut as u8 => {
                let p = get_operand(code, &mut pc, &mut state);
                let v = get_operand(code, &mut pc, &mut state);
                let to_err = p == 2; let ch: u8 = if (v as usize) < state.memory.len() { state.memory[v as usize] } else { v as u8 };
                if to_err { let _ = io::stderr().write_all(&[ch]); } else { print!("{}", ch as char); let _ = io::stdout().flush(); }
            }
            x if x == Op::In as u8 => {
                let dest_addr = get_operand(code, &mut pc, &mut state) as usize;
                let mut line = String::new(); let _ = io::stdin().read_line(&mut line);
                let mut bytes = line.into_bytes(); bytes.push(0);
                if dest_addr + bytes.len() > state.memory.len() { state.memory.resize(dest_addr + bytes.len(), 0); }
                state.memory[dest_addr..dest_addr+bytes.len()].copy_from_slice(&bytes);
            }
            _ => {}
        }
    }
    Ok(())
}
