import Foundation
import Observation

/// The menu bar's whole world-view, derived from `/analytics/now`.
public enum NowPhase: Equatable, Sendable {
    case connecting
    case ready(Now)
    /// Daemon unreachable or auth failed; `hint` is shown in the popover.
    case down(hint: String)
}

/// Drives the icon + popover. Refresh policy (spec §1.3): any SSE event
/// debounces into one `/now` refetch; a fallback timer polls only while the
/// event stream is down; reconnects back off exponentially (capped). All
/// timing is injected so tests run instantly.
@MainActor
@Observable
public final class NowStore {
    public private(set) var phase: NowPhase = .connecting
    public private(set) var lastRefresh: Date?

    public var liveCount: Int {
        if case .ready(let now) = phase { return now.activeSessions.count }
        return 0
    }

    private let client: any NowProviding
    private let events: @Sendable () -> AsyncThrowingStream<ServerEvent, Error>
    private let sleep: @Sendable (Duration) async throws -> Void
    private let debounce: Duration
    private let fallbackInterval: Duration

    private var eventTask: Task<Void, Never>?
    private var refreshTask: Task<Void, Never>?
    private var fallbackTask: Task<Void, Never>?

    public init(
        client: any NowProviding,
        events: @escaping @Sendable () -> AsyncThrowingStream<ServerEvent, Error>,
        debounce: Duration = .seconds(1),
        fallbackInterval: Duration = .seconds(60),
        sleep: @escaping @Sendable (Duration) async throws -> Void = {
            try await Task.sleep(for: $0)
        }
    ) {
        self.client = client
        self.events = events
        self.debounce = debounce
        self.fallbackInterval = fallbackInterval
        self.sleep = sleep
    }

    /// Kick off: initial fetch + the event loop. Idempotent.
    public func start() {
        guard eventTask == nil else { return }
        refreshNow()
        eventTask = Task { await runEventLoop() }
    }

    public func stop() {
        eventTask?.cancel()
        eventTask = nil
        refreshTask?.cancel()
        refreshTask = nil
        stopFallback()
    }

    /// Immediate refetch (popover open, Refresh button).
    public func refreshNow() {
        refreshTask?.cancel()
        refreshTask = Task { await refresh() }
    }

    private func refresh() async {
        do {
            let now = try await client.now()
            phase = .ready(now)
            lastRefresh = Date()
        } catch let error as DaemonError {
            phase = .down(hint: Self.hint(for: error))
        } catch {
            phase = .down(hint: Self.hint(for: .unreachable))
        }
    }

    /// Coalesce event bursts into one refetch after `debounce`.
    private func scheduleDebouncedRefresh() {
        refreshTask?.cancel()
        refreshTask = Task {
            try? await sleep(debounce)
            guard !Task.isCancelled else { return }
            await refresh()
        }
    }

    private func runEventLoop() async {
        var attempt = 0
        while !Task.isCancelled {
            do {
                for try await _ in events() {
                    attempt = 0
                    stopFallback()
                    scheduleDebouncedRefresh()
                }
            } catch {
                // fall through to reconnect
            }
            guard !Task.isCancelled else { return }
            // Stream is down: keep `/now` fresh by timer while we back off.
            startFallback()
            let backoff = Duration.seconds(min(60, 1 << min(attempt, 6)))
            attempt += 1
            try? await sleep(backoff)
        }
    }

    private func startFallback() {
        guard fallbackTask == nil else { return }
        fallbackTask = Task {
            while !Task.isCancelled {
                try? await sleep(fallbackInterval)
                guard !Task.isCancelled else { return }
                await refresh()
            }
        }
    }

    private func stopFallback() {
        fallbackTask?.cancel()
        fallbackTask = nil
    }

    static func hint(for error: DaemonError) -> String {
        switch error {
        case .unreachable:
            return "Daemon not running — start it with `decant daemon install`."
        case .unauthorized:
            return "Token mismatch — check ~/.decant/daemon.token."
        case .server(let status):
            return "Daemon error (HTTP \(status)) — see `decant daemon logs`."
        case .decoding:
            return "Unexpected daemon response — version mismatch?"
        }
    }
}
