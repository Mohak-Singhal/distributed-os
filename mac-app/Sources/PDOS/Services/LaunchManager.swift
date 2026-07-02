import Foundation
import AppKit

class RunningProcess {
    private(set) var pid: Int32?
    private(set) var name: String
    private(set) var executable: String
    private(set) var args: [String]
    private var process: Process?
    private var backgroundThread: Thread?

    init(name: String, executable: String, args: [String]) {
        self.name = name
        self.executable = executable
        self.args = args
    }

    func start(completion: @escaping (Int32, Bool) -> Void) {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: executable)
        if !args.isEmpty {
            process.arguments = args
        }

        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = pipe

        backgroundThread = Thread {
            autoreleasepool {
                do {
                    try process.run()
                    self.pid = process.processIdentifier
                    pipe.fileHandleForReading.readDataToEndOfFile()
                    completion(process.processIdentifier, true)
                } catch {
                    completion(0, false)
                }
            }
        }
        backgroundThread?.start()
        self.process = process
    }

    func stop() {
        process?.terminate()
        backgroundThread?.cancel()
        backgroundThread = nil
    }
}
