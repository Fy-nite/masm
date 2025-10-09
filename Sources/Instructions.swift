
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
        case halt
        case nop
    }
    public func ParseInstructions(from input: String) -> [Instruction] {
        var instructions: [Instruction] = []
        for rawLine in input.split(separator: "\n", omittingEmptySubsequences: false) {
            var linecontents = String(rawLine)
            // strip comments starting with ';'
            if let semicolonRange = linecontents.firstIndex(of: ";") {
                linecontents = String(linecontents[..<semicolonRange])
            }
            let trimmed = linecontents.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmed.isEmpty else { continue }
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
    private func encodeOperand(_ s: String, labels: [String:Int], codeBase: Int) -> (mode: UInt8, value: UInt64) {
        // mode: 0 immediate, 1 register id, 2 label address
        if s.hasPrefix("#") { // label ref
            let name = String(s.dropFirst())
            if let off = labels[name] { return (2, UInt64(codeBase + off)) }
            // unresolved; placeholder 0
            return (2, 0)
        }
        if isRegister(s) {
            let id = RegisterMap.id(for: s) ?? 0
            return (1, UInt64(id))
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
        for ins in instructions {
            switch ins {
            case .label(let name):
                labels[name] = pc
            case .ret, .halt, .nop:
                pc += 1 // opcode only
            case .jmp, .je, .jne:
                pc += 1 /*opcode*/ + 1 /*mode*/ + 8 /*addr*/
            case .call:
                pc += 1 + 1 + 8
            case .push, .pop:
                pc += 1 + 1 + 8
            case .output:
                pc += 1 + (1+8) + (1+8)
            case .mov, .add, .sub, .cmp:
                pc += 1 + (1+8) + (1+8)
            }
        }

        // Header (16 bytes): magic(4), version(2), reserved(2), entry(8)
        var header = Data()
        header.append(contentsOf: [0x4D,0x41,0x53,0x49]) // 'MASI'
        writeU16LE(1, to: &header) // version
        writeU16LE(0, to: &header) // reserved
        // Entry: first instruction offset in code section relative to code base
        writeU64LE(0, to: &header)

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

        // No import/local/const/export/data for now (minimal)
        let importTable = Data()
        let localVarTable = Data()
        let constTable = Data()
        let exportTable = Data()
        let dataSection = Data()

        // Emit code section (second pass)
        var code = Data()
        for ins in instructions {
            switch ins {
            case .label:
                continue
            case .mov(let d, let s):
                code.append(Op.mov.rawValue)
                let encD = encodeOperand(d, labels: labels, codeBase: 0)
                code.append(encD.mode)
                writeU64LE(encD.value, to: &code)
                let encS = encodeOperand(s, labels: labels, codeBase: 0)
                code.append(encS.mode)
                writeU64LE(encS.value, to: &code)
            case .add(let d, let s):
                code.append(Op.add.rawValue)
                let ed = encodeOperand(d, labels: labels, codeBase: 0)
                code.append(ed.mode); writeU64LE(ed.value, to: &code)
                let es = encodeOperand(s, labels: labels, codeBase: 0)
                code.append(es.mode); writeU64LE(es.value, to: &code)
            case .sub(let d, let s):
                code.append(Op.sub.rawValue)
                let ed = encodeOperand(d, labels: labels, codeBase: 0)
                code.append(ed.mode); writeU64LE(ed.value, to: &code)
                let es = encodeOperand(s, labels: labels, codeBase: 0)
                code.append(es.mode); writeU64LE(es.value, to: &code)
            case .cmp(let a, let b):
                code.append(Op.cmp.rawValue)
                let ea = encodeOperand(a, labels: labels, codeBase: 0)
                code.append(ea.mode); writeU64LE(ea.value, to: &code)
                let eb = encodeOperand(b, labels: labels, codeBase: 0)
                code.append(eb.mode); writeU64LE(eb.value, to: &code)
            case .jmp(let l):
                code.append(Op.jmp.rawValue)
                let enc = encodeOperand(l, labels: labels, codeBase: 0)
                code.append(enc.mode); writeU64LE(enc.value, to: &code)
            case .je(let l):
                code.append(Op.je.rawValue)
                let enc = encodeOperand(l, labels: labels, codeBase: 0)
                code.append(enc.mode); writeU64LE(enc.value, to: &code)
            case .jne(let l):
                code.append(Op.jne.rawValue)
                let enc = encodeOperand(l, labels: labels, codeBase: 0)
                code.append(enc.mode); writeU64LE(enc.value, to: &code)
            case .call(let l):
                code.append(Op.call.rawValue)
                let enc = encodeOperand(l, labels: labels, codeBase: 0)
                code.append(enc.mode); writeU64LE(enc.value, to: &code)
            case .ret:
                code.append(Op.ret.rawValue)
            case .push(let v):
                code.append(Op.push.rawValue)
                let enc = encodeOperand(v, labels: labels, codeBase: 0)
                code.append(enc.mode); writeU64LE(enc.value, to: &code)
            case .pop(let d):
                code.append(Op.pop.rawValue)
                let enc = encodeOperand(d, labels: labels, codeBase: 0)
                code.append(enc.mode); writeU64LE(enc.value, to: &code)
            case .output(let p, let v):
                code.append(Op.out.rawValue)
                let ep = encodeOperand(p, labels: labels, codeBase: 0)
                code.append(ep.mode); writeU64LE(ep.value, to: &code)
                let ev = encodeOperand(v, labels: labels, codeBase: 0)
                code.append(ev.mode); writeU64LE(ev.value, to: &code)
            case .halt:
                code.append(Op.hlt.rawValue)
            case .nop:
                code.append(Op.nop.rawValue)
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

        do {
            try out.write(to: URL(fileURLWithPath: output))
            return true
        } catch {
            print("Failed to write MASI: \(error)")
            return false
        }
    }
}