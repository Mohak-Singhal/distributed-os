import Foundation

// MARK: - Unified Binary Resolution

struct DOSBinaryResolver {
    
    static func resolveBinary(_ name: String) -> URL? {
        // Priority 1: Resources directory (bundled app)
        if let resourcesPath = Bundle.main.resourcePath {
            let binaryPath = resourcesPath + "/" + name
            if FileManager.default.isExecutableFile(atPath: binaryPath) {
                return URL(fileURLWithPath: binaryPath)
            }
        }
        
        // Priority 2: Framework-specific approach
        if let binaryURL = Bundle.main.url(forResource: name, withExtension: nil, subdirectory: "Resources") {
            return binaryURL
        }
        
        // Priority 3: System PATH (fallback)
        return resolveBinaryFromPATH(name: name)
    }
    
    private static func resolveBinaryFromPATH(name: String) -> URL? {
        let which = Process()
        which.executableURL = URL(fileURLWithPath: "/usr/bin/which")
        which.arguments = [name]
        let pipe = Pipe()
        which.standardOutput = pipe
        
        do {
            try which.run()
            which.waitUntilExit()
            let data = pipe.fileHandleForReading.readDataToEndOfFile()
            if let path = String(data: data, encoding: .utf8)?
                .trimmingCharacters(in: .whitespacesAndNewlines),
                !path.isEmpty,
                FileManager.default.isExecutableFile(atPath: path) {
                return URL(fileURLWithPath: path)
            }
        } catch {
            // System PATH fallback not available
        }
        
        return nil
    }
    
    static func verifyBinaryIntegrity(forBinary name: String) -> Bool {
        guard let binaryPath = Bundle.main.path(forResource: name, ofType: nil, inDirectory: "Resources") else { return false }
        
        let isReadable = FileManager.default.isReadableFile(atPath: binaryPath)
        let isExecutable = FileManager.default.isExecutableFile(atPath: binaryPath)
        
        return isReadable && isExecutable
    }
}

// Extension for easier binary path lookup
extension Bundle {
    func binaryPath(for name: String) -> String? {
        if let url = self.url(forResource: name, withExtension: nil, subdirectory: "Resources"),
           FileManager.default.isExecutableFile(atPath: url.path) {
            return url.path
        }
        return nil
    }
    
    func isBundledBinaryAvailable(for name: String) -> Bool {
        return self.binaryPath(for: name) != nil
    }
}