import Foundation
import Network

/// A lightweight, single-purpose HTTP server that validates pairing tokens
/// and returns device identity.
actor PairingServer {
    enum ServerError: Error {
        case startFailed(Error)
        case invalidPort
    }

    private var listener: NWListener?
    private var _connections: [NWConnection] = []

    deinit {
        _connections.forEach { $0.cancel() }
        listener?.cancel()
    }

    /// Start the server on a random available port.
    /// - Parameter onRequest: Called with the raw HTTP request line; return a (status, body) tuple.
    /// - Returns: The port the server is listening on.
    func start(
        onRequest: @escaping @Sendable (String) -> (status: Int, body: AnyEncodable)
    ) async throws -> UInt16 {
        let params = NWParameters.tcp
        params.allowLocalEndpointReuse = true

        return try await withCheckedThrowingContinuation { continuation in
            do {
                let lis = try NWListener(using: params)
                listener = lis
                lis.stateUpdateHandler = { state in
                    switch state {
                    case .ready:
                        guard let port = lis.port?.rawValue else {
                            continuation.resume(throwing: ServerError.invalidPort)
                            return
                        }
                        continuation.resume(returning: port)
                    case .failed(let error):
                        continuation.resume(throwing: ServerError.startFailed(error))
                    default:
                        break
                    }
                }
                lis.newConnectionHandler = { connection in
                    Task { await self.handleConnection(connection, onRequest: onRequest) }
                }
                lis.start(queue: .global(qos: .userInitiated))
            } catch {
                continuation.resume(throwing: ServerError.startFailed(error))
            }
        }
    }

    func stop() {
        listener?.cancel()
        listener = nil
        _connections.forEach { $0.cancel() }
        _connections.removeAll()
    }

    private func handleConnection(
        _ connection: NWConnection,
        onRequest: @Sendable @escaping (String) -> (status: Int, body: AnyEncodable)
    ) {
        _connections.append(connection)
        connection.stateUpdateHandler = { [weak self] state in
            guard state == .ready else { return }
            Task { [weak self] in
                await self?.receive(on: connection, onRequest: onRequest)
            }
        }
        connection.start(queue: .global(qos: .userInitiated))
    }

    private func receive(
        on connection: NWConnection,
        onRequest: @Sendable @escaping (String) -> (status: Int, body: AnyEncodable)
    ) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 8192) { [weak self] data, _, _, error in
            defer {
                connection.cancel()
                Task { [weak self] in await self?._connections.removeAll { $0 === connection } }
            }

            guard let self = self, let data = data, error == nil else { return }
            let request = String(data: data, encoding: .utf8) ?? ""

            let (status, body) = onRequest(request)

            let json: Data
            do {
                json = try JSONEncoder().encode(body)
            } catch {
                let fallback = "{\"error\":\"internal error\"}".data(using: .utf8)!
                sendHTTPResponse(status: 500, data: fallback, on: connection)
                return
            }
            sendHTTPResponse(status: status, data: json, on: connection)
        }
    }

    private func sendHTTPResponse(status: Int, data: Data, on connection: NWConnection) {
        let reason: String
        switch status {
        case 200: reason = "OK"
        case 400: reason = "Bad Request"
        case 401: reason = "Unauthorized"
        default: reason = ""
        }
        let response = """
        HTTP/1.1 \(status) \(reason)\r
        Content-Type: application/json\r
        Content-Length: \(data.count)\r
        Connection: close\r
        \r
        """
        var responseData = Data(response.utf8)
        responseData.append(data)
        connection.send(content: responseData, completion: .contentProcessed { _ in })
    }
}

/// Type-erased encodable wrapper for sending arbitrary JSON responses.
struct AnyEncodable: Encodable {
    private let _encode: (Encoder) throws -> Void
    init<T: Encodable>(_ wrapped: T) {
        _encode = { try wrapped.encode(to: $0) }
    }
    func encode(to encoder: Encoder) throws {
        try _encode(encoder)
    }
}
