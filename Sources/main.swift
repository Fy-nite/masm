import Foundation
// Set the PYTHON_LIBRARY environment variable with the path to the local python library based on os


struct ParsedArguments {
    var showHelp: Bool = false
    var showVersion: Bool = false
    var debugMode: Bool = false
    var masmFile: String? = nil
    var outputFile: String? = nil
    var disassemble: Bool = false
    var dump: Bool = false
    var run: Bool = false
    var unknownArg: String? = nil
}

func parseArguments(_ args: ArraySlice<String>) -> ParsedArguments {
    var result: ParsedArguments = ParsedArguments()
    
    var iterator = args.makeIterator()
    while let arg: String = iterator.next() {
        switch arg {
        case "--help", "-h":
            result.showHelp = true
            return result
        case "--version", "-v":
            result.showVersion = true
            return result
        case "--debug", "-d":
            result.debugMode = true
        // continue parsing in case there are more args
        case "-o":
            if let next = iterator.next() {
                result.outputFile = next
            } else {
                result.unknownArg = "-o requires a file path"
                return result
            }
        case "--disasm", "-x":
            result.disassemble = true
        case "--dump", "-t":
            result.dump = true
        case "--run", "-r":
            result.run = true
        default:
            if arg.hasSuffix(".masm") {
                result.masmFile = arg
                // continue parsing in case there are more args
            } else if arg.hasSuffix(".masi") {
                result.masmFile = arg // reuse field for input path
                // default to disassemble unless --run or --dump provided
                if !result.run && !result.dump { result.disassemble = true }
            } else {
                result.unknownArg = arg
                return result
            }
        }
    }
    return result
}

class Program {
    public static func main() {
        let args: ArraySlice<String> = CommandLine.arguments[1...]
        let parsed: ParsedArguments = parseArguments(args)

        if parsed.showHelp {
            Instructions.printUsage()
            return
        }
        if parsed.showVersion {
            print("valamasm version \(Config.version)")
            return
        }
        if parsed.debugMode {
            Config.instance.Debug = true
            print("Debug mode is now enabled.")
        }
        for arg: String in parsed.unknownArg != nil ? [parsed.unknownArg!] : [] {
            print("Unknown argument: \(arg)")
            Instructions.printUsage()
            return
        }
     
        if let unknown: String = parsed.unknownArg {
            print("Unknown argument: \(unknown)")
            Instructions.printUsage()
            return
        }
        if !parsed.showHelp && !parsed.showVersion && parsed.masmFile == nil {
            print("No input file specified.")
            Instructions.printUsage()
            return
        }
        
        // If we reach here, we have a valid masm file to process
        if let file: String = parsed.masmFile {
            do {
                if parsed.dump {
                    let dis = Disassembler()
                    let masi = try dis.load(path: file)
                    let text = dis.dump(masi)
                    if let out = parsed.outputFile {
                        try text.write(to: URL(fileURLWithPath: out), atomically: true, encoding: .utf8)
                        print("Wrote dump to \(out)")
                    } else {
                        print(text)
                    }
                } else if parsed.run {
                    let dis = Disassembler()
                    let masi = try dis.load(path: file)
                    let interp = Interpreter()
                    interp.run(masi: masi)
                } else if parsed.disassemble {
                    // Disassemble MASI
                    let dis = Disassembler()
                    let masi = try dis.load(path: file)
                    let asm = dis.disassemble(masi)
                    if let out = parsed.outputFile {
                        try asm.write(to: URL(fileURLWithPath: out), atomically: true, encoding: .utf8)
                        print("Wrote disassembly to \(out)")
                    } else {
                        print(asm)
                    }
                } else {
                    // Assemble MASM
                    let fileContents: String = try String(contentsOfFile: file, encoding: .utf8)
                    let instructions: Instructions = Instructions()
                    let parsedInstructions: [Instructions.Instruction] = instructions.ParseInstructions(from: fileContents)
                    // Compile to MASI format
                    let outPath: String = parsed.outputFile ?? Config.instance.OutputFileName
                    let ok = instructions.CompileInstructions(to: outPath, from: parsedInstructions)
                    if ok {
                        print("Wrote MASI to \(outPath)")
                    } else {
                        print("Failed to compile MASI output")
                    }
                }
            } catch {
                print("Error reading file: \(error)")
                return
            }
        }

        
    }
}


Program.main()
