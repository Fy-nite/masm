
import Foundation
public class Instructions {
    public static func printUsage() -> Void {
                let usage = """
                Usage: valamasm [options] <input file>

                Modes:
                    <.masm>            Assemble to MASI (use -o to choose output)
                    <.masi> --disasm   Disassemble to MASM (use -o to choose output)
                    <.masi> --dump     Dump MASI header/sections/labels
                    <.masi> --run      Execute MASI with built-in interpreter

                Options:
                    -o <file>          Specify output file (default: out.masi for assemble)
                    -x, --disasm       Disassemble .masi input
                    -t, --dump         Dump .masi structure
                    -r, --run          Run .masi in interpreter
                    -d, --debug        Enable debug logs
                    -h, --help         Show this help message
                """
        print(usage)
    }
    // Data section support
    enum DataDirective {
        case db(label: String?, bytes: [UInt8])
        case dw(label: String?, words: [UInt16])
        case dd(label: String?, dwords: [UInt32])
        case dq(label: String?, qwords: [UInt64])
        case df(label: String?, floats: [Float])
        case ddbl(label: String?, doubles: [Double])
        case res(label: String?, bytes: Int)
        case directDB(address: Int, bytes: [UInt8], nullTerminated: Bool)
    }
    private var dataDirectives: [DataDirective] = []
    private var dataLabelOffsets: [String:Int] = [:]
    private var mniStringLabels: [String:String] = [:] // content -> generated label name
    // Minimal opcodes for demo MASI format
    enum Op: UInt8 {
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
    public indirect enum Instruction {
        case mov(destination: String, source: String)
        case add(destination: String, source: String)
        case sub(destination: String, source: String)
        case jmp(label: String)
        case cmp(operand1: String, operand2: String)
        case je(label: String)
        case jne(label: String)
        case label(name: String)
        case call(function: String)
        case ret
        case push(value: String)
        case pop(destination: String)
        case output(port: String, value: String)
        case coutput(port: String, value: String)
        case input(dest: String)
        case enter(size: String)
        case leave
    case mni(modulePtr: String, functionPtr: String, args: [String])
        case halt
        case nop
    }
    private func parseStringLiteral(_ token: String) -> [UInt8]? {
        // token surrounded by quotes
        guard token.first == "\"", token.last == "\"" else { return nil }
        let inner = token.dropFirst().dropLast()
        var out: [UInt8] = []
        var i = inner.startIndex
        while i < inner.endIndex {
            let c = inner[i]
            if c == "\\" {
                i = inner.index(after: i)
                if i == inner.endIndex { break }
                switch inner[i] {
                case "n": out.append(10)
                case "r": out.append(13)
                case "t": out.append(9)
                case "\\": out.append(92)
                case "\"": out.append(34)
                case "0": out.append(0)
                default:
                    let scalar = inner[i].unicodeScalars.first?.value ?? 0
                    out.append(UInt8(truncatingIfNeeded: scalar))
                }
            } else {
                let scalar = c.unicodeScalars.first?.value ?? 0
                out.append(UInt8(truncatingIfNeeded: scalar))
            }
            i = inner.index(after: i)
        }
        return out
    }

    private func parseDataLine(_ trimmed: String) -> Bool {
        // Handle label: DIRECTIVE ... or standalone directives like db $100 "..."
        var labelName: String? = nil
        var rest = trimmed
        if let colon = trimmed.firstIndex(of: ":") {
            labelName = String(trimmed[..<colon]).trimmingCharacters(in: .whitespaces)
            rest = String(trimmed[trimmed.index(after: colon)...]).trimmingCharacters(in: .whitespaces)
        }
        // Identify directive keyword
        let lower = rest.lowercased()
        func parseValues(_ part: String) -> [String] { part.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) } }
        if lower.hasPrefix("db ") {
            let rhs = String(rest.dropFirst(3)).trimmingCharacters(in: .whitespaces)
            // direct memory: db $<addr> "str"
            if rhs.hasPrefix("$") {
                let comps = rhs.split(separator: " ", maxSplits: 1).map { String($0) }
                if comps.count == 2 {
                    let addrStr = comps[0].dropFirst()
                    let addr = Int(addrStr.hasPrefix("0x") ? String(addrStr.dropFirst(2)) : String(addrStr), radix: addrStr.hasPrefix("0x") ? 16 : 10) ?? 0
                    if let bytes = parseStringLiteral(comps[1]) {
                        dataDirectives.append(.directDB(address: addr, bytes: bytes, nullTerminated: true))
                        return true
                    }
                }
            }
            // label-based DB values or strings separated by commas
            var allBytes: [UInt8] = []
            for tok in parseValues(rhs) {
                if let strBytes = parseStringLiteral(tok) {
                    allBytes.append(contentsOf: strBytes)
                } else if tok.lowercased().hasPrefix("0x"), let v = UInt8(tok.dropFirst(2), radix: 16) {
                    allBytes.append(v)
                } else if let v = Int(tok) { allBytes.append(UInt8(truncatingIfNeeded: v)) }
            }
            dataDirectives.append(.db(label: labelName, bytes: allBytes))
            return true
        } else if lower.hasPrefix("dw ") {
            let rhs = String(rest.dropFirst(3))
            let vals = parseValues(rhs)
            var a: [UInt16] = []
            for tok in vals {
                if tok.lowercased().hasPrefix("0x"), let v = UInt16(tok.dropFirst(2), radix: 16) { a.append(v) }
                else if let v = Int(tok) { a.append(UInt16(truncatingIfNeeded: v)) }
            }
            dataDirectives.append(.dw(label: labelName, words: a)); return true
        } else if lower.hasPrefix("dd ") {
            let rhs = String(rest.dropFirst(3))
            let vals = parseValues(rhs)
            var a: [UInt32] = []
            for tok in vals {
                if tok.lowercased().hasPrefix("0x"), let v = UInt32(tok.dropFirst(2), radix: 16) { a.append(v) }
                else if let v = Int(tok) { a.append(UInt32(truncatingIfNeeded: v)) }
            }
            dataDirectives.append(.dd(label: labelName, dwords: a)); return true
        } else if lower.hasPrefix("dq ") {
            let rhs = String(rest.dropFirst(3))
            let vals = parseValues(rhs)
            var a: [UInt64] = []
            for tok in vals {
                if tok.hasPrefix("#") { /* label address placeholder not supported here */ }
                if tok.lowercased().hasPrefix("0x"), let v = UInt64(tok.dropFirst(2), radix: 16) { a.append(v) }
                else if let v = UInt64(tok) { a.append(v) }
            }
            dataDirectives.append(.dq(label: labelName, qwords: a)); return true
        } else if lower.hasPrefix("df ") {
            let rhs = String(rest.dropFirst(3))
            let vals = parseValues(rhs)
            var a: [Float] = []
            for tok in vals { if let v = Float(tok) { a.append(v) } }
            dataDirectives.append(.df(label: labelName, floats: a)); return true
        } else if lower.hasPrefix("ddbl ") {
            let rhs = String(rest.dropFirst(5))
            let vals = parseValues(rhs)
            var a: [Double] = []
            for tok in vals { if let v = Double(tok) { a.append(v) } }
            dataDirectives.append(.ddbl(label: labelName, doubles: a)); return true
        } else if lower.hasPrefix("resb ") || lower.hasPrefix("resw ") || lower.hasPrefix("resd ") || lower.hasPrefix("resq ") || lower.hasPrefix("resf ") || lower.hasPrefix("resdbl ") {
            let parts = rest.split(separator: " ", maxSplits: 1).map { String($0) }
            guard parts.count == 2, let count = Int(parts[1]) else { return false }
            let factor: Int
            switch parts[0].lowercased() {
            case "resb": factor = 1
            case "resw": factor = 2
            case "resd": factor = 4
            case "resq": factor = 8
            case "resf": factor = 4
            case "resdbl": factor = 8
            default: factor = 1
            }
            dataDirectives.append(.res(label: labelName, bytes: count * factor)); return true
        }
        return false
    }

    public func ParseInstructions(from input: String) -> [Instruction] {
        var instructions: [Instruction] = []
        dataDirectives = []
        dataLabelOffsets = [:]
        mniStringLabels = [:]
        for rawLine in input.split(separator: "\n", omittingEmptySubsequences: false) {
            var linecontents = String(rawLine)
            // strip comments starting with ';'
            if let semicolonRange = linecontents.firstIndex(of: ";") {
                linecontents = String(linecontents[..<semicolonRange])
            }
            let trimmed = linecontents.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmed.isEmpty else { continue }
            // Try data line first
            if parseDataLine(trimmed) {
                continue
            }
            let components = trimmed.split(separator: " ", maxSplits: 1).map { String($0) }
            guard let mnemonic = components.first else { continue }
            let operands = components.count > 1 ? components[1].split(separator: " ").map { $0.trimmingCharacters(in: .whitespaces) } : []
            switch mnemonic.lowercased() {
            case "lbl" where operands.count == 1:
                // Handle label definition
                let instruction = Instruction.label(name: operands[0])
                Dbg.print("Defined label: \(operands[0])")
                instructions.append(instruction)
            case "mov" where operands.count == 2:
                // Handle mov instruction
                let instruction = Instruction.mov(destination: operands[0], source: operands[1])
                Dbg.print("Parsed mov instruction: \(operands[0]) <- \(operands[1])")
                instructions.append(instruction)
            case "add" where operands.count == 2:
                // Handle add instruction
                let instruction = Instruction.add(destination: operands[0], source: operands[1])
                Dbg.print("Parsed add instruction: \(operands[0]) <- \(operands[1])")
                instructions.append(instruction)
            case "sub" where operands.count == 2:
                // Handle sub instruction
                let instruction = Instruction.sub(destination: operands[0], source: operands[1])
                Dbg.print("Parsed sub instruction: \(operands[0]) <- \(operands[1])")
                instructions.append(instruction)
            case "jmp" where operands.count == 1:
                // Handle jmp instruction
                let instruction = Instruction.jmp(label: operands[0])
                Dbg.print("Parsed jmp instruction: \(operands[0])")
                instructions.append(instruction)
            case "cmp" where operands.count == 2:
                // Handle cmp instruction
                let instruction = Instruction.cmp(operand1: operands[0], operand2: operands[1])
                Dbg.print("Parsed cmp instruction: \(operands[0]) <- \(operands[1])")
                instructions.append(instruction)
            case "je" where operands.count == 1:
                // Handle je instruction
                let instruction = Instruction.je(label: operands[0])
                Dbg.print("Parsed je instruction: \(operands[0])")
                instructions.append(instruction)
            case "jne" where operands.count == 1:
                // Handle jne instruction
                let instruction = Instruction.jne(label: operands[0])
                Dbg.print("Parsed jne instruction: \(operands[0])")
                instructions.append(instruction)
            case "enter" where operands.count == 1:
                let instruction = Instruction.enter(size: operands[0])
                Dbg.print("Parsed enter instruction: size \(operands[0])")
                instructions.append(instruction)
            case "leave" where operands.isEmpty:
                let instruction = Instruction.leave
                Dbg.print("Parsed leave instruction")
                instructions.append(instruction)
            case "mni" where operands.count >= 1:
                // Support two forms:
                // 1) MNI $mod $fn
                // 2) MNI module.func [args...]
                if operands.count >= 2 && operands[0].hasPrefix("$") && operands[1].hasPrefix("$") {
                    let extra = operands.count > 2 ? Array(operands.dropFirst(2)) : []
                    let instruction = Instruction.mni(modulePtr: operands[0], functionPtr: operands[1], args: extra)
                    Dbg.print("Parsed mni instruction: modulePtr \(operands[0]) functionPtr \(operands[1]) args \(extra)")
                    instructions.append(instruction)
                } else {
                    let name = operands[0]
                    let parts = name.split(separator: ".", maxSplits: 1).map(String.init)
                    let module = parts.first ?? name
                    let function = parts.count > 1 ? parts[1] : "main"
                    let rawArgs = operands.count > 1 ? Array(operands.dropFirst(1)) : []
                    func labelForString(_ s: String) -> String {
                        if let lbl = mniStringLabels[s] { return lbl }
                        // generate a new label and append to data as null-terminated
                        let safe = s.replacingOccurrences(of: "[^A-Za-z0-9_]", with: "_", options: .regularExpression)
                        let label = "__mni_str_\(mniStringLabels.count)_\(safe)"
                        var bytes = Array(s.utf8); bytes.append(0)
                        dataDirectives.append(.db(label: label, bytes: bytes))
                        mniStringLabels[s] = label
                        return label
                    }
                    let modLbl = labelForString(module)
                    let fnLbl = labelForString(function)
                    // Intern arguments as strings too and keep labels
                    var argLabels: [String] = []
                    for a in rawArgs { argLabels.append(labelForString(a)) }
                    let instruction = Instruction.mni(modulePtr: "$\(modLbl)", functionPtr: "$\(fnLbl)", args: argLabels)
                    Dbg.print("Parsed mni instruction: \(module).\(function) -> labels \(modLbl), \(fnLbl) args \(argLabels)")
                    instructions.append(instruction)
                }
            case "label" where operands.count == 1:
                // Handle label definition
                let instruction = Instruction.label(name: operands[0])
                Dbg.print("Defined label: \(operands[0])")
                instructions.append(instruction)
            case "call" where operands.count == 1:
                // Handle call instruction
                let instruction = Instruction.call(function: operands[0])
                Dbg.print("Parsed call instruction: \(operands[0])")
                instructions.append(instruction)
            case "ret" where operands.isEmpty:
                // Handle ret instruction
                let instruction = Instruction.ret
                Dbg.print("Parsed ret instruction")
                instructions.append(instruction)
            case "push" where operands.count == 1:
                // Handle push instruction
                let instruction = Instruction.push(value: operands[0])
                Dbg.print("Parsed push instruction: \(operands[0])")
                instructions.append(instruction)
            case "pop" where operands.count == 1:
                // Handle pop instruction
                let instruction = Instruction.pop(destination: operands[0])
                Dbg.print("Parsed pop instruction: \(operands[0])")
                instructions.append(instruction)
            case "nop" where operands.isEmpty:
                // Handle nop instruction
                let instruction = Instruction.nop
                Dbg.print("Parsed nop instruction")
                instructions.append(instruction)
            case "out" where operands.count == 2:
                // out <port|register> <value|register>
                let port = operands[0]
                let value = operands[1]
                let instruction = Instruction.output(port: port, value: value)
                Dbg.print("Parsed out instruction: port \(port) value \(value)")
                instructions.append(instruction)
            case "cout" where operands.count == 2:
                let port = operands[0]
                let value = operands[1]
                let instruction = Instruction.coutput(port: port, value: value)
                Dbg.print("Parsed cout instruction: port \(port) value \(value)")
                instructions.append(instruction)
            case "in" where operands.count == 1:
                let instruction = Instruction.input(dest: operands[0])
                Dbg.print("Parsed in instruction: dest \(operands[0])")
                instructions.append(instruction)
            case "hlt" where operands.isEmpty:
                // Handle halt instruction
                let instruction = Instruction.halt
                Dbg.print("Parsed halt instruction")
                instructions.append(instruction)
            default:
                print("Error: Unrecognized instruction or wrong number of operands: \(trimmed)")
            }
        }
        return instructions
    }
    // Helper to write little-endian values
    private func writeU16LE(_ value: UInt16, to data: inout Data) {
        var v = value.littleEndian
        withUnsafeBytes(of: &v) { data.append(contentsOf: $0) }
    }
    private func writeU32LE(_ value: UInt32, to data: inout Data) {
        var v = value.littleEndian
        withUnsafeBytes(of: &v) { data.append(contentsOf: $0) }
    }
    private func writeU64LE(_ value: UInt64, to data: inout Data) {
        var v = value.littleEndian
        withUnsafeBytes(of: &v) { data.append(contentsOf: $0) }
    }

    private func isRegister(_ s: String) -> Bool {
        return RegisterMap.id(for: s) != nil
    }
    private func encodeOperand(_ s: String, labels: [String:Int], dataLabels: [String:Int], codeBase: Int) -> (mode: UInt8, value: UInt64) {
        // mode: 0 immediate, 1 register id, 2 label address
        if s.hasPrefix("#") { // label ref (code)
            let name = String(s.dropFirst())
            if let off = labels[name] { return (2, UInt64(codeBase + off)) }
            // unresolved; placeholder 0
            return (2, 0)
        }
        if s.hasPrefix("$") { // memory addressing
            let rest = String(s.dropFirst())
            // $REG => mode 4 with register id
            if isRegister(rest) {
                let id = RegisterMap.id(for: rest) ?? 0
                return (4, UInt64(id))
            }
            // $label in data => mode 3 with absolute data offset
            if let off = dataLabelOffsets[rest] ?? dataLabels[rest] { return (3, UInt64(off)) }
            // $0x.. or $number
            if rest.lowercased().hasPrefix("0x"), let v = UInt64(rest.dropFirst(2), radix: 16) { return (3, v) }
            if let v = UInt64(rest) { return (3, v) }
            return (3, 0)
        }
        if isRegister(s) {
            let id = RegisterMap.id(for: s) ?? 0
            return (1, UInt64(id))
        }
        if let off = dataLabelOffsets[s] ?? dataLabels[s] {
            return (0, UInt64(off))
        }
        // try integer immediate (supports 0x...)
        if s.lowercased().hasPrefix("0x"), let v = UInt64(s.dropFirst(2), radix: 16) {
            return (0, v)
        }
        if let v = UInt64(s) { return (0, v) }
        return (0, 0)
    }

    public func CompileInstructions(to output: String, from instructions: [Instruction]) -> Bool {
        // Build label table with code offsets (first pass approximate sizing)
        var labels: [String:Int] = [:]
        var pc = 0 // code size in bytes
        var entryOffset: Int? = nil
        for ins in instructions {
            switch ins {
            case .label(let name):
                labels[name] = pc
            case .ret, .halt, .nop:
                if entryOffset == nil { entryOffset = pc }
                pc += 1 // opcode only
            case .leave:
                if entryOffset == nil { entryOffset = pc }
                pc += 1
            case .enter:
                if entryOffset == nil { entryOffset = pc }
                pc += 1 + (1+8)
            case .jmp, .je, .jne:
                if entryOffset == nil { entryOffset = pc }
                pc += 1 /*opcode*/ + 1 /*mode*/ + 8 /*addr*/
            case .call:
                if entryOffset == nil { entryOffset = pc }
                pc += 1 + 1 + 8
            case .push, .pop:
                if entryOffset == nil { entryOffset = pc }
                pc += 1 + 1 + 8
            case .output, .coutput:
                if entryOffset == nil { entryOffset = pc }
                pc += 1 + (1+8) + (1+8)
            case .input:
                if entryOffset == nil { entryOffset = pc }
                pc += 1 + (1+8)
            case .mni(_, _, let args):
                if entryOffset == nil { entryOffset = pc }
                // opcode + mod(1+8) + fn(1+8) + count(2) + each arg (1+8)
                pc += 1 + (1+8) + (1+8) + 2 + (args.count * (1+8))
            case .mov, .add, .sub, .cmp:
                if entryOffset == nil { entryOffset = pc }
                pc += 1 + (1+8) + (1+8)
            }
        }

        // Header (16 bytes): magic(4), version(2), reserved(2), entry(8)
        var header = Data()
        header.append(contentsOf: [0x4D,0x41,0x53,0x49]) // 'MASI'
        writeU16LE(1, to: &header) // version
        writeU16LE(0, to: &header) // reserved
    // Entry: first non-label instruction offset in code section
    writeU64LE(UInt64(entryOffset ?? 0), to: &header)

        // Build simple label table: count + entries of nameLen(2)+name+addr(8)
        var labelTable = Data()
        let labelNames = Array(labels.keys)
        writeU16LE(UInt16(labelNames.count), to: &labelTable)
        for name in labelNames {
            let bytes = Array(name.utf8)
            writeU16LE(UInt16(bytes.count), to: &labelTable)
            labelTable.append(contentsOf: bytes)
            writeU64LE(UInt64(labels[name] ?? 0), to: &labelTable)
        }

        // Build data section from directives
        var data = Data()
        func alignData(_ alignment: Int) {
            let rem = data.count % alignment
            if rem != 0 { data.append(contentsOf: Array(repeating: 0, count: alignment - rem)) }
        }
        for d in dataDirectives {
            switch d {
            case .db(let label, let bytes):
                if let name = label { dataLabelOffsets[name] = data.count }
                data.append(contentsOf: bytes)
            case .dw(let label, let words):
                if let name = label { dataLabelOffsets[name] = data.count }
                for w in words { var v = w.littleEndian; withUnsafeBytes(of: &v) { data.append(contentsOf: $0) } }
            case .dd(let label, let dws):
                if let name = label { dataLabelOffsets[name] = data.count }
                for w in dws { var v = w.littleEndian; withUnsafeBytes(of: &v) { data.append(contentsOf: $0) } }
            case .dq(let label, let qws):
                if let name = label { dataLabelOffsets[name] = data.count }
                for w in qws { var v = w.littleEndian; withUnsafeBytes(of: &v) { data.append(contentsOf: $0) } }
            case .df(let label, let floats):
                if let name = label { dataLabelOffsets[name] = data.count }
                for f in floats { var v = f.bitPattern.littleEndian; withUnsafeBytes(of: &v) { data.append(contentsOf: $0) } }
            case .ddbl(let label, let doubles):
                if let name = label { dataLabelOffsets[name] = data.count }
                for f in doubles { var v = f.bitPattern.littleEndian; withUnsafeBytes(of: &v) { data.append(contentsOf: $0) } }
            case .res(let label, let bytes):
                if let name = label { dataLabelOffsets[name] = data.count }
                data.append(contentsOf: Array(repeating: 0, count: bytes))
            case .directDB(let address, let bytes, let nullTerminated):
                let needed = address + bytes.count + (nullTerminated ? 1 : 0)
                if data.count < needed { data.append(contentsOf: Array(repeating: 0, count: needed - data.count)) }
                if address >= 0 {
                    for (i, b) in bytes.enumerated() { data[address + i] = b }
                    if nullTerminated { data[address + bytes.count] = 0 }
                }
            }
        }

        // No import/local/const/export for now (minimal)
        let importTable = Data()
        // Serialize data label map into locals table: count + [nameLen u16][name][offset u64]
        var localVarTable = Data()
        let dataNames = Array(dataLabelOffsets.keys)
        writeU16LE(UInt16(dataNames.count), to: &localVarTable)
        for name in dataNames {
            let nb = Array(name.utf8)
            writeU16LE(UInt16(nb.count), to: &localVarTable)
            localVarTable.append(contentsOf: nb)
            writeU64LE(UInt64(dataLabelOffsets[name] ?? 0), to: &localVarTable)
        }
        let constTable = Data()
        let exportTable = Data()
        let dataSection = data

        // Emit code section (second pass)
        var code = Data()
        for ins in instructions {
            switch ins {
            case .label:
                continue
            case .mov(let d, let s):
                code.append(Op.mov.rawValue)
                let encD = encodeOperand(d, labels: labels, dataLabels: dataLabelOffsets, codeBase: 0)
                code.append(encD.mode)
                writeU64LE(encD.value, to: &code)
                let encS = encodeOperand(s, labels: labels, dataLabels: dataLabelOffsets, codeBase: 0)
                code.append(encS.mode)
                writeU64LE(encS.value, to: &code)
            case .add(let d, let s):
                code.append(Op.add.rawValue)
                let ed = encodeOperand(d, labels: labels, dataLabels: dataLabelOffsets, codeBase: 0)
                code.append(ed.mode); writeU64LE(ed.value, to: &code)
                let es = encodeOperand(s, labels: labels, dataLabels: dataLabelOffsets, codeBase: 0)
                code.append(es.mode); writeU64LE(es.value, to: &code)
            case .sub(let d, let s):
                code.append(Op.sub.rawValue)
                let ed = encodeOperand(d, labels: labels, dataLabels: dataLabelOffsets, codeBase: 0)
                code.append(ed.mode); writeU64LE(ed.value, to: &code)
                let es = encodeOperand(s, labels: labels, dataLabels: dataLabelOffsets, codeBase: 0)
                code.append(es.mode); writeU64LE(es.value, to: &code)
            case .cmp(let a, let b):
                code.append(Op.cmp.rawValue)
                let ea = encodeOperand(a, labels: labels, dataLabels: dataLabelOffsets, codeBase: 0)
                code.append(ea.mode); writeU64LE(ea.value, to: &code)
                let eb = encodeOperand(b, labels: labels, dataLabels: dataLabelOffsets, codeBase: 0)
                code.append(eb.mode); writeU64LE(eb.value, to: &code)
            case .jmp(let l):
                code.append(Op.jmp.rawValue)
                let enc = encodeOperand(l, labels: labels, dataLabels: dataLabelOffsets, codeBase: 0)
                code.append(enc.mode); writeU64LE(enc.value, to: &code)
            case .je(let l):
                code.append(Op.je.rawValue)
                let enc = encodeOperand(l, labels: labels, dataLabels: dataLabelOffsets, codeBase: 0)
                code.append(enc.mode); writeU64LE(enc.value, to: &code)
            case .jne(let l):
                code.append(Op.jne.rawValue)
                let enc = encodeOperand(l, labels: labels, dataLabels: dataLabelOffsets, codeBase: 0)
                code.append(enc.mode); writeU64LE(enc.value, to: &code)
            case .call(let l):
                code.append(Op.call.rawValue)
                let enc = encodeOperand(l, labels: labels, dataLabels: dataLabelOffsets, codeBase: 0)
                code.append(enc.mode); writeU64LE(enc.value, to: &code)
            case .ret:
                code.append(Op.ret.rawValue)
            case .push(let v):
                code.append(Op.push.rawValue)
                let enc = encodeOperand(v, labels: labels, dataLabels: dataLabelOffsets, codeBase: 0)
                code.append(enc.mode); writeU64LE(enc.value, to: &code)
            case .pop(let d):
                code.append(Op.pop.rawValue)
                let enc = encodeOperand(d, labels: labels, dataLabels: dataLabelOffsets, codeBase: 0)
                code.append(enc.mode); writeU64LE(enc.value, to: &code)
            case .output(let p, let v):
                code.append(Op.out.rawValue)
                let ep = encodeOperand(p, labels: labels, dataLabels: dataLabelOffsets, codeBase: 0)
                code.append(ep.mode); writeU64LE(ep.value, to: &code)
                let ev = encodeOperand(v, labels: labels, dataLabels: dataLabelOffsets, codeBase: 0)
                code.append(ev.mode); writeU64LE(ev.value, to: &code)
            case .coutput(let p, let v):
                code.append(Op.cout.rawValue)
                let ep = encodeOperand(p, labels: labels, dataLabels: dataLabelOffsets, codeBase: 0)
                code.append(ep.mode); writeU64LE(ep.value, to: &code)
                let ev = encodeOperand(v, labels: labels, dataLabels: dataLabelOffsets, codeBase: 0)
                code.append(ev.mode); writeU64LE(ev.value, to: &code)
            case .input(let d):
                code.append(Op.in.rawValue)
                let ed = encodeOperand(d, labels: labels, dataLabels: dataLabelOffsets, codeBase: 0)
                code.append(ed.mode); writeU64LE(ed.value, to: &code)
            case .halt:
                code.append(Op.hlt.rawValue)
            case .nop:
                code.append(Op.nop.rawValue)
            case .enter(let sz):
                code.append(Op.enter.rawValue)
                let e = encodeOperand(sz, labels: labels, dataLabels: dataLabelOffsets, codeBase: 0)
                code.append(e.mode); writeU64LE(e.value, to: &code)
            case .leave:
                code.append(Op.leave.rawValue)
            case .mni(let m, let f, let a):
                code.append(Op.mni.rawValue)
                let em = encodeOperand(m, labels: labels, dataLabels: dataLabelOffsets, codeBase: 0)
                code.append(em.mode); writeU64LE(em.value, to: &code)
                let ef = encodeOperand(f, labels: labels, dataLabels: dataLabelOffsets, codeBase: 0)
                code.append(ef.mode); writeU64LE(ef.value, to: &code)
                // Write args as: count u16 + each arg pointer encoded as immediate addresses
                writeU16LE(UInt16(a.count), to: &code)
                for argLbl in a {
                    let enc = encodeOperand("$" + argLbl, labels: labels, dataLabels: dataLabelOffsets, codeBase: 0)
                    // Force absolute address (mode 3)
                    code.append(enc.mode)
                    writeU64LE(enc.value, to: &code)
                }
            }
        }

        // Assemble final MASI file layout (very simple):
        // [Header16]
        // [ImportTableSize4][ImportTable]
        // [LocalVarTableSize4][LocalVarTable]
        // [LabelTableSize4][LabelTable]
        // [ConstTableSize4][ConstTable]
        // [DataSize4][Data]
        // [ExportTableSize4][ExportTable]
        // [CodeSize4][Code]
        var out = Data()
        out.append(header)
        func appendChunk(_ d: Data) {
            var size = UInt32(d.count).littleEndian
            withUnsafeBytes(of: &size) { out.append(contentsOf: $0) }
            out.append(d)
        }
        appendChunk(importTable)
        appendChunk(localVarTable)
        appendChunk(labelTable)
        appendChunk(constTable)
        appendChunk(dataSection)
        appendChunk(exportTable)
        appendChunk(code)

        // Print compile summary before writing file
        let totalSize = out.count
        let labelCount = labelNames.count
        let dataLabelCount = dataLabelOffsets.count
        let instrCount = instructions.filter { if case .label = $0 { return false } else { return true } }.count
        print("Compile summary:")
        print("- Header: \(header.count) bytes")
        print("- Sections:")
        print("  - import: \(importTable.count) bytes")
        print("  - locals (data labels): \(localVarTable.count) bytes (\(dataLabelCount) labels)")
        print("  - labels: \(labelTable.count) bytes (\(labelCount) labels)")
        print("  - const: \(constTable.count) bytes")
        print("  - data: \(dataSection.count) bytes")
        print("  - export: \(exportTable.count) bytes")
        print("  - code: \(code.count) bytes (\(instrCount) instructions)")
        print(String(format: "- Entry: 0x%016llX", UInt64(entryOffset ?? 0)))
        // 4-byte size prefix per chunk x 7 chunks
        let chunkOverhead = 7 * 4
        print("- Total output: \(totalSize) bytes (including \(chunkOverhead) bytes of chunk headers)")

        do {
            try out.write(to: URL(fileURLWithPath: output))
            return true
        } catch {
            print("Failed to write MASI: \(error)")
            return false
        }
    }
}