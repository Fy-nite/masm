public  class Config
{
    public let verbose: Bool = false
    public static let version: String = "0.1.0"
    public let OutputFileName: String = "out.masi"
    public let InputFileName: String = ""
    public let Optimize: Bool = false
    public var Debug: Bool = false
     nonisolated(unsafe) public static let instance = Config()
    // apparently this is a constructor?
    init() {
        // set default values
    }
}