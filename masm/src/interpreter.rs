// Debug printing macro, controlled by DEBUG_PRINT flag
macro_rules! debug_println {
    ($($arg:tt)*) => {
        if cfg!(feature = "debug_print") || DEBUG_PRINT.load(std::sync::atomic::Ordering::Relaxed) {
            println!($($arg)*);
        }
    };
}

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
// removed unused std::sync::mpsc imports
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::thread::{self, JoinHandle};
static DEBUG_PRINT: AtomicBool = AtomicBool::new(false);

pub fn set_debug_print(enabled: bool) {
    DEBUG_PRINT.store(enabled, Ordering::Relaxed);
}
use crate::disassembler::{self, MASIFile};
use crate::register_map::RegisterMap;
use libloading::Library;
use mlua::{Function as LuaFunction, Lua, Table as LuaTable, Value as LuaValue};
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ffi::{CStr, CString};
use std::fs;
use std::io::{self, BufRead, Write};
use std::os::raw::{c_char, c_void};
use std::path::Path;
#[cfg(feature = "raylib_mni")]
#[path = "mni_raylib.rs"]
mod mni_raylib;

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
    // Floating point
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

#[derive(Clone)]
pub struct State {
    pub regs: HashMap<u16, u64>,
    pub flags: (bool, bool, bool, bool), // ZF, SF, CF, OF
    pub rip: u64,
    pub stack: Vec<u64>,
    // Shared memory buffer between threads. Protected by a mutex for safe concurrent access.
    pub memory: Arc<Mutex<Vec<u8>>>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            regs: HashMap::new(),
            flags: (false, false, false, false),
            rip: 0,
            stack: Vec::new(),
            memory: Arc::new(Mutex::new(Vec::new())),
            warnings: Vec::new(),
            errors: Vec::new(),
        }
    }
}

// ---------------- Debugger support (optional) ----------------
// A simple hook the VM calls before each instruction. Implementations can block to run a REPL,
// decide stepping/continuation, and request exit.
pub enum DebuggerControl {
    Continue,
    Exit,
}

pub trait Debugger {
    fn before_execute(
        &mut self,
        masi: &MASIFile,
        state: &State,
        pc: usize,
        opcode: u8,
    ) -> DebuggerControl;
}

thread_local! {
    static TLS_DEBUGGER: RefCell<Option<Box<dyn Debugger>>> = RefCell::new(None);
}

/// Install/uninstall a debugger for the current thread. Call this before run_masi/run_masi_with_io.
pub fn set_thread_debugger(dbg: Option<Box<dyn Debugger>>) {
    TLS_DEBUGGER.with(|cell| {
        *cell.borrow_mut() = dbg;
    });
}

// Minimal TUI-style debugger (key-driven). This is intentionally small: it
// blocks before each instruction unless the user chose "continue". Keys:
//  s = step (advance one instruction)
//  c = continue (run without stopping)
//  q = quit (request VM exit)
pub struct TuiDebugger {
    continue_mode: std::cell::Cell<bool>,
}

impl TuiDebugger {
    pub fn new() -> Self { Self { continue_mode: std::cell::Cell::new(false) } }
}

impl Debugger for TuiDebugger {
    fn before_execute(&mut self, _masi: &MASIFile, state: &State, pc: usize, opcode: u8) -> DebuggerControl {
        // If already in continue mode, do nothing
        if self.continue_mode.get() { return DebuggerControl::Continue; }

        // Print a compact status line and prompt for a single-key command.
        // Use crossterm event read to capture one keypress without requiring Enter.
        println!("[DBG] RIP=0x{:X} PC={} OPCODE=0x{:02X}", state.rip, pc, opcode);
        // show a few registers (RAX,RBX,RSP,RBP if present)
        for name in ["RAX","RBX","RCX","RDX","RSP","RBP"].iter() {
            if let Some(id) = RegisterMap::build_name_to_id().get(*name) {
                let val = *state.regs.get(id).unwrap_or(&0);
                print!("{}=0x{:X} ", name, val);
            }
        }
        println!("");
        print!("[dbg] (s)tep (c)ontinue (q)uit > ");
        let _ = std::io::stdout().flush();

        // Read a single key via crossterm (fall back to line read on error)
        match crossterm::event::read() {
            Ok(ev) => {
                if let crossterm::event::Event::Key(k) = ev {
                    match k.code {
                        crossterm::event::KeyCode::Char('s') => { return DebuggerControl::Continue; }
                        crossterm::event::KeyCode::Char('c') => { self.continue_mode.set(true); return DebuggerControl::Continue; }
                        crossterm::event::KeyCode::Char('q') => { return DebuggerControl::Exit; }
                        _ => { println!("[dbg] unrecognized key"); return DebuggerControl::Continue; }
                    }
                }
            }
            Err(_) => {
                // fallback: read a line
                let mut s = String::new(); let _ = std::io::stdin().read_line(&mut s);
                let s = s.trim(); if s == "c" { self.continue_mode.set(true); return DebuggerControl::Continue; }
                if s == "q" { return DebuggerControl::Exit; }
                return DebuggerControl::Continue;
            }
        }
        DebuggerControl::Continue
    }
}

fn with_debugger<F: FnOnce(&mut dyn Debugger) -> DebuggerControl>(f: F) -> Option<DebuggerControl> {
    TLS_DEBUGGER.with(|cell| {
        if let Some(ref mut d) = *cell.borrow_mut() {
            Some(f(d.as_mut()))
        } else {
            None
        }
    })
}

pub struct MniCtx {
    pub state: State,
    pub args: Vec<String>,
}

type MniFunc = Box<dyn Fn(&mut MniCtx) + 'static>;

pub struct ModuleRegistry {
    funcs: HashMap<String, HashMap<String, MniFunc>>,
    dyn_libs: HashMap<String, Library>,
}

impl ModuleRegistry {
    pub fn new() -> Self {
        Self {
            funcs: HashMap::new(),
            dyn_libs: HashMap::new(),
        }
    }
    pub fn register<F: Fn(&mut MniCtx) + 'static>(&mut self, module: &str, name: &str, f: F) {
        let module_key = module.trim().to_lowercase();
        let name_key = name.trim().to_lowercase();
        self.funcs
            .entry(module_key)
            .or_default()
            .insert(name_key, Box::new(f));
    }
    pub fn lookup(&self, module: &str, name: &str) -> Option<&MniFunc> {
        self.funcs.get(module)?.get(name)
    }

    fn build_candidate_lib_names(module: &str) -> Vec<String> {
        let mut cands: Vec<String> = Vec::new();
        let base = module.to_string();
        if cfg!(windows) {
            cands.push(format!("{}.dll", base));
            cands.push(format!("lib{}.dll", base));
        } else if cfg!(target_os = "macos") {
            cands.push(format!("lib{}.dylib", base));
            cands.push(format!("{}.dylib", base));
        } else {
            cands.push(format!("lib{}.so", base));
            cands.push(format!("{}.so", base));
        }
        cands
    }

    fn build_search_dirs() -> Vec<std::path::PathBuf> {
        let mut dirs: Vec<std::path::PathBuf> = Vec::new();
        if let Ok(cwd) = std::env::current_dir() {
            dirs.push(cwd);
        }
        dirs.push(std::path::PathBuf::from("modules"));
        if let Ok(exe) = std::env::current_exe() {
            if let Some(p) = exe.parent() {
                dirs.push(p.to_path_buf());
            }
        }
        dirs
    }

    fn load_dyn_lib(&mut self, module_name_raw: &str) -> Result<&Library, String> {
        let key = module_name_raw.to_string();
        if self.dyn_libs.contains_key(&key) {
            return Ok(self.dyn_libs.get(&key).unwrap());
        }
        let mut last_err: Option<String> = None;
        let dirs = Self::build_search_dirs();
        let names = Self::build_candidate_lib_names(module_name_raw);
        for d in dirs.iter() {
            for n in names.iter() {
                let path = d.join(n);
                if path.exists() {
                    match unsafe { Library::new(&path) } {
                        Ok(lib) => {
                            self.dyn_libs.insert(key.clone(), lib);
                            return Ok(self.dyn_libs.get(&key).unwrap());
                        }
                        Err(e) => {
                            last_err = Some(format!("failed to load {}: {}", path.display(), e));
                        }
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| format!("dynamic module '{}' not found", module_name_raw)))
    }
}

// Thread registry for spawned VM threads
struct ThreadEntry {
    handle: JoinHandle<()>,
}

struct ThreadRegistry {
    map: Mutex<std::collections::HashMap<u64, ThreadEntry>>,
    next_id: AtomicU64,
}

impl ThreadRegistry {
    fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }
    fn insert(&self, entry: ThreadEntry) -> u64 {
        let id = self.next_id.fetch_add(1, AtomicOrdering::SeqCst);
        let mut m = self.map.lock().unwrap();
        m.insert(id, entry);
        id
    }
    fn remove(&self, id: u64) -> Option<ThreadEntry> {
        let mut m = self.map.lock().unwrap();
        m.remove(&id)
    }
}

// Simple allocation registry to track allocations (base -> size)
struct AllocRegistry {
    map: Mutex<std::collections::HashMap<usize, usize>>,
}

impl AllocRegistry {
    fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
        }
    }
    fn insert(&self, base: usize, size: usize) {
        let mut m = self.map.lock().unwrap();
        m.insert(base, size);
    }
    fn remove(&self, base: usize) -> Option<usize> {
        let mut m = self.map.lock().unwrap();
        m.remove(&base)
    }
    fn get(&self, base: usize) -> Option<usize> {
        let m = self.map.lock().unwrap();
        m.get(&base).copied()
    }
}

fn read_u16_le(data: &[u8], off: &mut usize) -> u16 {
    let mut v = 0u16;
    v |= data[*off] as u16;
    v |= (data[*off + 1] as u16) << 8;
    *off += 2;
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

fn read_u64_from_memory(addr: u64, state: &State) -> u64 {
    let a = addr as usize;
    let mem = state.memory.lock().unwrap();
    if a >= mem.len() {
        return 0;
    }
    let end = a.saturating_add(8);
    if end > mem.len() {
        return 0;
    }
    let mut v = 0u64;
    for i in 0..8 {
        v |= (mem[a + i] as u64) << (8 * i);
    }
    v
}

fn write_u64_to_memory(addr: u64, value: u64, state: &mut State) {
    let a = addr as usize;
    if a > usize::MAX - 8 {
        return;
    }
    let mut mem = state.memory.lock().unwrap();
    if mem.len() < a + 8 {
        let old = mem.len();
        mem.resize(a + 8, 0);
        // Warn when writes extend memory (helps catch likely bugs)
        state.warnings.push(format!(
            "warning: write extended memory from {} to {}\n  --> pc: 0x{:X}\n   = note: store at 0x{:X} (size 8)",
            old, mem.len(), state.rip, a
        ));
    }
    let mut v = value;
    for i in 0..8 {
        mem[a + i] = (v & 0xFF) as u8;
        v >>= 8;
    }
}

// Helper: read a C-style NUL-terminated string from shared memory
fn read_c_string(addr: u64, state: &State) -> Option<String> {
    let start = addr as usize;
    let mem = state.memory.lock().unwrap();
    if start >= mem.len() {
        return None;
    }
    let mut i = start;
    let mut bytes: Vec<u8> = Vec::new();
    while i < mem.len() {
        let b = mem[i];
        i += 1;
        if b == 0 {
            break;
        }
        bytes.push(b);
    }
    String::from_utf8(bytes).ok()
}

// -------- MNI C-ABI host context (optional, for native libraries) --------
#[repr(C)]
pub struct MniVmCtx {
    pub api_version: u32,
    pub user_data: *mut c_void, // internal: points to State
    pub memory_ptr: *mut u8,
    pub memory_len: usize,
    // host ops
    pub read_u64: extern "C" fn(*mut MniVmCtx, u64) -> u64,
    pub write_u64: extern "C" fn(*mut MniVmCtx, u64, u64),
    pub get_reg_by_name: extern "C" fn(*mut MniVmCtx, *const c_char) -> u64,
    pub set_reg_by_name: extern "C" fn(*mut MniVmCtx, *const c_char, u64),
}

extern "C" fn host_read_u64(ctx: *mut MniVmCtx, addr: u64) -> u64 {
    unsafe {
        if ctx.is_null() {
            return 0;
        }
        let s = &mut *((*ctx).user_data as *mut State);
        read_u64_from_memory(addr, s)
    }
}

extern "C" fn host_write_u64(ctx: *mut MniVmCtx, addr: u64, value: u64) {
    unsafe {
        if ctx.is_null() {
            return;
        }
        let s = &mut *((*ctx).user_data as *mut State);
        write_u64_to_memory(addr, value, s);
        // update pointers after potential resize
        {
            let mem = s.memory.lock().unwrap();
            (*ctx).memory_ptr = mem.as_ptr() as *mut u8;
            (*ctx).memory_len = mem.len();
        }
    }
}

extern "C" fn host_get_reg_by_name(ctx: *mut MniVmCtx, name: *const c_char) -> u64 {
    unsafe {
        if ctx.is_null() || name.is_null() {
            return 0;
        }
        let s = &mut *((*ctx).user_data as *mut State);
        let nm = match CStr::from_ptr(name).to_str() {
            Ok(v) => v,
            Err(_) => return 0,
        };
        let nm_up = nm.to_uppercase();
        let idmap = RegisterMap::build_name_to_id();
        if let Some(id) = idmap.get(nm_up.as_str()) {
            *s.regs.get(id).unwrap_or(&0)
        } else {
            0
        }
    }
}

extern "C" fn host_set_reg_by_name(ctx: *mut MniVmCtx, name: *const c_char, value: u64) {
    unsafe {
        if ctx.is_null() || name.is_null() {
            return;
        }
        let s = &mut *((*ctx).user_data as *mut State);
        let nm = match CStr::from_ptr(name).to_str() {
            Ok(v) => v,
            Err(_) => return,
        };
        let nm_up = nm.to_uppercase();
        let idmap = RegisterMap::build_name_to_id();
        if let Some(id) = idmap.get(nm_up.as_str()) {
            s.regs.insert(*id, value);
        }
    }
}

fn get_operand(code: &[u8], pc: &mut usize, state: &mut State) -> u64 {
    let mode = code[*pc];
    *pc += 1;
    let val = read_u64_le(code, pc);
    match mode {
        0 => val,
        1 => {
            let id = val as u16;
            *state.regs.get(&id).unwrap_or(&0)
        }
        2 => val,
        3 => {
            let a = val as usize;
            {
                let mem = state.memory.lock().unwrap();
                if a >= mem.len() || a.saturating_add(8) > mem.len() {
                    state.warnings.push(format!(
                        "warning: read_u64 OOB at 0x{:X} (mem size {})\n  --> pc: 0x{:X}",
                        a,
                        mem.len(),
                        state.rip
                    ));
                }
            }
            read_u64_from_memory(val, state)
        }
        4 => {
            let id = val as u16;
            let addr = *state.regs.get(&id).unwrap_or(&0);
            let a = addr as usize;
            {
                let mem = state.memory.lock().unwrap();
                if a >= mem.len() || a.saturating_add(8) > mem.len() {
                    state.warnings.push(format!(
                        "warning: read_u64 OOB at 0x{:X} (mem size {})\n  --> pc: 0x{:X}",
                        a,
                        mem.len(),
                        state.rip
                    ));
                }
            }
            read_u64_from_memory(addr, state)
        }
        _ => val,
    }
}

fn set_operand(code: &[u8], pc: &mut usize, state: &mut State, value: u64) {
    let mode = code[*pc];
    *pc += 1;
    let val = read_u64_le(code, pc);
    match mode {
        1 => {
            let id = val as u16;
            state.regs.insert(id, value);
        }
        3 => {
            write_u64_to_memory(val, value, state);
        }
        4 => {
            let id = val as u16;
            let addr = *state.regs.get(&id).unwrap_or(&0);
            write_u64_to_memory(addr, value, state);
        }
        _ => {}
    }
}

fn update_add_flags(a: u64, b: u64, r: u64, state: &mut State) {
    // ZF,SF,CF,OF
    let zf = r == 0;
    let sf = (r as i64) < 0;
    let cf = r < a;
    let sa = a as i64;
    let sb = b as i64;
    let sr = r as i64;
    let of = (sa > 0 && sb > 0 && sr < 0) || (sa < 0 && sb < 0 && sr > 0);
    state.flags = (zf, sf, cf, of);
}
fn update_sub_flags(a: u64, b: u64, r: u64, state: &mut State) {
    let zf = r == 0;
    let sf = (r as i64) < 0;
    let cf = a < b;
    let sa = a as i64;
    let sb = b as i64;
    let sr = r as i64;
    let of = (sa >= 0 && sb < 0 && sr < 0) || (sa < 0 && sb >= 0 && sr >= 0);
    state.flags = (zf, sf, cf, of);
}

// Note: read_c_string previously accepted a slice; with shared memory we read under a lock.
// Use the version defined earlier that takes (&State).

fn load_lua_modules(registry: &mut ModuleRegistry) -> Result<(), String> {
    let modules_dir = Path::new("modules");
    if !modules_dir.is_dir() {
        return Ok(());
    }
    let lua = Lua::new();
    for entry in fs::read_dir(modules_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.eq_ignore_ascii_case("lua"))
            .unwrap_or(false)
        {
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
                    let f: Option<LuaTable> =
                        globals.get("MNI_FUNCTIONS").map_err(|e| e.to_string())?;
                    (m, f)
                }
            };
            if let (Some(mod_name), Some(funcs)) = (module_name, funcs_table) {
                for pair in funcs.pairs::<String, LuaFunction>() {
                    let (fname, lf) = pair.map_err(|e| e.to_string())?;
                    // Only register if not already present
                    let already = registry.lookup(&mod_name, &fname).is_some();
                    if already {
                        debug_println!(
                            "[Lua MNI] Skipping already-registered {}.{}",
                            mod_name,
                            fname
                        );
                        continue;
                    }
                    debug_println!("[Lua MNI] Registering {}.{}", mod_name, fname);
                    let lua_ref = lua.clone();
                    registry.register(&mod_name, &fname, move |ctx: &mut MniCtx| {
                        let args_arr = lua_ref.create_table().unwrap();
                        for (i, a) in ctx.args.iter().enumerate() {
                            let _ = args_arr.set((i + 1) as i64, a.as_str());
                        }
                        let regs_tbl = lua_ref.create_table().unwrap();
                        for (id, val) in ctx.state.regs.iter() {
                            if let Some(name) = RegisterMap::build_id_to_name().get(id) {
                                let _ = regs_tbl.set(name.as_str(), *val as i64);
                            }
                        }
                        let ret: mlua::Result<LuaValue> = lf.call((args_arr, regs_tbl));
                        if let Ok(val) = ret {
                            match val {
                                LuaValue::Nil => {}
                                LuaValue::String(s) => {
                                    if let Ok(st) = s.to_str() {
                                        println!("{}", st);
                                    }
                                }
                                LuaValue::Table(t) => {
                                    if let Ok(Some(out)) = t.get::<Option<String>>("out") {
                                        println!("{}", out);
                                    }
                                    if let Ok(Some(upd)) = t.get::<Option<LuaTable>>("regs") {
                                        for pair in upd.pairs::<String, i64>() {
                                            if let Ok((rname, ival)) = pair {
                                                if let Some(id) =
                                                    RegisterMap::build_name_to_id().get(&rname)
                                                {
                                                    ctx.state.regs.insert(*id, ival as u64);
                                                }
                                            }
                                        }
                                    }
                                    if let Ok(Some(store)) = t.get::<Option<LuaTable>>("store") {
                                        let mut dest: Option<u64> = None;
                                        if let Ok(Some(addr)) = store.get::<Option<u64>>("addr") {
                                            dest = Some(addr);
                                        } else if let Ok(Some(rname)) =
                                            store.get::<Option<String>>("reg")
                                        {
                                            if let Some(id) =
                                                RegisterMap::build_name_to_id().get(&rname)
                                            {
                                                dest = Some(*ctx.state.regs.get(id).unwrap_or(&0));
                                            }
                                        }
                                        if let Some(base) = dest {
                                            let base_usize = base as usize;
                                            if let Ok(Some(s)) =
                                                store.get::<Option<String>>("string")
                                            {
                                                let mut bytes = s.into_bytes();
                                                bytes.push(0);
                                                let mut mem = ctx.state.memory.lock().unwrap();
                                                if mem.len() < base_usize + bytes.len() {
                                                    mem.resize(base_usize + bytes.len(), 0);
                                                }
                                                mem[base_usize..base_usize + bytes.len()]
                                                    .copy_from_slice(&bytes);
                                            } else if let Ok(Some(arr)) =
                                                store.get::<Option<LuaTable>>("bytes")
                                            {
                                                let len = arr.len().unwrap_or(0) as usize;
                                                let mut bytes: Vec<u8> = Vec::with_capacity(len);
                                                for i in 1..=len {
                                                    if let Ok(v) = arr.get::<i64>(i as i64) {
                                                        bytes.push(v as u8);
                                                    }
                                                }
                                                let mut mem = ctx.state.memory.lock().unwrap();
                                                if mem.len() < base_usize + bytes.len() {
                                                    mem.resize(base_usize + bytes.len(), 0);
                                                }
                                                mem[base_usize..base_usize + bytes.len()]
                                                    .copy_from_slice(&bytes);
                                            }
                                        }
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
    Ok(())
}

pub fn run_path(path: &str) -> Result<(), String> {
    let masi = disassembler::load(path)?;
    run_masi(&masi)
}

/// Run MASI directly from a byte buffer.
pub fn run_masi_from_bytes(buf: &[u8]) -> Result<State, String> {
    let masi = disassembler::parse_masi_bytes(buf)?;
    // use default IO runner
    let _ = run_masi(&masi)?;
    // run_masi returns Result<(),String> so call run_masi_with_io to get State
    let mut out = io::stdout();
    let mut err = io::stderr();
    let mut stdin_lock = io::stdin().lock();
    run_masi_with_io(
        &masi,
        &mut out as &mut dyn Write,
        &mut err as &mut dyn Write,
        Some(&mut stdin_lock as &mut dyn BufRead),
    )
}

/// Compile a MASM source string into MASI bytes. This is a stub; the assembler is not implemented here.
pub fn compile_source_to_masi_bytes(_src: &str) -> Result<Vec<u8>, String> {
    // Forward to the assembler implementation in this crate.
    // This returns MASI file bytes that can be parsed by `disassembler::parse_masi_bytes` or written to disk.
    crate::assembler::assemble_to_masi(_src)
}

pub fn run_masi(masi: &MASIFile) -> Result<(), String> {
    // Default runner using real stdio; discards final state
    let mut out = io::stdout();
    let mut err = io::stderr();
    let mut stdin_lock = io::stdin().lock();
    let _state = run_masi_with_io(
        masi,
        &mut out as &mut dyn Write,
        &mut err as &mut dyn Write,
        Some(&mut stdin_lock as &mut dyn BufRead),
    )?;
    Ok(())
}

/// Run a MASI file with injectable IO and return final VM state for testing.
///
/// - out: where OUT/COUT to port 1 prints go (newline semantics preserved)
/// - err: where OUT/COUT to port 2 prints go
/// - input: optional buffered reader used by IN; if None, reads from stdin
pub fn run_masi_with_io(
    masi: &MASIFile,
    out: &mut dyn Write,
    err: &mut dyn Write,
    mut input: Option<&mut dyn BufRead>,
) -> Result<State, String> {
    let mut state = State::default();
    // initialize shared memory from MASI data
    state.memory = Arc::new(Mutex::new(masi.data.clone()));
    let mut registry = ModuleRegistry::new();
    // Register built-in MNI modules implemented in Rust
    #[cfg(feature = "raylib_mni")]
    mni_raylib::register_raylib_mni(&mut registry);

    // Built-in Rust MNI shims
    {
        // Thread registry (shared for MNI functions)
        use std::sync::Once;
        static mut THREAD_REGISTRY_PTR: *const ThreadRegistry = 0 as *const ThreadRegistry;
        static THREAD_REGISTRY_INIT: Once = Once::new();
        THREAD_REGISTRY_INIT.call_once(|| {
            let reg = Box::new(ThreadRegistry::new());
            unsafe {
                THREAD_REGISTRY_PTR = Box::into_raw(reg);
            }
        });
        let thread_registry: &'static ThreadRegistry = unsafe { &*THREAD_REGISTRY_PTR };
        // Allocation registry (shared for memory bookkeeping)
        static mut ALLOC_REGISTRY_PTR: *const AllocRegistry = 0 as *const AllocRegistry;
        static ALLOC_REGISTRY_INIT: Once = Once::new();
        ALLOC_REGISTRY_INIT.call_once(|| {
            let reg = Box::new(AllocRegistry::new());
            unsafe {
                ALLOC_REGISTRY_PTR = Box::into_raw(reg);
            }
        });
        let alloc_registry: &'static AllocRegistry = unsafe { &*ALLOC_REGISTRY_PTR };

        // tool.set_rax already exists via Lua examples, but provide a basic Rust one if not provided by Lua
        let nm = RegisterMap::build_name_to_id();
        let rax = *nm.get("RAX").unwrap_or(&0);
        registry.register("tool", "set_rax", move |ctx: &mut MniCtx| {
            if let Some(first) = ctx.args.get(0) {
                if let Ok(v) = first.parse::<i64>() {
                    ctx.state.regs.insert(rax, v as u64);
                }
            }
        });
        // Memory.allocate size -> R1
        let r1 = *nm.get("R1").unwrap_or(&0);
        {
            let alloc_registry = alloc_registry;
            registry.register("Memory", "allocate", move |ctx: &mut MniCtx| {
                if let Some(sz_s) = ctx.args.get(0) {
                    if let Ok(sz) = sz_s.parse::<usize>() {
                        let mut mem = ctx.state.memory.lock().unwrap();
                        let base = mem.len();
                        mem.resize(base + sz, 0);
                        alloc_registry.insert(base, sz);
                        ctx.state.regs.insert(r1, base as u64);
                    }
                }
            });
        }
        // Memory.free ptr (no-op in simple flat memory model)
        {
            let alloc_registry = alloc_registry;
            registry.register("Memory", "free", move |ctx: &mut MniCtx| {
                if let Some(ptr_s) = ctx.args.get(0) {
                    if let Ok(ptr) = ptr_s.parse::<usize>() {
                        // remove allocation bookkeeping if present
                        alloc_registry.remove(ptr);
                    }
                }
            });
        }
        // Math.sqrt in-place: Math.sqrt src_fpr dest_fpr (by names)
        registry.register("Math", "sqrt", move |ctx: &mut MniCtx| {
            if ctx.args.len() >= 2 {
                let name_to_id = RegisterMap::build_name_to_id();
                let src = &ctx.args[0];
                let dst = &ctx.args[1];
                if let (Some(&sid), Some(&did)) = (name_to_id.get(src), name_to_id.get(dst)) {
                    let vbits = *ctx.state.regs.get(&sid).unwrap_or(&0);
                    let v = f64::from_bits(vbits);
                    ctx.state.regs.insert(did, v.sqrt().to_bits());
                }
            }
        });
        // Memory.write_bytes dest_addr src_label -> writes NUL-terminated string into memory at dest_addr, returns bytes written in RAX
        {
            let rax = *nm.get("RAX").unwrap_or(&0);
            let alloc_registry = alloc_registry;
            registry.register("Memory", "write_bytes", move |ctx: &mut MniCtx| {
                if ctx.args.len() >= 2 {
                    if let (Ok(dest), src) = (ctx.args[0].parse::<usize>(), &ctx.args[1]) {
                        // src is usually a string value passed by the assembler; try to write it with NUL
                        let mut bytes = src.as_bytes().to_vec();
                        bytes.push(0);
                        let mut mem = ctx.state.memory.lock().unwrap();
                        if dest + bytes.len() > mem.len() {
                            mem.resize(dest + bytes.len(), 0);
                        }
                        mem[dest..dest + bytes.len()].copy_from_slice(&bytes);
                        ctx.state.regs.insert(rax, bytes.len() as u64);
                        // record allocation if it matches end of heap
                        alloc_registry.insert(dest, bytes.len());
                    }
                }
            });
        }

        // Memory.realloc ptr new_size -> returns new_ptr in RAX or error (u64::MAX)
        {
            let rax = *nm.get("RAX").unwrap_or(&0);
            let alloc_registry = alloc_registry;
            registry.register("Memory", "realloc", move |ctx: &mut MniCtx| {
                if ctx.args.len() >= 2 {
                    if let (Ok(ptr), Ok(new_sz)) =
                        (ctx.args[0].parse::<usize>(), ctx.args[1].parse::<usize>())
                    {
                        // locate old size
                        if let Some(old_sz) = alloc_registry.get(ptr) {
                            let mut mem = ctx.state.memory.lock().unwrap();
                            // naive realloc: if ptr is end of heap, extend or shrink in-place
                            if ptr + old_sz == mem.len() {
                                if new_sz <= old_sz {
                                    mem.truncate(ptr + new_sz);
                                    alloc_registry.insert(ptr, new_sz);
                                    ctx.state.regs.insert(rax, ptr as u64);
                                    return;
                                } else {
                                    mem.resize(ptr + new_sz, 0);
                                    alloc_registry.insert(ptr, new_sz);
                                    ctx.state.regs.insert(rax, ptr as u64);
                                    return;
                                }
                            } else {
                                // otherwise allocate new block at end and copy
                                let base = mem.len();
                                let copy_len = std::cmp::min(old_sz, new_sz);
                                // copy source bytes into temp buffer before resizing to avoid
                                // simultaneous immutable/mutable borrows of `mem`
                                let temp: Vec<u8> = mem[ptr..ptr + copy_len].to_vec();
                                mem.resize(base + new_sz, 0);
                                mem[base..base + copy_len].copy_from_slice(&temp);
                                alloc_registry.insert(base, new_sz);
                                alloc_registry.remove(ptr);
                                ctx.state.regs.insert(rax, base as u64);
                                return;
                            }
                        }
                    }
                }
                ctx.state.regs.insert(rax, u64::MAX);
            });
        }

        // Thread.spawn: args = [label_addr (numeric) , optional: arg_addr (c-string address)] -> returns thread id in RAX
        {
            let thread_registry = thread_registry;
            let masi_arc = Arc::new(masi.clone());
            registry.register("Thread", "spawn", move |ctx: &mut MniCtx| {
                // parse label address
                if let Some(first) = ctx.args.get(0) {
                    if let Ok(label) = first.parse::<u64>() {
                        // clone state but share memory and code
                        let mut child_state = State::default();
                        child_state.memory = ctx.state.memory.clone();
                        child_state.regs = ctx.state.regs.clone();
                        child_state.rip = label as u64;
                        // spawn thread that runs until halt
                        let masi_for_thread = masi_arc.clone();
                        let handle = thread::spawn(move || {
                            // run the MASI using the default IO runner; discard result
                            let _ = run_masi(&*masi_for_thread);
                        });
                        let entry = ThreadEntry { handle };
                        let id = thread_registry.insert(entry);
                        // place id into RAX
                        let rax = *RegisterMap::build_name_to_id().get("RAX").unwrap_or(&0);
                        ctx.state.regs.insert(rax, id);
                    }
                }
            });
        }

        // Thread.join: args = [thread_id] -> returns 0 on success, -1 on error
        {
            let thread_registry = thread_registry;
            registry.register("Thread", "join", move |ctx: &mut MniCtx| {
                if let Some(first) = ctx.args.get(0) {
                    if let Ok(id) = first.parse::<u64>() {
                        if let Some(entry) = thread_registry.remove(id) {
                            let _ = entry.handle.join();
                            let rax = *RegisterMap::build_name_to_id().get("RAX").unwrap_or(&0);
                            ctx.state.regs.insert(rax, 0);
                            return;
                        }
                    }
                }
                let rax = *RegisterMap::build_name_to_id().get("RAX").unwrap_or(&0);
                ctx.state.regs.insert(rax, u64::MAX);
            });
        }

        // Thread.detach: args = [thread_id] -> returns 0 on success, -1 on error
        {
            let thread_registry = thread_registry;
            registry.register("Thread", "detach", move |ctx: &mut MniCtx| {
                if let Some(first) = ctx.args.get(0) {
                    if let Ok(id) = first.parse::<u64>() {
                        if thread_registry.remove(id).is_some() {
                            let rax = *RegisterMap::build_name_to_id().get("RAX").unwrap_or(&0);
                            ctx.state.regs.insert(rax, 0);
                            return;
                        }
                    }
                }
                let rax = *RegisterMap::build_name_to_id().get("RAX").unwrap_or(&0);
                ctx.state.regs.insert(rax, u64::MAX);
            });
        }
    }

    // Load Lua modules if present
    let _ = load_lua_modules(&mut registry);

    let code = &masi.code;
    let mut pc: usize = masi.entry as usize;
    while pc < code.len() {
        state.rip = pc as u64;
        let opcode = code[pc];
        // Debugger hook: allow debugger to pause/step/exit before executing this instruction
        if let Some(ctrl) = with_debugger(|d| d.before_execute(masi, &state, pc, opcode)) {
            match ctrl {
                DebuggerControl::Continue => {}
                DebuggerControl::Exit => {
                    return Ok(state);
                }
            }
        }
        let byte = opcode;
        pc += 1;
        match byte {
            x if x == Op::Nop as u8 => {
                continue;
            }
            x if x == Op::Hlt as u8 => {
                return Ok(state);
            }
            x if x == Op::Mov as u8 => {
                // dest then source
                let dest_pos = pc;
                let _mode = code[pc];
                pc += 1;
                let _id_skip = read_u64_le(code, &mut pc);
                let src_val = get_operand(code, &mut pc, &mut state);
                let after_src = pc;
                let mut tmp_pc = dest_pos;
                set_operand(code, &mut tmp_pc, &mut state, src_val);
                pc = after_src;
            }
            // Floating point move: same semantics as MOV, bitwise copy of 64-bit payload
            x if x == Op::FMov as u8 => {
                let dest_pos = pc;
                let _mode = code[pc];
                pc += 1;
                let _id_skip = read_u64_le(code, &mut pc);
                let src_bits = get_operand(code, &mut pc, &mut state);
                let after_src = pc;
                let mut tmp_pc = dest_pos;
                set_operand(code, &mut tmp_pc, &mut state, src_bits);
                pc = after_src;
            }
            x if x == Op::Add as u8 => {
                let dest_mode = code[pc];
                pc += 1;
                let dest_id64 = read_u64_le(code, &mut pc);
                let src_val = get_operand(code, &mut pc, &mut state);
                if dest_mode == 1 {
                    let id = dest_id64 as u16;
                    let a = *state.regs.get(&id).unwrap_or(&0);
                    let r = a.wrapping_add(src_val);
                    state.regs.insert(id, r);
                    update_add_flags(a, src_val, r, &mut state);
                }
            }
            // Floating point arithmetic: operate on f64 values but store as raw u64 bits; flags set only by FCMP
            x if x == Op::FAdd as u8 => {
                let dest_mode = code[pc];
                pc += 1;
                let dest_id64 = read_u64_le(code, &mut pc);
                let src_bits = get_operand(code, &mut pc, &mut state);
                if dest_mode == 1 {
                    let id = dest_id64 as u16;
                    let a_bits = *state.regs.get(&id).unwrap_or(&0);
                    let a = f64::from_bits(a_bits);
                    let b = f64::from_bits(src_bits);
                    let r = a + b;
                    if r.is_nan() || r.is_infinite() {
                        state.warnings.push(format!(
                            "warning: FP result is {}\n  --> pc: 0x{:X}",
                            if r.is_nan() { "NaN" } else { "infinite" },
                            state.rip
                        ));
                    }
                    state.regs.insert(id, r.to_bits());
                }
            }
            x if x == Op::Sub as u8 => {
                let dest_mode = code[pc];
                pc += 1;
                let dest_id64 = read_u64_le(code, &mut pc);
                let src_val = get_operand(code, &mut pc, &mut state);
                if dest_mode == 1 {
                    let id = dest_id64 as u16;
                    let a = *state.regs.get(&id).unwrap_or(&0);
                    let r = a.wrapping_sub(src_val);
                    state.regs.insert(id, r);
                    update_sub_flags(a, src_val, r, &mut state);
                }
            }
            x if x == Op::FSub as u8 => {
                let dest_mode = code[pc];
                pc += 1;
                let dest_id64 = read_u64_le(code, &mut pc);
                let src_bits = get_operand(code, &mut pc, &mut state);
                if dest_mode == 1 {
                    let id = dest_id64 as u16;
                    let a_bits = *state.regs.get(&id).unwrap_or(&0);
                    let a = f64::from_bits(a_bits);
                    let b = f64::from_bits(src_bits);
                    let r = a - b;
                    if r.is_nan() || r.is_infinite() {
                        state.warnings.push(format!(
                            "warning: FP result is {}\n  --> pc: 0x{:X}",
                            if r.is_nan() { "NaN" } else { "infinite" },
                            state.rip
                        ));
                    }
                    state.regs.insert(id, r.to_bits());
                }
            }
            x if x == Op::FMul as u8 => {
                let dest_mode = code[pc];
                pc += 1;
                let dest_id64 = read_u64_le(code, &mut pc);
                let src_bits = get_operand(code, &mut pc, &mut state);
                if dest_mode == 1 {
                    let id = dest_id64 as u16;
                    let a_bits = *state.regs.get(&id).unwrap_or(&0);
                    let a = f64::from_bits(a_bits);
                    let b = f64::from_bits(src_bits);
                    let r = a * b;
                    if r.is_nan() || r.is_infinite() {
                        state.warnings.push(format!(
                            "warning: FP result is {}\n  --> pc: 0x{:X}",
                            if r.is_nan() { "NaN" } else { "infinite" },
                            state.rip
                        ));
                    }
                    state.regs.insert(id, r.to_bits());
                }
            }
            x if x == Op::FDiv as u8 => {
                let dest_mode = code[pc];
                pc += 1;
                let dest_id64 = read_u64_le(code, &mut pc);
                let src_bits = get_operand(code, &mut pc, &mut state);
                if dest_mode == 1 {
                    let id = dest_id64 as u16;
                    let a_bits = *state.regs.get(&id).unwrap_or(&0);
                    let a = f64::from_bits(a_bits);
                    let b = f64::from_bits(src_bits);
                    let r = a / b; // IEEE-754: handles div by zero -> inf or NaN
                    if r.is_nan() || r.is_infinite() {
                        state.warnings.push(format!(
                            "warning: FP result is {}\n  --> pc: 0x{:X}",
                            if r.is_nan() { "NaN" } else { "infinite" },
                            state.rip
                        ));
                    }
                    state.regs.insert(id, r.to_bits());
                }
            }
            x if x == Op::Cmp as u8 => {
                let a = get_operand(code, &mut pc, &mut state);
                let b = get_operand(code, &mut pc, &mut state);
                let r = a.wrapping_sub(b);
                update_sub_flags(a, b, r, &mut state);
            }
            x if x == Op::FCmp as u8 => {
                let a_bits = get_operand(code, &mut pc, &mut state);
                let b_bits = get_operand(code, &mut pc, &mut state);
                let a = f64::from_bits(a_bits);
                let b = f64::from_bits(b_bits);
                if a.is_nan() || b.is_nan() {
                    // unordered
                    state.flags = (false, false, false, true); // ZF, SF, CF, OF=unordered
                } else if a == b {
                    state.flags = (true, false, false, false); // equal
                } else if a < b {
                    state.flags = (false, true, false, false); // less-than
                } else {
                    state.flags = (false, false, true, false); // greater-than
                }
            }
            x if x == Op::Jmp as u8 => {
                let t = get_operand(code, &mut pc, &mut state);
                let target = t as usize;
                if target >= code.len() {
                    let msg = format!(
                        "error[EVM004]: invalid jump target\n  --> pc: 0x{:X}\n   = note: jump target 0x{:X}, code size {}",
                        state.rip,
                        target,
                        code.len()
                    );
                    state.errors.push(msg.clone());
                    return Err(msg);
                }
                pc = target;
            }
            x if x == Op::Je as u8 => {
                let t = get_operand(code, &mut pc, &mut state);
                if state.flags.0 {
                    let target = t as usize;
                    if target >= code.len() {
                        let msg = format!(
                            "error[EVM004]: invalid jump target\n  --> pc: 0x{:X}\n   = note: jump target 0x{:X}, code size {}",
                            state.rip,
                            target,
                            code.len()
                        );
                        state.errors.push(msg.clone());
                        return Err(msg);
                    }
                    pc = target;
                }
            }
            x if x == Op::Jne as u8 => {
                let t = get_operand(code, &mut pc, &mut state);
                if !state.flags.0 {
                    let target = t as usize;
                    if target >= code.len() {
                        let msg = format!(
                            "error[EVM004]: invalid jump target\n  --> pc: 0x{:X}\n   = note: jump target 0x{:X}, code size {}",
                            state.rip,
                            target,
                            code.len()
                        );
                        state.errors.push(msg.clone());
                        return Err(msg);
                    }
                    pc = target;
                }
            }
            x if x == Op::Jl as u8 => {
                let t = get_operand(code, &mut pc, &mut state);
                let (zf, sf, _cf, of) = state.flags;
                if (sf ^ of) && !zf {
                    let target = t as usize;
                    if target >= code.len() {
                        let msg = format!(
                            "error[EVM004]: invalid jump target\n  --> pc: 0x{:X}\n   = note: jump target 0x{:X}, code size {}",
                            state.rip,
                            target,
                            code.len()
                        );
                        state.errors.push(msg.clone());
                        return Err(msg);
                    }
                    pc = target;
                }
            }
            x if x == Op::Jle as u8 => {
                let t = get_operand(code, &mut pc, &mut state);
                let (zf, sf, _cf, of) = state.flags;
                if zf || (sf ^ of) {
                    let target = t as usize;
                    if target >= code.len() {
                        let msg = format!(
                            "error[EVM004]: invalid jump target\n  --> pc: 0x{:X}\n   = note: jump target 0x{:X}, code size {}",
                            state.rip,
                            target,
                            code.len()
                        );
                        state.errors.push(msg.clone());
                        return Err(msg);
                    }
                    pc = target;
                }
            }
            x if x == Op::Jg as u8 => {
                let t = get_operand(code, &mut pc, &mut state);
                let (zf, sf, _cf, of) = state.flags;
                if !zf && !(sf ^ of) {
                    let target = t as usize;
                    if target >= code.len() {
                        let msg = format!(
                            "error[EVM004]: invalid jump target\n  --> pc: 0x{:X}\n   = note: jump target 0x{:X}, code size {}",
                            state.rip,
                            target,
                            code.len()
                        );
                        state.errors.push(msg.clone());
                        return Err(msg);
                    }
                    pc = target;
                }
            }
            x if x == Op::Jge as u8 => {
                let t = get_operand(code, &mut pc, &mut state);
                let (_zf, sf, _cf, of) = state.flags;
                if !(sf ^ of) {
                    let target = t as usize;
                    if target >= code.len() {
                        let msg = format!(
                            "error[EVM004]: invalid jump target\n  --> pc: 0x{:X}\n   = note: jump target 0x{:X}, code size {}",
                            state.rip,
                            target,
                            code.len()
                        );
                        state.errors.push(msg.clone());
                        return Err(msg);
                    }
                    pc = target;
                }
            }
            x if x == Op::FJe as u8 => {
                let t = get_operand(code, &mut pc, &mut state);
                if state.flags.0 {
                    let target = t as usize;
                    if target >= code.len() {
                        let msg = format!(
                            "error[EVM004]: invalid jump target\n  --> pc: 0x{:X}\n   = note: jump target 0x{:X}, code size {}",
                            state.rip,
                            target,
                            code.len()
                        );
                        state.errors.push(msg.clone());
                        return Err(msg);
                    }
                    pc = target;
                }
            }
            x if x == Op::FJne as u8 => {
                let t = get_operand(code, &mut pc, &mut state);
                if !state.flags.0 {
                    let target = t as usize;
                    if target >= code.len() {
                        let msg = format!(
                            "error[EVM004]: invalid jump target\n  --> pc: 0x{:X}\n   = note: jump target 0x{:X}, code size {}",
                            state.rip,
                            target,
                            code.len()
                        );
                        state.errors.push(msg.clone());
                        return Err(msg);
                    }
                    pc = target;
                }
            }
            x if x == Op::FJlt as u8 => {
                let t = get_operand(code, &mut pc, &mut state);
                if state.flags.1 {
                    let target = t as usize;
                    if target >= code.len() {
                        let msg = format!(
                            "error[EVM004]: invalid jump target\n  --> pc: 0x{:X}\n   = note: jump target 0x{:X}, code size {}",
                            state.rip,
                            target,
                            code.len()
                        );
                        state.errors.push(msg.clone());
                        return Err(msg);
                    }
                    pc = target;
                }
            }
            x if x == Op::FJle as u8 => {
                let t = get_operand(code, &mut pc, &mut state);
                if state.flags.0 || state.flags.1 {
                    let target = t as usize;
                    if target >= code.len() {
                        let msg = format!(
                            "error[EVM004]: invalid jump target\n  --> pc: 0x{:X}\n   = note: jump target 0x{:X}, code size {}",
                            state.rip,
                            target,
                            code.len()
                        );
                        state.errors.push(msg.clone());
                        return Err(msg);
                    }
                    pc = target;
                }
            }
            x if x == Op::FJgt as u8 => {
                let t = get_operand(code, &mut pc, &mut state);
                if state.flags.2 {
                    let target = t as usize;
                    if target >= code.len() {
                        let msg = format!(
                            "error[EVM004]: invalid jump target\n  --> pc: 0x{:X}\n   = note: jump target 0x{:X}, code size {}",
                            state.rip,
                            target,
                            code.len()
                        );
                        state.errors.push(msg.clone());
                        return Err(msg);
                    }
                    pc = target;
                }
            }
            x if x == Op::FJge as u8 => {
                let t = get_operand(code, &mut pc, &mut state);
                if state.flags.0 || state.flags.2 {
                    let target = t as usize;
                    if target >= code.len() {
                        let msg = format!(
                            "error[EVM004]: invalid jump target\n  --> pc: 0x{:X}\n   = note: jump target 0x{:X}, code size {}",
                            state.rip,
                            target,
                            code.len()
                        );
                        state.errors.push(msg.clone());
                        return Err(msg);
                    }
                    pc = target;
                }
            }
            x if x == Op::FJuo as u8 => {
                let t = get_operand(code, &mut pc, &mut state);
                if state.flags.3 {
                    let target = t as usize;
                    if target >= code.len() {
                        let msg = format!(
                            "error[EVM004]: invalid jump target\n  --> pc: 0x{:X}\n   = note: jump target 0x{:X}, code size {}",
                            state.rip,
                            target,
                            code.len()
                        );
                        state.errors.push(msg.clone());
                        return Err(msg);
                    }
                    pc = target;
                }
            }
            x if x == Op::Call as u8 => {
                let t = get_operand(code, &mut pc, &mut state);
                let target = t as usize;
                if target >= code.len() {
                    let msg = format!(
                        "error[EVM003]: invalid call target\n  --> pc: 0x{:X}\n   = note: call target 0x{:X}, code size {}",
                        state.rip,
                        target,
                        code.len()
                    );
                    state.errors.push(msg.clone());
                    return Err(msg);
                }
                state.stack.push(pc as u64);
                pc = target;
            }
            x if x == Op::Ret as u8 => {
                if let Some(ret) = state.stack.pop() {
                    let target = ret as usize;
                    if target >= code.len() {
                        let msg = format!(
                            "error[EVM002]: return address outside code section\n  --> pc: 0x{:X}\n   = note: return target 0x{:X}, code size {}\n   = help: did you POP the return address by accident? (stack imbalance)",
                            state.rip,
                            target,
                            code.len()
                        );
                        state.errors.push(msg.clone());
                        return Err(msg);
                    }
                    pc = target;
                }
            }
            x if x == Op::Push as u8 => {
                let v = get_operand(code, &mut pc, &mut state);
                state.stack.push(v);
            }
            x if x == Op::Pop as u8 => {
                let dest_mode = code[pc];
                pc += 1;
                let dest_id64 = read_u64_le(code, &mut pc);
                if dest_mode == 1 {
                    if let Some(v) = state.stack.pop() {
                        state.regs.insert(dest_id64 as u16, v);
                    } else {
                        state.warnings.push(format!(
                            "warning: POP on empty stack\n  --> pc: 0x{:X}",
                            state.rip
                        ));
                    }
                }
            }
            x if x == Op::Enter as u8 => {
                let size = get_operand(code, &mut pc, &mut state);
                let name_to_id = RegisterMap::build_name_to_id();
                let rbp = *name_to_id.get("RBP").unwrap_or(&0);
                let rsp = *name_to_id.get("RSP").unwrap_or(&0);
                let cur_rbp = *state.regs.get(&rbp).unwrap_or(&0);
                state.stack.push(cur_rbp);
                let cur_rsp = *state.regs.get(&rsp).unwrap_or(&(state.stack.len() as u64));
                state.regs.insert(rbp, cur_rsp);
                state.regs.insert(rsp, cur_rsp.wrapping_add(size));
            }
            x if x == Op::Leave as u8 => {
                let name_to_id = RegisterMap::build_name_to_id();
                let rbp = *name_to_id.get("RBP").unwrap_or(&0);
                let rsp = *name_to_id.get("RSP").unwrap_or(&0);
                let frame_top = *state.regs.get(&rbp).unwrap_or(&0);
                state.regs.insert(rsp, frame_top);
                if let Some(v) = state.stack.pop() {
                    state.regs.insert(rbp, v);
                }
            }
            x if x == Op::Mni as u8 => {
                // Read module/function as raw (mode,val) pairs (addresses expected when mode=3)
                let m_mode = code[pc];
                pc += 1;
                let m_val = read_u64_le(code, &mut pc);
                let f_mode = code[pc];
                pc += 1;
                let f_val = read_u64_le(code, &mut pc);
                let argc = read_u16_le(code, &mut pc) as usize;
                let mut argv: Vec<String> = Vec::new();
                for _ in 0..argc {
                    let mode = code[pc];
                    pc += 1;
                    let val = read_u64_le(code, &mut pc);
                    match mode {
                        0 => argv.push(format!("{}", val)),
                        1 => {
                            let id = val as u16;
                            let name = RegisterMap::build_id_to_name()
                                .remove(&id)
                                .unwrap_or_else(|| format!("REG{}", id));
                            argv.push(name);
                        }
                        3 => {
                            if let Some(s) = read_c_string(val, &state) {
                                argv.push(s);
                            } else {
                                argv.push(format!("$0x{:X}", val));
                            }
                        }
                        4 => {
                            let id = val as u16;
                            let name = RegisterMap::build_id_to_name()
                                .remove(&id)
                                .unwrap_or_else(|| format!("REG{}", id));
                            argv.push(format!("${}", name));
                        }
                        _ => argv.push(format!("{}", val)),
                    }
                }
                // Extra debug: show raw m_mode/f_mode and values
                debug_println!("[DEBUG-MNI-Raw] m_mode={} m_val=0x{:X} f_mode={} f_val=0x{:X}", m_mode, m_val, f_mode, f_val);
                if m_mode == 3 {
                    if let Some(s) = read_c_string(m_val, &state) { debug_println!("[DEBUG-MNI-Raw] module-cstr='{}'", s); } else { debug_println!("[DEBUG-MNI-Raw] module-cstr=<invalid ptr 0x{:X}>", m_val); }
                }
                if f_mode == 3 {
                    if let Some(s) = read_c_string(f_val, &state) { debug_println!("[DEBUG-MNI-Raw] function-cstr='{}'", s); } else { debug_println!("[DEBUG-MNI-Raw] function-cstr=<invalid ptr 0x{:X}>", f_val); }
                }
                // Decode module/function names
                let mod_name = match m_mode { 3 => read_c_string(m_val, &state), 1 => Some(RegisterMap::build_id_to_name().remove(&(m_val as u16)).unwrap_or_else(|| format!("REG{}", m_val as u16))), 0 => Some(format!("{}", m_val)), 4 => Some(format!("${}", RegisterMap::build_id_to_name().remove(&(m_val as u16)).unwrap_or_else(|| format!("REG{}", m_val as u16)))), _ => None };
                let fn_name  = match f_mode { 3 => read_c_string(f_val, &state), 1 => Some(RegisterMap::build_id_to_name().remove(&(f_val as u16)).unwrap_or_else(|| format!("REG{}", f_val as u16))), 0 => Some(format!("{}", f_val)), 4 => Some(format!("${}", RegisterMap::build_id_to_name().remove(&(f_val as u16)).unwrap_or_else(|| format!("REG{}", f_val as u16)))), _ => None };
                debug_println!("[DEBUG] MNI lookup: module={:?}, function={:?}", &mod_name, &fn_name);
                debug_println!("[DEBUG] Registered MNI functions (omitted in normal run)");
                if let (Some(mn), Some(fn_)) = (&mod_name, &fn_name) {
                    let mn_lc = mn.trim().to_lowercase();
                    let fn_lc = fn_.trim().to_lowercase();
                    if let Some(func) = registry.lookup(&mn_lc, &fn_lc) {
                        let mut ctx = MniCtx {
                            state: state.clone(),
                            args: argv,
                        };
                        func(&mut ctx);
                        state = ctx.state;
                        let rax_id = RegisterMap::build_name_to_id()
                            .get("RAX")
                            .copied()
                            .unwrap_or(0);
                        let rax_val = state.regs.get(&rax_id).copied().unwrap_or(0);
                        debug_println!("[DEBUG] RAX after MNI: {}", rax_val);
                    } else {
                        // Try dynamic library loading via libloading
                        let module_raw = mn.trim();
                        let func_raw = fn_.trim();
                        match registry.load_dyn_lib(module_raw) {
                            Ok(lib) => {
                                unsafe {
                                    // Try exact symbol, then with mni_ prefix, try lowercased variants
                                    let mut sym_err: Option<String> = None;
                                    let mut call_res: Option<i64> = None;
                                    type MniC = unsafe extern "C" fn(
                                        argc: i32,
                                        argv: *const *const c_char,
                                    )
                                        -> i64;
                                    type MniCWithCtx = unsafe extern "C" fn(
                                        ctx: *mut MniVmCtx,
                                        argc: i32,
                                        argv: *const *const c_char,
                                    )
                                        -> i64;
                                    let try_symbol = |name: &str| -> Result<MniC, String> {
                                        match lib.get::<MniC>(name.as_bytes()) {
                                            Ok(sym) => Ok(*sym),
                                            Err(e) => Err(e.to_string()),
                                        }
                                    };
                                    let try_symbol_ctx =
                                        |name: &str| -> Result<MniCWithCtx, String> {
                                            match lib.get::<MniCWithCtx>(name.as_bytes()) {
                                                Ok(sym) => Ok(*sym),
                                                Err(e) => Err(e.to_string()),
                                            }
                                        };
                                    let candidates = vec![
                                        func_raw.to_string(),
                                        format!("mni_{}", func_raw),
                                        func_raw.to_lowercase(),
                                        format!("mni_{}", func_raw.to_lowercase()),
                                    ];
                                    // Build argv once
                                    let c_args: Vec<CString> = argv
                                        .iter()
                                        .map(|s| {
                                            CString::new(s.as_str())
                                                .unwrap_or_else(|_| CString::new("\u{0}").unwrap())
                                        })
                                        .collect();
                                    let mut ptrs: Vec<*const c_char> =
                                        c_args.iter().map(|s| s.as_ptr()).collect();
                                    ptrs.push(std::ptr::null());
                                    // Prepare VM context
                                    // Do not expose raw memory pointer; native modules should call host_read_u64/host_write_u64
                                    let mut vm_ctx = MniVmCtx {
                                        api_version: 1,
                                        user_data: (&mut state as *mut State).cast(),
                                        memory_ptr: std::ptr::null_mut(),
                                        memory_len: 0,
                                        read_u64: host_read_u64,
                                        write_u64: host_write_u64,
                                        get_reg_by_name: host_get_reg_by_name,
                                        set_reg_by_name: host_set_reg_by_name,
                                    };
                                    for cname in candidates.iter() {
                                        // Prefer ctx variant first ("name_ctx")
                                        let ctx_variants =
                                            [format!("{}_ctx", cname), format!("{}_CTX", cname)];
                                        let mut used = false;
                                        for v in ctx_variants.iter() {
                                            if let Ok(fctx) = try_symbol_ctx(v) {
                                                let ret = fctx(
                                                    &mut vm_ctx as *mut MniVmCtx,
                                                    c_args.len() as i32,
                                                    ptrs.as_ptr(),
                                                );
                                                call_res = Some(ret);
                                                used = true;
                                                break;
                                            }
                                        }
                                        if used {
                                            break;
                                        }
                                        match try_symbol(cname) {
                                            Ok(f) => {
                                                let ret = f(c_args.len() as i32, ptrs.as_ptr());
                                                call_res = Some(ret);
                                                break;
                                            }
                                            Err(e) => {
                                                sym_err = Some(e);
                                            }
                                        }
                                    }
                                    if let Some(val) = call_res {
                                        // Place return value into RAX
                                        let rax_id = RegisterMap::build_name_to_id()
                                            .get("RAX")
                                            .copied()
                                            .unwrap_or(0);
                                        state.regs.insert(rax_id, val as u64);
                                        // sync memory pointer/len in case C mutated memory via ctx
                                        // (state already updated by host_write_u64 when called)
                                    } else {
                                        let msg = format!(
                                            "MNI: dynamic function not found in module '{}': {} (last error: {:?})",
                                            module_raw, func_raw, sym_err
                                        );
                                        state.errors.push(msg.clone());
                                        return Err(msg);
                                    }
                                }
                            }
                            Err(e) => {
                                let msg = format!(
                                    "MNI: function not found: {}.{} (dynamic load error: {})",
                                    mn, fn_, e
                                );
                                state.errors.push(msg.clone());
                                return Err(msg);
                            }
                        }
                    }
                } else {
                    // Debug: module/function decoding failed — show some memory & registers
                    let dbg_m = mod_name.clone();
                    let dbg_f = fn_name.clone();
                    debug_println!("[DEBUG-MNI-FAIL] pc=0x{:X} mod_name={:?} fn_name={:?}", state.rip, dbg_m, dbg_f);
                    {
                        let mem = state.memory.lock().unwrap();
                        let len = std::cmp::min(128, mem.len());
                        let hex = mem[0..len].iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ");
                        debug_println!("[DEBUG-MNI-FAIL] mem[0..{}]: {}", len, hex);
                    }
                    let idmap = RegisterMap::build_name_to_id();
                    for rname in ["RAX","RBP","RSP","R1","R2","R3","R4"].iter() {
                        if let Some(id) = idmap.get(*rname) {
                            let v = *state.regs.get(id).unwrap_or(&0);
                            debug_println!("[DEBUG-MNI-FAIL] {} = 0x{:X}", rname, v);
                        }
                    }
                    let msg = "MNI: module or function decoding failed".to_string();
                    state.errors.push(msg.clone());
                    return Err(msg);
                }
            }
            x if x == Op::Syscall as u8 => {
                // Minimal syscall emulation (runtime-level, not host OS):
                // RAX: number; args RDI, RSI, RDX, R10, R8, R9. Return in RAX.
                let nm = RegisterMap::build_name_to_id();
                let rax = *nm.get("RAX").unwrap_or(&0);
                let rdi = *nm.get("RDI").unwrap_or(&0);
                let rsi = *nm.get("RSI").unwrap_or(&0);
                let rdx = *nm.get("RDX").unwrap_or(&0);
                let r10 = *nm.get("R10").unwrap_or(&0);
                let r8 = *nm.get("R8").unwrap_or(&0);
                let r9 = *nm.get("R9").unwrap_or(&0);
                let num = *state.regs.get(&rax).unwrap_or(&0);
                let a1 = *state.regs.get(&rdi).unwrap_or(&0);
                let a2 = *state.regs.get(&rsi).unwrap_or(&0);
                let a3 = *state.regs.get(&rdx).unwrap_or(&0);
                let _a4 = *state.regs.get(&r10).unwrap_or(&0);
                let _a5 = *state.regs.get(&r8).unwrap_or(&0);
                let _a6 = *state.regs.get(&r9).unwrap_or(&0);
                match num {
                    60 => {
                        // exit(code)
                        return Ok(state);
                    }
                    1 => {
                        // write(fd, buf, count)
                        let fd = a1;
                        let buf = a2 as usize;
                        let cnt = a3 as usize;
                        // copy bytes out of locked memory then write without holding lock
                        let slice_vec = {
                            let mem = state.memory.lock().unwrap();
                            let end = buf.saturating_add(cnt).min(mem.len());
                            if buf < end {
                                mem[buf..end].to_vec()
                            } else {
                                Vec::new()
                            }
                        };
                        if fd == 1 {
                            let _ = out.write_all(&slice_vec);
                            let _ = out.flush();
                        } else if fd == 2 {
                            let _ = err.write_all(&slice_vec);
                            let _ = err.flush();
                        }
                        state.regs.insert(rax, slice_vec.len() as u64);
                    }
                    0 => {
                        // read(fd, buf, count) - only stdin supported
                        let fd = a1;
                        let buf = a2 as usize;
                        let cnt = a3 as usize;
                        if fd == 0 {
                            let mut s = String::new();
                            match input.as_deref_mut() {
                                Some(r) => {
                                    let _ = r.read_line(&mut s);
                                }
                                None => {
                                    let _ = io::stdin().read_line(&mut s);
                                }
                            }
                            let mut bytes = s.into_bytes();
                            if bytes.len() > cnt {
                                bytes.truncate(cnt);
                            }
                            // Mirror IN semantics: append a NUL terminator and auto-resize memory to fit
                            bytes.push(0);
                            {
                                let mut mem = state.memory.lock().unwrap();
                                if mem.len() < buf + bytes.len() {
                                    mem.resize(buf + bytes.len(), 0);
                                }
                                mem[buf..buf + bytes.len()].copy_from_slice(&bytes);
                            }
                            // Return number of bytes read (excluding NUL terminator)
                            let reported = if bytes.len() == 0 { 0 } else { bytes.len() - 1 };
                            state.regs.insert(rax, reported as u64);
                        } else {
                            state.regs.insert(rax, 0);
                        }
                    }
                    _ => {
                        // Unimplemented syscalls: return 0
                        state.regs.insert(rax, 0);
                    }
                }
            }
            x if x == Op::Out as u8 => {
                // OUT port value: if value is a memory address (mode 3 or 4), print string; otherwise print numeric.
                // Resolve port to a numeric value (supports immediate, register, code-addr, and mem-indirect forms).
                let port_mode = code[pc];
                pc += 1;
                let port_val = read_u64_le(code, &mut pc);
                let val_mode = code[pc];
                pc += 1;
                let val_val = read_u64_le(code, &mut pc);
                let resolve_port = |mode: u8, val: u64, st: &State| -> u64 {
                    match mode {
                        0 | 2 => val,
                        1 => {
                            let id = val as u16;
                            *st.regs.get(&id).unwrap_or(&0)
                        }
                        3 => read_u64_from_memory(val, st),
                        4 => {
                            let id = val as u16;
                            let addr = *st.regs.get(&id).unwrap_or(&0);
                            read_u64_from_memory(addr, st)
                        }
                        _ => val,
                    }
                };
                let port_num = resolve_port(port_mode, port_val, &state);
                let to_err = port_num == 2;
                if port_num != 1 && port_num != 2 {
                    state.warnings.push(format!(
                        "warning: unknown OUT port {} (defaulting to stdout)\n  --> pc: 0x{:X}",
                        port_num, state.rip
                    ));
                }
                let w: &mut dyn Write = if to_err {
                    err as &mut dyn Write
                } else {
                    out as &mut dyn Write
                };
                match val_mode {
                    3 => {
                        if let Some(s) = read_c_string(val_val, &state) {
                            let _ = writeln!(w, "{}", s);
                        } else {
                            state.warnings.push(format!(
                                "warning: OUT read invalid string address $0x{:X}\n  --> pc: 0x{:X}",
                                val_val, state.rip
                            ));
                            let _ = writeln!(w, "{}", val_val);
                        }
                    }
                    4 => {
                        // Register-indirect: val_val encodes a register id; fetch address from the register first
                        let addr = state.regs.get(&(val_val as u16)).copied().unwrap_or(0);
                        if let Some(s) = read_c_string(addr, &state) {
                            let _ = writeln!(w, "{}", s);
                        } else {
                            state.warnings.push(format!(
                                "warning: OUT read invalid string address $0x{:X}\n  --> pc: 0x{:X}",
                                addr, state.rip
                            ));
                            let _ = writeln!(w, "{}", addr);
                        }
                    }
                    1 => {
                        let reg_val = state.regs.get(&(val_val as u16)).copied().unwrap_or(0);
                        let _ = writeln!(w, "{}", reg_val);
                    }
                    _ => {
                        let _ = writeln!(w, "{}", val_val);
                    }
                }
            }
            x if x == Op::COut as u8 => {
                // Print a single character or a C-string (no trailing newline).
                // Resolve port and value using get_operand (supports immediates/regs/mem-addressing via modes inside get_operand paths).
                let p = get_operand(code, &mut pc, &mut state);
                let v = get_operand(code, &mut pc, &mut state);
                let to_err = p == 2;
                if p != 1 && p != 2 {
                    state.warnings.push(format!(
                        "warning: unknown COUT port {} (defaulting to stdout)\n  --> pc: 0x{:X}",
                        p, state.rip
                    ));
                }
                let w: &mut dyn Write = if to_err {
                    err as &mut dyn Write
                } else {
                    out as &mut dyn Write
                };
                // Try to read a C-string at address v. If present, print it; else print a single byte.
                if let Some(s) = read_c_string(v, &state) {
                    let _ = write!(w, "{}", s);
                    let _ = w.flush();
                } else {
                    let ch: u8 = {
                        let mem = state.memory.lock().unwrap();
                        if (v as usize) < mem.len() {
                            mem[v as usize]
                        } else {
                            v as u8
                        }
                    };
                    {
                        let mem_len = state.memory.lock().unwrap().len();
                        if (v as usize) >= mem_len {
                            state.warnings.push(format!(
                                "warning: COUT read out-of-bounds at 0x{:X}\n  --> pc: 0x{:X}",
                                v, state.rip
                            ));
                        }
                    }
                    if to_err {
                        let _ = err.write_all(&[ch]);
                        let _ = err.flush();
                    } else {
                        let _ = out.write_all(&[ch]);
                        let _ = out.flush();
                    }
                }
            }
            x if x == Op::In as u8 => {
                let dest_addr = get_operand(code, &mut pc, &mut state) as usize;
                let mut line = String::new();
                match input.as_deref_mut() {
                    Some(reader) => {
                        let _ = reader.read_line(&mut line);
                    }
                    None => {
                        let _ = io::stdin().read_line(&mut line);
                    }
                }
                let mut bytes = line.into_bytes();
                bytes.push(0);
                {
                    let mut mem = state.memory.lock().unwrap();
                    if dest_addr + bytes.len() > mem.len() {
                        let old = mem.len();
                        mem.resize(dest_addr + bytes.len(), 0);
                        state.warnings.push(format!(
                            "warning: IN extended memory from {} to {}\n  --> pc: 0x{:X}",
                            old,
                            mem.len(),
                            state.rip
                        ));
                    }
                    mem[dest_addr..dest_addr + bytes.len()].copy_from_slice(&bytes);
                }
            }
            _ => {}
        }
        // Safety check: if PC moves past end of code unexpectedly, surface an error
        if pc > code.len() {
            let msg = format!(
                "error[EVM001]: program counter moved past end of code\n  --> pc: 0x{:X}\n   = note: attempted to execute at 0x{:X}, code size {}",
                state.rip,
                pc,
                code.len()
            );
            state.errors.push(msg.clone());
            return Err(msg);
        }
    }
    Ok(state)
}

// ---------------- Simple CLI Debugger ----------------

fn opcode_name(op: u8) -> &'static str {
    match op {
        0x00 => "NOP",
        0x01 => "MOV",
        0x02 => "ADD",
        0x03 => "SUB",
        0x10 => "JMP",
        0x11 => "CMP",
        0x12 => "JE",
        0x13 => "JNE",
        0x14 => "JL",
        0x15 => "JLE",
        0x16 => "JG",
        0x17 => "JGE",
        0x20 => "CALL",
        0x21 => "RET",
        0x30 => "PUSH",
        0x31 => "POP",
        0x40 => "OUT",
        0x41 => "COUT",
        0x42 => "IN",
        0x50 => "ENTER",
        0x51 => "LEAVE",
        0x60 => "MNI",
        0x70 => "FMOV",
        0x71 => "FADD",
        0x72 => "FSUB",
        0x73 => "FMUL",
        0x74 => "FDIV",
        0x75 => "FCMP",
        0x76 => "FJE",
        0x77 => "FJNE",
        0x78 => "FJLT",
        0x79 => "FJLE",
        0x7A => "FJGT",
        0x7B => "FJGE",
        0x7C => "FJUO",
        0xFF => "HLT",
        _ => "?",
    }
}

pub struct CliDebugger {
    breakpoints: HashSet<usize>,
    running: bool,
}

impl CliDebugger {
    pub fn new() -> Self {
        Self {
            breakpoints: HashSet::new(),
            running: false,
        }
    }

    fn parse_addr(&self, masi: &MASIFile, tok: &str) -> Option<usize> {
        if tok.starts_with("0x") {
            usize::from_str_radix(&tok[2..], 16).ok()
        } else if tok.chars().all(|c| c.is_ascii_digit()) {
            tok.parse::<usize>().ok()
        } else {
            // try label name
            for (off, name) in masi.label_map.iter() {
                if name == tok {
                    return Some(*off as usize);
                }
            }
            None
        }
    }

    fn print_regs(&self, state: &State) {
        let rev = RegisterMap::build_id_to_name();
        let mut ids: Vec<_> = rev.keys().copied().collect();
        ids.sort();
        let mut line = String::new();
        for id in ids {
            let name = rev.get(&id).unwrap();
            let val = state.regs.get(&id).copied().unwrap_or(0);
            line.push_str(&format!("{}=0x{:016X} ", name, val));
        }
        println!("{}", line.trim());
        let (zf, sf, cf, of) = state.flags;
        println!("FLAGS: ZF={} SF={} CF={} OF={}", zf, sf, cf, of);
        println!("RIP=0x{:016X}", state.rip);
    }

    fn print_mem(&self, state: &State, addr: usize, len: usize) {
        let mem = state.memory.lock().unwrap();
        let end = addr.saturating_add(len).min(mem.len());
        let mut i = addr;
        while i < end {
            print!("{:08X}: ", i);
            for j in 0..16 {
                if i + j < end {
                    print!("{:02X} ", mem[i + j]);
                } else {
                    print!("   ");
                }
            }
            print!(" | ");
            for j in 0..16 {
                if i + j < end {
                    let b = mem[i + j];
                    let c = if (32..=126).contains(&b) {
                        b as char
                    } else {
                        '.'
                    };
                    print!("{}", c);
                }
            }
            println!();
            i = i.saturating_add(16);
        }
    }

    fn prompt(&self, masi: &MASIFile, _state: &State, pc: usize, opcode: u8) {
        let where_s = if let Some(name) = masi.label_map.get(&(pc as u64)) {
            format!("{} (0x{:X})", name, pc)
        } else {
            format!("0x{:X}", pc)
        };
        println!(
            "stopped at {}: {} (0x{:02X})",
            where_s,
            opcode_name(opcode),
            opcode
        );
    }

    fn fmt_op(&self, masi: &MASIFile, mode: u8, value: u64) -> String {
        let reg_rev = RegisterMap::build_id_to_name();
        match mode {
            0 => {
                if let Some(name) = masi.data_label_map.get(&value) {
                    return format!("${}", name);
                }
                format!("{}", value)
            }
            1 => {
                let id = value as u16;
                if let Some(n) = reg_rev.get(&id) {
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
                if let Some(n) = reg_rev.get(&id) {
                    format!("${}", n)
                } else {
                    format!("$REG{}", id)
                }
            }
            _ => format!("{}", value),
        }
    }

    fn read_op_pair<'a>(&self, code: &'a [u8], pc: &mut usize) -> Option<(u8, u64)> {
        if *pc >= code.len() {
            return None;
        }
        let mode = code[*pc];
        *pc += 1;
        if *pc + 8 > code.len() {
            return None;
        }
        let val = read_u64_le(code, pc);
        Some((mode, val))
    }

    fn disassemble_from(
        &self,
        masi: &MASIFile,
        start_pc: usize,
        count: usize,
    ) -> Vec<(usize, String)> {
        let mut lines = Vec::new();
        let mut pc = start_pc;
        let code = &masi.code;
        let mut n = 0;
        while pc < code.len() && n < count {
            let this_pc = pc;
            let opcode = code[pc];
            pc += 1;
            let mut text = String::new();
            match opcode {
                0x01 => {
                    let d = self.read_op_pair(code, &mut pc);
                    let s = self.read_op_pair(code, &mut pc);
                    if let (Some(d), Some(s)) = (d, s) {
                        text = format!(
                            "MOV {} {}",
                            self.fmt_op(masi, d.0, d.1),
                            self.fmt_op(masi, s.0, s.1)
                        );
                    }
                }
                0x02 => {
                    let d = self.read_op_pair(code, &mut pc);
                    let s = self.read_op_pair(code, &mut pc);
                    if let (Some(d), Some(s)) = (d, s) {
                        text = format!(
                            "ADD {} {}",
                            self.fmt_op(masi, d.0, d.1),
                            self.fmt_op(masi, s.0, s.1)
                        );
                    }
                }
                0x03 => {
                    let d = self.read_op_pair(code, &mut pc);
                    let s = self.read_op_pair(code, &mut pc);
                    if let (Some(d), Some(s)) = (d, s) {
                        text = format!(
                            "SUB {} {}",
                            self.fmt_op(masi, d.0, d.1),
                            self.fmt_op(masi, s.0, s.1)
                        );
                    }
                }
                0x70 => {
                    let d = self.read_op_pair(code, &mut pc);
                    let s = self.read_op_pair(code, &mut pc);
                    if let (Some(d), Some(s)) = (d, s) {
                        text = format!(
                            "FMOV {} {}",
                            self.fmt_op(masi, d.0, d.1),
                            self.fmt_op(masi, s.0, s.1)
                        );
                    }
                }
                0x71 => {
                    let d = self.read_op_pair(code, &mut pc);
                    let s = self.read_op_pair(code, &mut pc);
                    if let (Some(d), Some(s)) = (d, s) {
                        text = format!(
                            "FADD {} {}",
                            self.fmt_op(masi, d.0, d.1),
                            self.fmt_op(masi, s.0, s.1)
                        );
                    }
                }
                0x72 => {
                    let d = self.read_op_pair(code, &mut pc);
                    let s = self.read_op_pair(code, &mut pc);
                    if let (Some(d), Some(s)) = (d, s) {
                        text = format!(
                            "FSUB {} {}",
                            self.fmt_op(masi, d.0, d.1),
                            self.fmt_op(masi, s.0, s.1)
                        );
                    }
                }
                0x73 => {
                    let d = self.read_op_pair(code, &mut pc);
                    let s = self.read_op_pair(code, &mut pc);
                    if let (Some(d), Some(s)) = (d, s) {
                        text = format!(
                            "FMUL {} {}",
                            self.fmt_op(masi, d.0, d.1),
                            self.fmt_op(masi, s.0, s.1)
                        );
                    }
                }
                0x74 => {
                    let d = self.read_op_pair(code, &mut pc);
                    let s = self.read_op_pair(code, &mut pc);
                    if let (Some(d), Some(s)) = (d, s) {
                        text = format!(
                            "FDIV {} {}",
                            self.fmt_op(masi, d.0, d.1),
                            self.fmt_op(masi, s.0, s.1)
                        );
                    }
                }
                0x75 => {
                    let a = self.read_op_pair(code, &mut pc);
                    let b = self.read_op_pair(code, &mut pc);
                    if let (Some(a), Some(b)) = (a, b) {
                        text = format!(
                            "FCMP {} {}",
                            self.fmt_op(masi, a.0, a.1),
                            self.fmt_op(masi, b.0, b.1)
                        );
                    }
                }
                0x76 => {
                    let t = self.read_op_pair(code, &mut pc);
                    if let Some(t) = t {
                        text = format!("FJE {}", self.fmt_op(masi, t.0, t.1));
                    }
                }
                0x77 => {
                    let t = self.read_op_pair(code, &mut pc);
                    if let Some(t) = t {
                        text = format!("FJNE {}", self.fmt_op(masi, t.0, t.1));
                    }
                }
                0x78 => {
                    let t = self.read_op_pair(code, &mut pc);
                    if let Some(t) = t {
                        text = format!("FJLT {}", self.fmt_op(masi, t.0, t.1));
                    }
                }
                0x79 => {
                    let t = self.read_op_pair(code, &mut pc);
                    if let Some(t) = t {
                        text = format!("FJLE {}", self.fmt_op(masi, t.0, t.1));
                    }
                }
                0x7A => {
                    let t = self.read_op_pair(code, &mut pc);
                    if let Some(t) = t {
                        text = format!("FJGT {}", self.fmt_op(masi, t.0, t.1));
                    }
                }
                0x7B => {
                    let t = self.read_op_pair(code, &mut pc);
                    if let Some(t) = t {
                        text = format!("FJGE {}", self.fmt_op(masi, t.0, t.1));
                    }
                }
                0x7C => {
                    let t = self.read_op_pair(code, &mut pc);
                    if let Some(t) = t {
                        text = format!("FJUO {}", self.fmt_op(masi, t.0, t.1));
                    }
                }
                0x10 => {
                    let t = self.read_op_pair(code, &mut pc);
                    if let Some(t) = t {
                        text = format!("JMP {}", self.fmt_op(masi, t.0, t.1));
                    }
                }
                0x11 => {
                    let a = self.read_op_pair(code, &mut pc);
                    let b = self.read_op_pair(code, &mut pc);
                    if let (Some(a), Some(b)) = (a, b) {
                        text = format!(
                            "CMP {} {}",
                            self.fmt_op(masi, a.0, a.1),
                            self.fmt_op(masi, b.0, b.1)
                        );
                    }
                }
                0x12 => {
                    let t = self.read_op_pair(code, &mut pc);
                    if let Some(t) = t {
                        text = format!("JE {}", self.fmt_op(masi, t.0, t.1));
                    }
                }
                0x13 => {
                    let t = self.read_op_pair(code, &mut pc);
                    if let Some(t) = t {
                        text = format!("JNE {}", self.fmt_op(masi, t.0, t.1));
                    }
                }
                0x14 => {
                    let t = self.read_op_pair(code, &mut pc);
                    if let Some(t) = t {
                        text = format!("JL {}", self.fmt_op(masi, t.0, t.1));
                    }
                }
                0x15 => {
                    let t = self.read_op_pair(code, &mut pc);
                    if let Some(t) = t {
                        text = format!("JLE {}", self.fmt_op(masi, t.0, t.1));
                    }
                }
                0x16 => {
                    let t = self.read_op_pair(code, &mut pc);
                    if let Some(t) = t {
                        text = format!("JG {}", self.fmt_op(masi, t.0, t.1));
                    }
                }
                0x17 => {
                    let t = self.read_op_pair(code, &mut pc);
                    if let Some(t) = t {
                        text = format!("JGE {}", self.fmt_op(masi, t.0, t.1));
                    }
                }
                0x20 => {
                    let t = self.read_op_pair(code, &mut pc);
                    if let Some(t) = t {
                        text = format!("CALL {}", self.fmt_op(masi, t.0, t.1));
                    }
                }
                0x21 => {
                    text = "RET".into();
                }
                0x30 => {
                    let v = self.read_op_pair(code, &mut pc);
                    if let Some(v) = v {
                        text = format!("PUSH {}", self.fmt_op(masi, v.0, v.1));
                    }
                }
                0x31 => {
                    let d = self.read_op_pair(code, &mut pc);
                    if let Some(d) = d {
                        text = format!("POP {}", self.fmt_op(masi, d.0, d.1));
                    }
                }
                0x40 => {
                    let p = self.read_op_pair(code, &mut pc);
                    let v = self.read_op_pair(code, &mut pc);
                    if let (Some(p), Some(v)) = (p, v) {
                        text = format!(
                            "OUT {} {}",
                            self.fmt_op(masi, p.0, p.1),
                            self.fmt_op(masi, v.0, v.1)
                        );
                    }
                }
                0x41 => {
                    let p = self.read_op_pair(code, &mut pc);
                    let v = self.read_op_pair(code, &mut pc);
                    if let (Some(p), Some(v)) = (p, v) {
                        text = format!(
                            "COUT {} {}",
                            self.fmt_op(masi, p.0, p.1),
                            self.fmt_op(masi, v.0, v.1)
                        );
                    }
                }
                0x42 => {
                    let d = self.read_op_pair(code, &mut pc);
                    if let Some(d) = d {
                        text = format!("IN {}", self.fmt_op(masi, d.0, d.1));
                    }
                }
                0x50 => {
                    let s = self.read_op_pair(code, &mut pc);
                    if let Some(s) = s {
                        text = format!("ENTER {}", self.fmt_op(masi, s.0, s.1));
                    }
                }
                0x51 => {
                    text = "LEAVE".into();
                }
                0x60 => {
                    let m = self.read_op_pair(code, &mut pc);
                    let f = self.read_op_pair(code, &mut pc);
                    if let (Some(m), Some(f)) = (m, f) {
                        if pc + 2 > code.len() {
                            break;
                        }
                        let mut off = pc;
                        let argc = read_u16_le(code, &mut off) as usize;
                        pc = off;
                        let mut line = format!(
                            "MNI {} {}",
                            self.fmt_op(masi, m.0, m.1),
                            self.fmt_op(masi, f.0, f.1)
                        );
                        for _ in 0..argc {
                            if let Some(a) = self.read_op_pair(code, &mut pc) {
                                line.push(' ');
                                line.push_str(&self.fmt_op(masi, a.0, a.1));
                            }
                        }
                        text = line;
                    }
                }
                0x00 => {
                    text = "NOP".into();
                }
                0xFF => {
                    text = "HLT".into();
                }
                _ => {
                    text = format!("; DB 0x{:02X}", opcode);
                }
            }
            let label = masi.label_map.get(&(this_pc as u64)).cloned();
            let full = if let Some(lbl) = label {
                format!("{:08X}: {:<8} {}", this_pc, lbl, text)
            } else {
                format!("{:08X}: {}", this_pc, text)
            };
            lines.push((this_pc, full));
            n += 1;
        }
        lines
    }
}

impl Debugger for CliDebugger {
    fn before_execute(
        &mut self,
        masi: &MASIFile,
        state: &State,
        pc: usize,
        opcode: u8,
    ) -> DebuggerControl {
        if self.running && !self.breakpoints.contains(&pc) {
            return DebuggerControl::Continue;
        }
        self.prompt(masi, state, pc, opcode);
        // simple REPL
        let stdin = io::stdin();
        loop {
            print!("(masmdbg) ");
            let _ = io::stdout().flush();
            let mut line = String::new();
            if stdin.read_line(&mut line).is_err() {
                return DebuggerControl::Exit;
            }
            let line = line.trim();
            if line.is_empty() {
                // default: step
                self.running = false;
                return DebuggerControl::Continue;
            }
            let mut parts = line.split_whitespace();
            let cmd = parts.next().unwrap_or("");
            match cmd {
                "c" | "cont" | "continue" => {
                    self.running = true;
                    return DebuggerControl::Continue;
                }
                "s" | "si" | "step" => {
                    self.running = false;
                    return DebuggerControl::Continue;
                }
                "q" | "quit" | "exit" => {
                    return DebuggerControl::Exit;
                }
                "b" | "break" => {
                    if let Some(arg) = parts.next() {
                        if let Some(addr) = self.parse_addr(masi, arg) {
                            self.breakpoints.insert(addr);
                            println!("Breakpoint set at 0x{:X}", addr);
                        } else {
                            println!("Unknown address/label: {}", arg);
                        }
                    } else {
                        println!("usage: break <addr|label>");
                    }
                }
                "db" | "del" | "delete" => {
                    if let Some(arg) = parts.next() {
                        if let Some(addr) = self.parse_addr(masi, arg) {
                            self.breakpoints.remove(&addr);
                            println!("Deleted breakpoint at 0x{:X}", addr);
                        } else {
                            println!("Unknown address/label: {}", arg);
                        }
                    } else {
                        println!("usage: delete <addr|label>");
                    }
                }
                "info" => match parts.next().unwrap_or("") {
                    "b" | "break" | "breakpoints" => {
                        if self.breakpoints.is_empty() {
                            println!("(no breakpoints)");
                        } else {
                            let mut v: Vec<_> = self.breakpoints.iter().copied().collect();
                            v.sort();
                            for bp in v {
                                if let Some(name) = masi.label_map.get(&(bp as u64)) {
                                    println!("- {} (0x{:X})", name, bp);
                                } else {
                                    println!("- 0x{:X}", bp);
                                }
                            }
                        }
                    }
                    _ => println!("usage: info breakpoints"),
                },
                "regs" | "registers" => {
                    self.print_regs(state);
                }
                "x" => {
                    // examine memory: x <addr> [len]
                    if let Some(arg) = parts.next() {
                        if let Some(addr) = self.parse_addr(masi, arg) {
                            let len = parts
                                .next()
                                .and_then(|s| s.parse::<usize>().ok())
                                .unwrap_or(64);
                            self.print_mem(state, addr, len);
                        } else {
                            println!("Unknown address/label: {}", arg);
                        }
                    } else {
                        println!("usage: x <addr|label> [len]");
                    }
                }
                "help" | "h" => {
                    println!(
                        "Commands: step(s), continue(c), break(b) <addr|label>, delete(db) <addr|label>, info breakpoints, regs, x <addr> [len], disas [addr] [count], quit(q)"
                    );
                }
                "disas" | "u" => {
                    // disassemble: disas [addr] [count]
                    let addr = parts
                        .next()
                        .and_then(|s| self.parse_addr(masi, s))
                        .unwrap_or(pc);
                    let count = parts
                        .next()
                        .and_then(|s| s.parse::<usize>().ok())
                        .unwrap_or(10);
                    let lines = self.disassemble_from(masi, addr, count);
                    for (_p, l) in lines {
                        println!("{}", l);
                    }
                }
                _ => {
                    println!("unknown command: {} (type 'help')", cmd);
                }
            }
        }
    }
}
