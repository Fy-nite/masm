public class vm
{
    public var registers: [String: Int] = ["RAX": 0, "RBX": 0, "RCX": 0, "RDX": 0, "RSP": 0, "RBP": 0, "RSI": 0, "RDI": 0, "RIP": 0, "ZF": 0, "SF": 0, "OF": 0]
    public var memory: [Int: Int] = [:]
    public var callStack: [Int] = []
    public var labels: [String: Int] = [:]
    public init() {
        // set default values
    }
    public func run(instructions: [Instructions.Instruction]) {
       

    }
}