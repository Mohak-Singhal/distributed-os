import Foundation

struct FileTransferService {

    static func sendFile(url: URL, deviceID: String, remotePath: String = "~/Downloads", completion: ((Bool, String?) -> Void)? = nil) {
        let task = Process()
        task.executableURL = resolveDOSBinary()
        task.arguments = ["send-file", "--http", "auto", url.path, url.lastPathComponent]
        task.standardOutput = FileHandle.nullDevice
        task.standardError = FileHandle.nullDevice

        task.terminationHandler = { process in
            let success = process.terminationStatus == 0
            completion?(success, success ? nil : "Process exited with code \(process.terminationStatus)")
        }

        do {
            try task.run()
        } catch {
            completion?(false, error.localizedDescription)
        }
    }

    static func sendFiles(_ urls: [URL], deviceID: String, remotePath: String = "~/Downloads", completion: ((Int, Int, String?) -> Void)? = nil) {
        var successCount = 0
        var failCount = 0
        let group = DispatchGroup()

        for url in urls {
            group.enter()
            sendFile(url: url, deviceID: deviceID, remotePath: remotePath) { success, error in
                if success {
                    successCount += 1
                } else {
                    failCount += 1
                }
                group.leave()
            }
        }

        group.notify(queue: .main) {
            completion?(successCount, failCount, nil)
        }
    }

    static func listDevices(completion: @escaping ([Device]) -> Void) {
        discoverDevices { devices in
            DispatchQueue.main.async { completion(devices) }
        }
    }

    static func parseKnownDevices() -> [Device] {
        let semaphore = DispatchSemaphore(value: 0)
        var result: [Device] = []
        discoverDevices { devices in
            result = devices
            semaphore.signal()
        }
        semaphore.wait()
        return result
    }

    private static func discoverDevices(completion: @escaping ([Device]) -> Void) {
        DispatchQueue.global().async {
            let task = Process()
            task.executableURL = resolveDOSBinary()
            task.arguments = ["discover"]
            let pipe = Pipe()
            task.standardOutput = pipe
            task.standardError = FileHandle.nullDevice
            let semaphore = DispatchSemaphore(value: 0)

            task.terminationHandler = { _ in
                let data = pipe.fileHandleForReading.readDataToEndOfFile()
                let output = String(data: data, encoding: .utf8) ?? ""
                let devices = parseDiscoverOutput(output)
                completion(devices)
                semaphore.signal()
            }

            do {
                try task.run()
                _ = semaphore.wait(timeout: .now() + 15)
            } catch {
                completion([])
            }
        }
    }

    static func parseDiscoverOutput(_ output: String) -> [Device] {
        var devices: [Device] = []
        let lines = output.components(separatedBy: .newlines)

        for line in lines {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            guard let firstChar = trimmed.first, firstChar.isNumber || firstChar == "." else { continue }
            guard let arrow = trimmed.range(of: " — ") else { continue }

            let left = String(trimmed[..<arrow.lowerBound]).trimmingCharacters(in: .whitespaces)
            let right = String(trimmed[arrow.upperBound...]).trimmingCharacters(in: .whitespaces)

            let leftStripped = left
                .replacingOccurrences(of: "^\\d+\\.\\s*", with: "", options: .regularExpression)
                .trimmingCharacters(in: .whitespaces)

            guard let lpStart = leftStripped.lastIndex(of: "("),
                  let lpEnd = leftStripped.lastIndex(of: ")"),
                  lpStart < lpEnd else { continue }

            let ipPort = String(leftStripped[leftStripped.index(after: lpStart)..<lpEnd])
                .trimmingCharacters(in: .whitespaces)

            guard let rpStart = right.lastIndex(of: "("),
                  let rpEnd = right.lastIndex(of: ")"),
                  rpStart < rpEnd else { continue }

            let displayName = String(right[..<rpStart]).trimmingCharacters(in: .whitespaces)
            let platform = String(right[right.index(after: rpStart)..<rpEnd])
                .trimmingCharacters(in: .whitespaces)

            devices.append(Device(id: ipPort, name: displayName, platform: platform))
        }

        return devices
    }
}

struct Device: Identifiable, Hashable {
    let id: String
    let name: String
    let platform: String

    func hash(into hasher: inout Hasher) { hasher.combine(id) }
    static func == (lhs: Device, rhs: Device) -> Bool { lhs.id == rhs.id }
}
