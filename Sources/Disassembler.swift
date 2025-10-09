import Foundation

public struct Disassembler {
    public struct MASIFile {
        var version: UInt16
        var entry: UInt64
        var labelMap: [UInt64: String] // codeOffset -> name
        var code: Data
        var sectionSizes: [String: Int]
    }

    private func readU16LE(_ data: Data, _ offset: inout Int) -> UInt16 {
        var v: UInt16 = 0
        v |= UInt16(data[offset + 0])
        v |= UInt16(data[offset + 1]) << 8
        offset += 2
        return v
    }
    private func readU32LE(_ data: Data, _ offset: inout Int) -> UInt32 {
        var v: UInt32 = 0
        for i in 0..<4 { v |= UInt32(data[offset + i]) << (8 * i) }
        offset += 4
        return v
    }
    private func readU64LE(_ data: Data, _ offset: inout Int) -> UInt64 {
        var v: UInt64 = 0
        for i in 0..<8 { v |= UInt64(data[offset + i]) << (8 * i) }
        offset += 8
        return v
    }

    public func load(path: String) throws -> MASIFile {
        let bytes = try Data(contentsOf: URL(fileURLWithPath: path))
        var off = 0
        // Header
        guard bytes.count >= 16 else { throw NSError(domain: "MASI", code: 1, userInfo: [NSLocalizedDescriptionKey: "File too small"]) }
        let magic = bytes.subdata(in: 0..<4)
        off = 4
        guard String(data: magic, encoding: .utf8) == "MASI" else { throw NSError(domain: "MASI", code: 2, userInfo: [NSLocalizedDescriptionKey: "Bad magic"]) }
        let version = readU16LE(bytes, &off)
        _ = readU16LE(bytes, &off) // reserved
        let entry = readU64LE(bytes, &off)
        // Chunks in order
        func readChunk() -> Data {
            let sz = Int(readU32LE(bytes, &off))
            let d = bytes.subdata(in: off..<(off+sz))
            off += sz
            return d
        }
    let importTable = readChunk()
    let localVarTable = readChunk()
        let labelTable = readChunk()
    let constTable = readChunk()
    let dataTable = readChunk()
    let exportTable = readChunk()
        let code = readChunk()

        // Parse labels: count + [nameLen u16][name][addr u64]
        var ltOff = 0
        var codeOffsetToName: [UInt64: String] = [:]
        if labelTable.count >= 2 {
            let count = Int(readU16LE(labelTable, &ltOff))
            for _ in 0..<count {
                if ltOff + 2 > labelTable.count { break }
                let nameLen = Int(readU16LE(labelTable, &ltOff))
                if ltOff + nameLen + 8 > labelTable.count { break }
                let nameData = labelTable.subdata(in: ltOff..<(ltOff+nameLen))
                ltOff += nameLen
                let name = String(data: nameData, encoding: .utf8) ?? "label_?"
                let addr = readU64LE(labelTable, &ltOff)
                codeOffsetToName[addr] = name
            }
        }
        let sizes = [
            "import": importTable.count,
            "locals": localVarTable.count,
            "labels": labelTable.count,
            "const": constTable.count,
            "data": dataTable.count,
            "export": exportTable.count,
            "code": code.count
        ]
        return MASIFile(version: version, entry: entry, labelMap: codeOffsetToName, code: code, sectionSizes: sizes)
    }

    public func disassemble(_ masi: MASIFile) -> String {
        var out: [String] = []
        // Emit labels at their offsets when encountered
        var pc = 0
        let code = masi.code
        while pc < code.count {
            if let name = masi.labelMap[UInt64(pc)] {
                out.append("LBL \(name)")
            }
            let opcode = code[pc]; pc += 1
            func readOp() -> (mode: UInt8, value: UInt64) {
                let mode = code[pc]; pc += 1
                let val = readU64LE(code, &pc)
                return (mode, val)
            }
            switch opcode {
            case 0x01: // MOV
                let d = readOp(); let s = readOp()
                out.append("MOV \(fmtOp(d, masi)) \(fmtOp(s, masi))")
            case 0x02: // ADD
                let d = readOp(); let s = readOp()
                out.append("ADD \(fmtOp(d, masi)) \(fmtOp(s, masi))")
            case 0x03: // SUB
                let d = readOp(); let s = readOp()
                out.append("SUB \(fmtOp(d, masi)) \(fmtOp(s, masi))")
            case 0x10: // JMP
                let t = readOp(); out.append("JMP \(fmtOp(t, masi))")
            case 0x11: // CMP
                let a = readOp(); let b = readOp(); out.append("CMP \(fmtOp(a, masi)) \(fmtOp(b, masi))")
            case 0x12: // JE
                let t = readOp(); out.append("JE \(fmtOp(t, masi))")
            case 0x13: // JNE
                let t = readOp(); out.append("JNE \(fmtOp(t, masi))")
            case 0x20: // CALL
                let t = readOp(); out.append("CALL \(fmtOp(t, masi))")
            case 0x21: // RET
                out.append("RET")
            case 0x30: // PUSH
                let v = readOp(); out.append("PUSH \(fmtOp(v, masi))")
            case 0x31: // POP
                let d = readOp(); out.append("POP \(fmtOp(d, masi))")
            case 0x40: // OUT
                let p = readOp(); let v = readOp(); out.append("OUT \(fmtOp(p, masi)) \(fmtOp(v, masi))")
            case 0x00:
                out.append("NOP")
            case 0xFF:
                out.append("HLT")
            default:
                out.append("; DB 0x\(String(format: "%02X", opcode))  ; Unknown opcode")
            }
        }
        return out.joined(separator: "\n")
    }

    private func fmtOp(_ op: (mode: UInt8, value: UInt64), _ masi: MASIFile) -> String {
        switch op.mode {
        case 0: // immediate
            return String(op.value)
        case 1: // register id => name
            let id = UInt16(truncatingIfNeeded: op.value)
            if let name = RegisterMap.name(for: id) { return name }
            return "REG\(id)"
        case 2: // label by address (code offset)
            if let name = masi.labelMap[op.value] { return "#\(name)" }
            return String(format: "#0x%llX", op.value)
        default:
            return String(op.value)
        }
    }
}

public extension Disassembler {
    func dump(_ masi: MASIFile) -> String {
        var lines: [String] = []
        lines.append("MASI Dump")
        lines.append("- Version: \(masi.version)")
        lines.append(String(format: "- Entry: 0x%016llX", masi.entry))
        lines.append("- Sections:")
        for key in ["import","locals","labels","const","data","export","code"] {
            if let sz = masi.sectionSizes[key] { lines.append("  - \(key): \(sz) bytes") }
        }
        if !masi.labelMap.isEmpty {
            lines.append("- Labels:")
            for (off, name) in masi.labelMap.sorted(by: { $0.key < $1.key }) {
                lines.append(String(format: "  - 0x%llX: %s", off, name))
            }
        }
        return lines.joined(separator: "\n")
    }
}
