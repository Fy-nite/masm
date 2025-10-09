
class Program {
    public static func main() -> Void {
        // get args
        let args = CommandLine.arguments[1...] // skip executable name
        for arg in args {
            switch arg {
            case "--help", "-h":
                Instructions.printUsage()
                return
            default:
                print("Unknown argument: \(arg)")
                Instructions.printUsage()
                return
            }
        }
    }
}
// why?
Program.main();