import Foundation

@MainActor
class NodeService: ObservableObject {
    static let shared = NodeService()

    @Published var nodes: [PDOSNode] = []
    @Published var knownRelays: [KnownRelay] = DevicePersistence.knownRelays
    @Published var isDiscovering = false
    @Published var discoveryError: String?

    private var pollTimer: Timer?

    private init() {}

    func startPolling(baseURL: String) {
        stopPolling()
        pollTimer = Timer.scheduledTimer(withTimeInterval: 5.0, repeats: true) { [weak self] _ in
            Task { @MainActor in
                await self?.fetchNodes(baseURL: baseURL)
            }
        }
        Task {
            await fetchNodes(baseURL: baseURL)
        }
    }

    func stopPolling() {
        pollTimer?.invalidate()
        pollTimer = nil
    }

    func fetchNodes(baseURL: String) async {
        guard let url = URL(string: "\(baseURL)/api/devices") else { return }
        do {
            let (data, resp) = try await URLSession.shared.data(from: url)
            guard let http = resp as? HTTPURLResponse, http.statusCode == 200 else { return }
            if let json = try JSONSerialization.jsonObject(with: data) as? [[String: Any]] {
                let fetched = json.compactMap { dict -> PDOSNode? in
                    guard let nodeId = dict["node_id"] as? String ?? dict["id"] as? String else { return nil }
                    return PDOSNode(
                        id: nodeId,
                        nodeId: nodeId,
                        name: dict["name"] as? String ?? dict["display_name"] as? String ?? "",
                        platform: dict["platform"] as? String ?? "unknown",
                        status: dict["status"] as? String ?? "online",
                        capabilities: dict["capabilities"] as? [String] ?? [],
                        lastSeen: nil,
                        isKnown: true
                    )
                }

                var merged = fetched
                let fetchedIds = Set(fetched.map { $0.nodeId })
                for relay in knownRelays {
                    let relayId = relay.id
                    if !fetchedIds.contains(relayId) && !merged.contains(where: { $0.nodeId == relayId }) {
                        merged.append(PDOSNode(
                            id: relayId,
                            nodeId: relayId,
                            name: relay.name,
                            platform: "relay",
                            status: "disconnected",
                            capabilities: ["ping"],
                            lastSeen: relay.lastConnected,
                            isKnown: true
                        ))
                    }
                }

                nodes = merged
            }
        } catch {
            discoveryError = error.localizedDescription
        }
    }

    func refreshKnownRelays() {
        knownRelays = DevicePersistence.knownRelays
    }

    deinit {
        pollTimer?.invalidate()
        pollTimer = nil
    }
}
