import Foundation
#if canImport(PythonKit)
import PythonKit
#endif

public final class Interpreter {
    // MARK: - MNI plumbing (minimal manual registry)
    public struct MNICtx {
        public var state: State
        public var args: [String] = []
        public mutating func writeString(_ s: String) {
            if let data = (s + "\n").data(using: .utf8) {
                try? FileHandle.standardError.write(contentsOf: data)
            }
        }
    }
    public typealias MNIFunc = (inout MNICtx) -> Void
    public class ModuleRegistry {
        private var funcs: [String: [String: MNIFunc]] = [:]
        public func register(module: String, name: String, fn: @escaping MNIFunc) {
            var mod = funcs[module] ?? [:]
            mod[name] = fn
            funcs[module] = mod
        }
        public func lookup(module: String, name: String) -> MNIFunc? { funcs[module]?[name] }
    }
    private let registry = ModuleRegistry()
    private func loadModules(from path: String) {
        // PythonKit-based loader: scan modules/*.py and register functions from MNI_FUNCTIONS
        // Contract per .py file:
        //   MNI_MODULE: str  -> module name to register under
        //   MNI_FUNCTIONS: dict[str, callable] -> functions taking (args: list[str], regs: dict[str,int]) and
        //     returning either None, a string to print, or a dict with optional keys:
        //       { "out": str, "regs": { regName: int, ... } }
        #if canImport(PythonKit)
        let fm = FileManager.default
        guard let enumerator = fm.enumerator(atPath: path) else { return }
        // Ensure modules path is importable
        let sys = Python.import("sys")
        let pyPath = PythonObject(path)
        if !(Bool(pyPath.__in__(sys.path)) ?? false) {
            _ = sys.path.insert(0, path)
        }
        for case let file as String in enumerator {
            guard file.hasSuffix(".py") else { continue }
            let moduleName = (file as NSString).deletingPathExtension
            // Import Python module
            let pymod = Python.import(moduleName)
            let builtins = Python.import("builtins")
            var anyRegistered = false
            // 1) Class-based contract: find classes with MNI_MODULE and methods with __mni_export__
            for item in pymod.__dict__.items() {
                let obj = item[1]
                let isType = Bool(builtins.isinstance(obj, builtins.type)) ?? false
                if !isType { continue }
                let hasClassModule = Bool(Python.hasattr(obj, "MNI_MODULE")) ?? false
                if !hasClassModule { continue }
                let modName = String(obj.MNI_MODULE) ?? ""
                let instance = obj()
                for mitem in obj.__dict__.items() {
                    let attrName = String(mitem[0])
                    let funcObj = mitem[1]
                    let isCallable = Bool(builtins.callable(funcObj)) ?? false
                    if !isCallable { continue }
                    let bound = Python.getattr(instance, PythonObject(attrName))
                    let hasExport = (Bool(Python.hasattr(bound, "__mni_export__")) ?? false) || (Bool(Python.hasattr(funcObj, "__mni_export__")) ?? false)
                    if !hasExport { continue }
                    let exportName = (Bool(Python.hasattr(bound, "__mni_export__")) ?? false) ? (String(bound.__mni_export__) ?? "") : (String(funcObj.__mni_export__) ?? "")
                    registry.register(module: modName, name: exportName) { ctx in
                        // Build regs snapshot: name -> Int
                        var regsMap: [String: Int] = [:]
                        for (id, val) in ctx.state.regs {
                            let name = RegisterMap.name(for: id) ?? "REG\(id)"
                            regsMap[name] = Int(truncatingIfNeeded: val)
                        }
                        // Call Python bound method: bound(args, regs)
                        let result = bound(PythonObject(ctx.args), PythonObject(regsMap))
                        // Handle result (string/None/dict)
                        let isDict = Bool(builtins.isinstance(result, builtins.dict)) ?? false
                        if isDict {
                            let out = result.get("out", default: Python.None)
                            if !(Bool(out.is_none) ?? false) {
                                let s = String(out)
                                if !(s?.isEmpty ?? true) { Swift.print(s!) }
                            }
                            // Support memory store: {"store": {"addr": int |, "reg": name, "string": str | "bytes": [ints] }}
                            let store = result.get("store", default: Python.None)
                            let storeIsDict = Bool(builtins.isinstance(store, builtins.dict)) ?? false
                            if storeIsDict {
                                var destAddr: UInt64? = nil
                                // addr or reg
                                let addrVal = store.get("addr", default: Python.None)
                                if !(Bool(addrVal.is_none) ?? false) {
                                    if let addrStr = String(addrVal), let a = UInt64(addrStr) { destAddr = a }
                                } else {
                                    let regNameObj = store.get("reg", default: Python.None)
                                    if !(Bool(regNameObj.is_none) ?? false) {
                                        if let rname = String(regNameObj), let rid = RegisterMap.id(for: rname) {
                                            destAddr = ctx.state.regs[rid] ?? 0
                                        }
                                    }
                                }
                                // data: string or bytes
                                var payload: [UInt8]? = nil
                                let sObj = store.get("string", default: Python.None)
                                if !(Bool(sObj.is_none) ?? false) {
                                    let s = String(sObj)
                                    if let sUnwrapped = s, let d = sUnwrapped.data(using: .utf8) {
                                        payload = Array(d) + [0] // null-terminate for C-strings
                                    }
                                } else {
                                    let bObj = store.get("bytes", default: Python.None)
                                    let listType = builtins.list
                                    if Bool(builtins.isinstance(bObj, listType)) ?? false {
                                        var buf: [UInt8] = []
                                        for it in bObj {
                                            if let itStr = String(it), let u = UInt64(itStr) { buf.append(UInt8(truncatingIfNeeded: u)) }
                                        }
                                        payload = buf
                                    }
                                }
                                if let addr = destAddr, let bytes = payload {
                                    let base = Int(truncatingIfNeeded: addr)
                                    if base >= 0 {
                                        let needed = base + bytes.count
                                        if ctx.state.memory.count < needed { ctx.state.memory.append(contentsOf: Array(repeating: 0, count: needed - ctx.state.memory.count)) }
                                        for i in 0..<bytes.count { ctx.state.memory[base + i] = bytes[i] }
                                    }
                                }
                            }
                            let updates = result.get("regs", default: Python.None)
                            let updatesIsDict = Bool(builtins.isinstance(updates, builtins.dict)) ?? false
                            if updatesIsDict {
                                for u in updates.items() {
                                    let rname = String(u[0])
                                    if let rnameUnwrapped = rname, let rid = RegisterMap.id(for: rnameUnwrapped) {
                                        let pyVal = u[1]
                                        if let pyValStr = String(pyVal), let ival = Int64(pyValStr) { ctx.state.regs[rid] = UInt64(bitPattern: ival) }
                                        else if let pyValStr = String(pyVal), let uval = UInt64(pyValStr) { ctx.state.regs[rid] = uval }
                                    }
                                }
                            }
                        } else {
                            let none = builtins.None
                            if !(Bool(result == none) ?? false) {
                                let s = String(result)
                                if !(s?.isEmpty ?? true) { Swift.print(s!) }
                            }
                        }
                    }
                    anyRegistered = true
                }
            }
            if anyRegistered { continue }
            // 2) Fallback: dict-based contract (MNI_MODULE + MNI_FUNCTIONS)
            let hasModule = Bool(Python.hasattr(pymod, "MNI_MODULE")) ?? false
            let hasFuncs  = Bool(Python.hasattr(pymod, "MNI_FUNCTIONS")) ?? false
            if hasModule && hasFuncs {
                let modName = String(pymod.MNI_MODULE) ?? ""
                let funcs = pymod.MNI_FUNCTIONS
                for pair in funcs.items() {
                    let fname = String(pair[0]) ?? ""
                    let fn = pair[1]
                    registry.register(module: modName, name: fname) { ctx in
                        var regsMap: [String: Int] = [:]
                        for (id, val) in ctx.state.regs {
                            let name = RegisterMap.name(for: id) ?? "REG\(id)"
                            regsMap[name] = Int(truncatingIfNeeded: val)
                        }
                        let result = fn(PythonObject(ctx.args), PythonObject(regsMap))
                        let isDict = Bool(builtins.isinstance(result, builtins.dict)) ?? false
                        if isDict {
                            let out = result.get("out", default: Python.None)
                            if !(Bool(out.is_none) ?? false) {
                                let s = String(out)
                                 if !(s?.isEmpty ?? true) { Swift.print(s!) }
                            }
                            // Support memory store contract
                            let store = result.get("store", default: Python.None)
                            let storeIsDict = Bool(builtins.isinstance(store, builtins.dict)) ?? false
                            if storeIsDict {
                                var destAddr: UInt64? = nil
                                let addrVal = store.get("addr", default: Python.None)
                                if !(Bool(addrVal.is_none) ?? false) {
                                    if let addrStr = String(addrVal), let a = UInt64(addrStr) { destAddr = a }
                                } else {
                                    let regNameObj = store.get("reg", default: Python.None)
                                    if !(Bool(regNameObj.is_none) ?? false) {
                                        if let rname = String(regNameObj), let rid = RegisterMap.id(for: rname) { destAddr = ctx.state.regs[rid] ?? 0 }
                                    }
                                }
                                var payload: [UInt8]? = nil
                                let sObj = store.get("string", default: Python.None)
                                if !(Bool(sObj.is_none) ?? false) {
                                    let s = String(sObj)
                                    if let sUnwrapped = s, let d = sUnwrapped.data(using: .utf8) { payload = Array(d) + [0] }
                                } else {
                                    let bObj = store.get("bytes", default: Python.None)
                                    let listType = builtins.list
                                    if Bool(builtins.isinstance(bObj, listType)) ?? false {
                                        var buf: [UInt8] = []
                                        for it in bObj {
                                            if let itStr = String(it), let u = UInt64(itStr) { buf.append(UInt8(truncatingIfNeeded: u)) }
                                        }
                                        payload = buf
                                    }
                                }
                                if let addr = destAddr, let bytes = payload {
                                    let base = Int(truncatingIfNeeded: addr)
                                    if base >= 0 {
                                        let needed = base + bytes.count
                                        if ctx.state.memory.count < needed { ctx.state.memory.append(contentsOf: Array(repeating: 0, count: needed - ctx.state.memory.count)) }
                                        for i in 0..<bytes.count { ctx.state.memory[base + i] = bytes[i] }
                                    }
                                }
                            }
                            let updates = result.get("regs", default: Python.None)
                            let updatesIsDict = Bool(builtins.isinstance(updates, builtins.dict)) ?? false
                            if updatesIsDict {
                                for u in updates.items() {
                                    let rname = String(u[0])
                                    if let rnameUnwrapped = rname, let rid = RegisterMap.id(for: rnameUnwrapped) {
                                        let pyVal = u[1]
                                        if let pyValStr = String(pyVal), let ival = Int64(pyValStr) { ctx.state.regs[rid] = UInt64(bitPattern: ival) }
                                        else if let pyValStr = String(pyVal), let uval = UInt64(pyValStr) { ctx.state.regs[rid] = uval }
                                    }
                                }
                            }
                        } else {
                            let none = builtins.None
                            if !(Bool(result == none) ?? false) {
                                let s = String(result)
                                if !(s?.isEmpty ?? true) { Swift.print(s!) }
                            }
                        }
                    }
                }
            }
        }
        #else
        // PythonKit is not available; no external module loading.
        _ = path // suppress unused warning
        #endif
    }
    private static func readCString(_ addr: UInt64, from mem: Data) -> String? {
        let start = Int(truncatingIfNeeded: addr)
        if start < 0 || start >= mem.count { return nil }
        var bytes: [UInt8] = []
        var i = start
        while i < mem.count {
            let b = mem[i]; i += 1
            if b == 0 { break }
            bytes.append(b)
        }
        return String(bytes: bytes, encoding: .utf8)
    }

    public struct State {
        // 64-bit registers (by ID); using RegisterMap for known names
        public var regs: [UInt16: UInt64] = [:]
        public var flags: (ZF: Bool, SF: Bool, CF: Bool, OF: Bool) = (false, false, false, false)
        public var rip: UInt64 = 0
        public var stack: [UInt64] = []
        public var memory: Data = Data() // data section backing
        public init() {}
    }

    private enum Op: UInt8 {
        case mov = 0x01
        case add = 0x02
        case sub = 0x03
        case jmp = 0x10
        case cmp = 0x11
        case je  = 0x12
        case jne = 0x13
        case call = 0x20
        case ret = 0x21
        case push = 0x30
        case pop = 0x31
        case out = 0x40
        case cout = 0x41
        case `in` = 0x42
        case enter = 0x50
        case leave = 0x51
        case mni = 0x60
        case hlt = 0xFF
        case nop = 0x00
    }

    private func getOperand(_ code: Data, _ pc: inout Int, _ state: inout State) -> UInt64 {
        let mode = code[pc]; pc += 1
        let v = readU64LE(code, &pc)
        switch mode {
        case 0: // immediate
            return v
        case 1: // register
            let id = UInt16(truncatingIfNeeded: v)
            return state.regs[id] ?? 0
        case 2: // label address (code offset)
            return v
        case 3: // memory absolute address: load 8 bytes from memory[v]
            return readU64FromMemory(address: v, state: &state)
        case 4: // memory via register id
            let id = UInt16(truncatingIfNeeded: v)
            let addr = state.regs[id] ?? 0
            return readU64FromMemory(address: addr, state: &state)
        default:
            return v
        }
    }
    private func setOperand(_ code: Data, _ pc: inout Int, _ state: inout State, value: UInt64) {
        let mode = code[pc]; pc += 1
        let v = readU64LE(code, &pc)
        switch mode {
        case 1: // register
            let id = UInt16(truncatingIfNeeded: v)
            state.regs[id] = value
        case 3: // memory absolute address
            writeU64ToMemory(address: v, value: value, state: &state)
        case 4: // memory via register id
            let id = UInt16(truncatingIfNeeded: v)
            let addr = state.regs[id] ?? 0
            writeU64ToMemory(address: addr, value: value, state: &state)
        default:
            // For now, only registers are writeable destinations
            break
        }
    }

    private func readU64LE(_ data: Data, _ offset: inout Int) -> UInt64 {
        var val: UInt64 = 0
        for i in 0..<8 { val |= UInt64(data[offset + i]) << (8 * i) }
        offset += 8
        return val
    }
    private func readU16LE(_ data: Data, _ offset: inout Int) -> UInt16 {
        var v: UInt16 = 0
        v |= UInt16(data[offset + 0])
        v |= UInt16(data[offset + 1]) << 8
        offset += 2
        return v
    }
    private func readU64FromMemory(address: UInt64, state: inout State) -> UInt64 {
        let addr = Int(truncatingIfNeeded: address)
        if addr < 0 || addr + 8 > state.memory.count { return 0 }
        var val: UInt64 = 0
        for i in 0..<8 { val |= UInt64(state.memory[addr + i]) << (8 * i) }
        return val
    }
    private func writeU64ToMemory(address: UInt64, value: UInt64, state: inout State) {
        let addr = Int(truncatingIfNeeded: address)
        if addr < 0 { return }
        let needed = addr + 8
        if state.memory.count < needed {
            state.memory.append(contentsOf: Array(repeating: 0, count: needed - state.memory.count))
        }
        var v = value
        for i in 0..<8 { state.memory[addr + i] = UInt8(truncatingIfNeeded: v & 0xFF); v >>= 8 }
    }

    private func updateAddFlags(_ a: UInt64, _ b: UInt64, _ r: UInt64, _ state: inout State) {
        state.flags.ZF = (r == 0)
        state.flags.SF = (Int64(bitPattern: r) < 0)
        // Unsigned carry
        state.flags.CF = r < a
        // Signed overflow
        let sa = Int64(bitPattern: a), sb = Int64(bitPattern: b), sr = Int64(bitPattern: r)
        state.flags.OF = ((sa > 0 && sb > 0 && sr < 0) || (sa < 0 && sb < 0 && sr > 0))
    }
    private func updateSubFlags(_ a: UInt64, _ b: UInt64, _ r: UInt64, _ state: inout State) {
        state.flags.ZF = (r == 0)
        state.flags.SF = (Int64(bitPattern: r) < 0)
        state.flags.CF = a < b
        let sa = Int64(bitPattern: a), sb = Int64(bitPattern: b), sr = Int64(bitPattern: r)
        state.flags.OF = ((sa >= 0 && sb < 0 && sr < 0) || (sa < 0 && sb >= 0 && sr >= 0))
    }

    public func run(masi: Disassembler.MASIFile) {
    var state = State()
    // Initialize memory from MASI data section bytes
    state.memory = masi.data
    // Register a demo MNI function: debug.echo expects RDI to point to a C-string in memory
    registry.register(module: "debug", name: "echo") { ctx in
        let rdi = RegisterMap.id(for: "RDI") ?? 0
        let ptr = ctx.state.regs[rdi] ?? 0
        if let s = Self.readCString(ptr, from: ctx.state.memory) {
            Swift.print(s)
        }
    }
    // Load external modules from ./modules
    let cwd = FileManager.default.currentDirectoryPath
    let modulesDir = (cwd as NSString).appendingPathComponent("modules")
    loadModules(from: modulesDir)
    var pc = Int(truncatingIfNeeded: masi.entry)
        let code = masi.code
        while pc < code.count {
            // Label at pc is implicit; RIP is code offset
            state.rip = UInt64(pc)
            let byte = code[pc]; pc += 1
            guard let op = Op(rawValue: byte) else { continue }
            switch op {
            case .nop:
                continue
            case .hlt:
                return
            case .mov:
                // dest then source
                let destPos = pc
                _ = code[pc]; pc += 1 // mode skip in setOperand
                _ = readU64LE(code, &pc) // id skip
                let srcVal = getOperand(code, &pc, &state)
                pc = destPos
                setOperand(code, &pc, &state, value: srcVal)
            case .add:
                let destMode = code[pc]; pc += 1
                let destId64 = readU64LE(code, &pc)
                let srcVal = getOperand(code, &pc, &state)
                if destMode == 1 {
                    let id = UInt16(truncatingIfNeeded: destId64)
                    let a = state.regs[id] ?? 0
                    let r = a &+ srcVal
                    state.regs[id] = r
                    updateAddFlags(a, srcVal, r, &state)
                }
            case .sub:
                let destMode = code[pc]; pc += 1
                let destId64 = readU64LE(code, &pc)
                let srcVal = getOperand(code, &pc, &state)
                if destMode == 1 {
                    let id = UInt16(truncatingIfNeeded: destId64)
                    let a = state.regs[id] ?? 0
                    let r = a &- srcVal
                    state.regs[id] = r
                    updateSubFlags(a, srcVal, r, &state)
                }
            case .cmp:
                let a = getOperand(code, &pc, &state)
                let b = getOperand(code, &pc, &state)
                let r = a &- b
                updateSubFlags(a, b, r, &state)
            case .jmp:
                let t = getOperand(code, &pc, &state)
                pc = Int(truncatingIfNeeded: t)
            case .je:
                let t = getOperand(code, &pc, &state)
                if state.flags.ZF { pc = Int(truncatingIfNeeded: t) }
            case .jne:
                let t = getOperand(code, &pc, &state)
                if !state.flags.ZF { pc = Int(truncatingIfNeeded: t) }
            case .call:
                let t = getOperand(code, &pc, &state)
                state.stack.append(UInt64(pc))
                pc = Int(truncatingIfNeeded: t)
            case .ret:
                if let ret = state.stack.popLast() { pc = Int(truncatingIfNeeded: ret) }
            case .push:
                let v = getOperand(code, &pc, &state)
                state.stack.append(v)
            case .pop:
                let destMode = code[pc]; pc += 1
                let destId64 = readU64LE(code, &pc)
                if destMode == 1, let v = state.stack.popLast() {
                    let id = UInt16(truncatingIfNeeded: destId64)
                    state.regs[id] = v
                }
            case .enter:
                let size = getOperand(code, &pc, &state)
                // Prologue: push RBP; move RBP=RSP; sub RSP,size
                let rbp = RegisterMap.id(for: "RBP") ?? 0
                let rsp = RegisterMap.id(for: "RSP") ?? 0
                let curRbp = state.regs[rbp] ?? 0
                state.stack.append(curRbp)
                let curRsp = state.regs[rsp] ?? UInt64(state.stack.count)
                state.regs[rbp] = curRsp
                // grow stack to reflect locals (conceptual); track RSP as stack.count + size in bytes
                state.regs[rsp] = curRsp &+ size
            case .leave:
                // Epilogue: move RSP=RBP; pop RBP
                let rbp = RegisterMap.id(for: "RBP") ?? 0
                let rsp = RegisterMap.id(for: "RSP") ?? 0
                let frameTop = state.regs[rbp] ?? 0
                state.regs[rsp] = frameTop
                if let v = state.stack.popLast() { state.regs[rbp] = v }
            case .mni:
                let modPtr = getOperand(code, &pc, &state)
                let fnPtr = getOperand(code, &pc, &state)
                let argc = Int(readU16LE(code, &pc))
                var argv: [String] = []
                for _ in 0..<argc {
                    let mode = code[pc]; pc += 1
                    let val = readU64LE(code, &pc)
                    switch mode {
                    case 0:
                        argv.append(String(val))
                    case 1:
                        let id = UInt16(truncatingIfNeeded: val)
                        argv.append(RegisterMap.name(for: id) ?? "REG\(id)")
                    case 3:
                        if let s = Self.readCString(val, from: state.memory) { argv.append(s) } else { argv.append(String(format: "$0x%llX", val)) }
                    case 4:
                        let id = UInt16(truncatingIfNeeded: val)
                        argv.append("$" + (RegisterMap.name(for: id) ?? "REG\(id)"))
                    default:
                        argv.append(String(val))
                    }
                }
                func readCString(at addr: UInt64) -> String? {
                    let start = Int(truncatingIfNeeded: addr)
                    if start < 0 || start >= state.memory.count { return nil }
                    var bytes: [UInt8] = []
                    var i = start
                    while i < state.memory.count {
                        let b = state.memory[i]; i += 1
                        if b == 0 { break }
                        bytes.append(b)
                    }
                    return String(bytes: bytes, encoding: .utf8)
                }
                if let m = readCString(at: modPtr), let f = readCString(at: fnPtr), let fn = registry.lookup(module: m, name: f) {
                    var ctx = MNICtx(state: state, args: argv)
                    fn(&ctx)
                    state = ctx.state
                } else {
                    // no-op or error print
                    try? FileHandle.standardError.write(contentsOf: "MNI: function not found\n".data(using: .utf8)!)
                }
            case .out:
                let p = getOperand(code, &pc, &state)
                let v = getOperand(code, &pc, &state)
                func printString(at addr: UInt64, toError: Bool) {
                    let start = Int(truncatingIfNeeded: addr)
                    if start < 0 || start >= state.memory.count { return }
                    var sBytes: [UInt8] = []
                    var i = start
                    while i < state.memory.count {
                        let b = state.memory[i]; i += 1
                        if b == 0 { break }
                        sBytes.append(b)
                    }
                    if !sBytes.isEmpty, let str = String(bytes: sBytes, encoding: .utf8) {
                        if toError { try? FileHandle.standardError.write(contentsOf: (str + "\n").data(using: .utf8)!) }
                        else { Swift.print(str) }
                    } else {
                        if toError { try? FileHandle.standardError.write(contentsOf: ("\(addr)\n").data(using: .utf8)!) }
                        else { Swift.print(addr) }
                    }
                }
                // Port 0/1 -> stdout, 2 -> stderr
                let toErr = (p == 2)
                // If value points into memory, print a C-string until null terminator; else print number
                if v < UInt64(state.memory.count) {
                    printString(at: v, toError: toErr)
                } else {
                    if toErr { try? FileHandle.standardError.write(contentsOf: ("\(v)\n").data(using: .utf8)!) }
                    else { Swift.print(v) }
                }
            case .cout:
                let p = getOperand(code, &pc, &state)
                let v = getOperand(code, &pc, &state)
                let toErr = (p == 2)
                func writeChar(_ ch: UInt8) {
                    if toErr { try? FileHandle.standardError.write(contentsOf: Data([ch])) }
                    else { if let s = String(bytes: [ch], encoding: .utf8) { Swift.print(s, terminator: "") } }
                }
                if v < UInt64(state.memory.count) {
                    // treat as pointer to first byte
                    let b = state.memory[Int(v)]
                    writeChar(b)
                } else {
                    writeChar(UInt8(truncatingIfNeeded: v))
                }
            case .in:
                let destAddr = getOperand(code, &pc, &state)
                // Read a line from stdin and store as null-terminated at destAddr
                if let line = readLine(strippingNewline: false) {
                    let bytes = Array(line.utf8) + [0]
                    let base = Int(truncatingIfNeeded: destAddr)
                    let needed = base + bytes.count
                    if base >= 0 {
                        if state.memory.count < needed { state.memory.append(contentsOf: Array(repeating: 0, count: needed - state.memory.count)) }
                        for i in 0..<bytes.count { state.memory[base + i] = bytes[i] }
                    }
                }
            }
        }
    }
}
