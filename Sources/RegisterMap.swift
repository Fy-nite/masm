import Foundation

public enum RegisterMap {
    // Stable, reversible register ID mapping
    static let nameToId: [String: UInt16] = {
        var m: [String: UInt16] = [:]
        func add(_ name: String, _ id: UInt16) { m[name] = id }
        add("RAX", 1); add("RBX", 2); add("RCX", 3); add("RDX", 4)
        add("RSI", 5); add("RDI", 6); add("RBP", 7); add("RSP", 8)
        add("RIP", 9)
        // General R0..R15
        for i in 0...15 { add("R\(i)", UInt16(32 + i)) }
        // Flags (reserved)
        add("ZF", 100); add("SF", 101); add("OF", 102)
        // Floating-point FPR0..FPR15
        for i in 0...15 { add("FPR\(i)", UInt16(200 + i)) }
        return m
    }()
    static let idToName: [UInt16: String] = {
        var m: [UInt16: String] = [:]
        for (k, v) in nameToId { m[v] = k }
        return m
    }()

    public static func id(for name: String) -> UInt16? {
        nameToId[name.uppercased()]
    }
    public static func name(for id: UInt16) -> String? {
        idToName[id]
    }
}
