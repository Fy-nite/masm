class Dbg {


    static func print(_ message: String) {
        if Config.instance.Debug {
            Swift.print("[DEBUG] \(message)")
        }
    }
}

// Adapter to satisfy TextOutputStream for printing from interpreter
struct TextOutputStreamAdapter: TextOutputStream {
    mutating func write(_ string: String) {
        Swift.print(string)
    }
}