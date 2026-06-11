import Foundation
import Testing

@testable import DecantKit

/// Scripted `now()` results; an actor for Swift 6 sendability.
actor FakeClient: NowProviding {
    private var script: [Result<Now, DaemonError>]
    private(set) var calls = 0

    init(script: [Result<Now, DaemonError>]) {
        self.script = script
    }

    func now() async throws -> Now {
        calls += 1
        guard !script.isEmpty else { return Self.empty }
        return try script.removeFirst().get()
    }

    static let empty = Now(
        today: Totals(sessions: 0, estimatedCostUSD: 0),
        activeSessions: [], lastSyncAt: nil, syncInProgress: false)

    static let busy = Now(
        today: Totals(sessions: 3, estimatedCostUSD: 1.5),
        activeSessions: [
            NowSession(
                tool: "claude_code", sourcePath: "/x/s.jsonl", idleSeconds: 5,
                title: "t", project: "/p/decant")
        ],
        lastSyncAt: "2026-06-10T18:00:00Z", syncInProgress: false)
}

/// Polls a condition without real sleeps dominating the suite.
func eventually(
    timeout: Duration = .seconds(2), _ condition: @MainActor @escaping () -> Bool
) async throws -> Bool {
    let deadline = ContinuousClock.now + timeout
    while ContinuousClock.now < deadline {
        if await MainActor.run(body: condition) { return true }
        try await Task.sleep(for: .milliseconds(10))
    }
    return await MainActor.run(body: condition)
}

@Suite("NowStore")
struct NowStoreTests {
    @MainActor
    private func makeStore(
        client: FakeClient,
        events: @escaping @Sendable () -> AsyncThrowingStream<ServerEvent, Error>
    ) -> NowStore {
        NowStore(
            client: client,
            events: events,
            debounce: .milliseconds(20),
            fallbackInterval: .milliseconds(50)
        )
    }

    @Test @MainActor func initialFetchReachesReadyAndDerivesLiveCount() async throws {
        let client = FakeClient(script: [.success(FakeClient.busy)])
        let store = makeStore(client: client) {
            AsyncThrowingStream { _ in }  // silent stream that never ends
        }
        store.start()
        defer { store.stop() }

        #expect(try await eventually { store.phase == .ready(FakeClient.busy) })
        #expect(store.liveCount == 1)
    }

    @Test @MainActor func clientErrorShowsHint() async throws {
        let client = FakeClient(script: [.failure(.unreachable)])
        let store = makeStore(client: client) { AsyncThrowingStream { _ in } }
        store.start()
        defer { store.stop() }

        #expect(
            try await eventually {
                if case .down(let hint) = store.phase { return hint.contains("daemon install") }
                return false
            })
    }

    @Test @MainActor func eventBurstCoalescesIntoOneRefetch() async throws {
        let client = FakeClient(script: [.success(FakeClient.empty), .success(FakeClient.busy)])
        let store = makeStore(client: client) {
            AsyncThrowingStream { continuation in
                for _ in 0..<5 {
                    continuation.yield(ServerEvent(name: "session_activity", data: "{}"))
                }
                // keep the stream open so no reconnect/fallback kicks in
            }
        }
        store.start()
        defer { store.stop() }

        #expect(try await eventually { store.phase == .ready(FakeClient.busy) })
        // initial fetch + exactly one debounced refetch for the burst of 5
        try await Task.sleep(for: .milliseconds(100))
        #expect(await client.calls == 2)
    }

    @Test @MainActor func deadStreamFallsBackToPolling() async throws {
        let client = FakeClient(script: [
            .failure(.unreachable), .failure(.unreachable), .success(FakeClient.busy),
        ])
        let store = makeStore(client: client) {
            AsyncThrowingStream { continuation in
                continuation.finish(throwing: DaemonError.unreachable)
            }
        }
        store.start()
        defer { store.stop() }

        // Fallback timer keeps refetching until the daemon comes back.
        #expect(try await eventually { store.phase == .ready(FakeClient.busy) })
        #expect(await client.calls >= 3)
    }
}
