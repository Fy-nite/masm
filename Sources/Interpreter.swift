import Foundation

public final class Interpreter {
    public struct State {
        // 64-bit registers (by ID); using RegisterMap for known names
        public var regs: [UInt16: UInt64] = [:]
        public var flags: (ZF: Bool, SF: Bool, CF: Bool, OF: Bool) = (false, false, false, false)
        public var rip: UInt64 = 0
        public var stack: [UInt64] = []
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
        var pc = 0
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
            case .out:
                let p = getOperand(code, &pc, &state)
                let v = getOperand(code, &pc, &state)
                if p == 2 {
                    // stderr
                    let msg = "\(v)\n"
                    if let data = msg.data(using: .utf8) {
                        try? FileHandle.standardError.write(contentsOf: data)
                    }
                } else {
                    // stdout (default port 0/1)
                    Swift.print(v)
                }
            }
        }
    }
}
