
import Foundation
public class Instructions {
    public static func printUsage() -> Void {
        let usage = """
        Usage: valamasm [options] <input file>
        
        Options:
          -o <file>       Specify output file (default: a.masi)
          -g             Generate debug information
          -h, --help    Show this help message
        """
        print(usage)
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
        case nop
    }
    public func ParseInstructions(from input: String) -> [Instruction] {
        for line in input.split(separator: "\n") {
 
            let linecontents = String(line)
            let trimmed = linecontents.trimmingCharacters(in: .whitespacesAndNewlines)
            let components = trimmed.split(separator: " ", maxSplits: 1).map { String($0) }
            guard !trimmed.isEmpty && !trimmed.hasPrefix(";") else { continue }
            guard let mnemonic = components.first else { continue }
            let operands = components.count > 1 ? components[1].split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) } : []
            switch mnemonic.lowercased() {
            case "mov" where operands.count == 2:
                // Handle mov instruction
                let instruction = Instruction.mov(destination: operands[0], source: operands[1])
                print(instruction)
            case "add" where operands.count == 2:
                // Handle add instruction
                let instruction = Instruction.add(destination: operands[0], source: operands[1])
                print(instruction)
            case "sub" where operands.count == 2:
                // Handle sub instruction
                let instruction = Instruction.sub(destination: operands[0], source: operands[1])
                print(instruction)
            case "jmp" where operands.count == 1:
                // Handle jmp instruction
                let instruction = Instruction.jmp(label: operands[0])
                print(instruction)
            case "cmp" where operands.count == 2:
                // Handle cmp instruction
                let instruction = Instruction.cmp(operand1: operands[0], operand2: operands[1])
                print(instruction)
            case "je" where operands.count == 1:
                // Handle je instruction
                let instruction = Instruction.je(label: operands[0])
                print(instruction)
            case "jne" where operands.count == 1:
                // Handle jne instruction
                let instruction = Instruction.jne(label: operands[0])
                print(instruction)
            case "label" where operands.count == 1:
                // Handle label definition
                let instruction = Instruction.label(name: operands[0])
                print(instruction)
            case "call" where operands.count == 1:
                // Handle call instruction
                let instruction = Instruction.call(function: operands[0])
                print(instruction)
            case "ret" where operands.isEmpty:
                // Handle ret instruction
                let instruction = Instruction.ret
                print(instruction)
            case "push" where operands.count == 1:
                // Handle push instruction
                let instruction = Instruction.push(value: operands[0])
                print(instruction)
            case "pop" where operands.count == 1:
                // Handle pop instruction
                let instruction = Instruction.pop(destination: operands[0])
                print(instruction)
            case "nop" where operands.isEmpty:
                // Handle nop instruction
                let instruction = Instruction.nop
                print(instruction)  
            default:
                print("Error: Unrecognized instruction or wrong number of operands: \(trimmed)")
            }
        }
        return []
    }
}