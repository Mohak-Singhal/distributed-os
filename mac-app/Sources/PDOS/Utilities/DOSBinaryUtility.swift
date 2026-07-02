import Foundation

// MARK: - Binary Resolution Utilities

func resolveDOSBinary() -> URL {
    return resolveBinary("dos")
}

func resolveRelayBinary() -> URL {
    return resolveBinary("dos-relay")
}

func resolveAdbBinary() -> URL {
    return resolveBinary("adb")
}

private func resolveBinary(_ name: String) -> URL {
    // First try to find the binary in the app's Resources directory
    let resourcesPath = Bundle.main.bundlePath + "/Contents/Resources/"
    let resourcesFile = resourcesPath + name
    if FileManager.default.isExecutableFile(atPath: resourcesFile) {
        return URL(fileURLWithPath: resourcesFile)
    }
    
    // Next try the app bundle's Resources directory
    if let bundled = Bundle.main.url(forResource: name, withExtension: nil, subdirectory: "Resources") {
        return bundled
    }
    
    // Then try the traditional locations
    let which = Process()
    which.executableURL = URL(fileURLWithPath: "/usr/bin/which")
    which.arguments = [name]
    let pipe = Pipe()
    which.standardOutput = pipe
    if (try? which.run()) != nil {
        which.waitUntilExit()
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        if let path = String(data: data, encoding: .utf8)?
            .trimmingCharacters(in: .whitespacesAndNewlines),
            !path.isEmpty, 
            FileManager.default.isExecutableFile(atPath: path) {
            return URL(fileURLWithPath: path)
        }
    }
    
    return URL(fileURLWithPath: "/usr/local/bin/\(name)")
}
