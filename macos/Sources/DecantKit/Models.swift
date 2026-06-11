import Foundation

/// The daemon's response envelope (`docs/api/openapi.yaml`): `{data, meta, errors}`.
/// Clients only need `data`; `errors` is always empty on success.
public struct Envelope<T: Decodable & Sendable>: Decodable, Sendable {
    public let data: T
}

/// `GET /api/v1/analytics/now` payload.
public struct Now: Decodable, Sendable, Equatable {
    public let today: Totals
    public let activeSessions: [NowSession]
    public let lastSyncAt: String?
    public let syncInProgress: Bool

    enum CodingKeys: String, CodingKey {
        case today
        case activeSessions = "active_sessions"
        case lastSyncAt = "last_sync_at"
        case syncInProgress = "sync_in_progress"
    }

    public init(today: Totals, activeSessions: [NowSession], lastSyncAt: String?, syncInProgress: Bool) {
        self.today = today
        self.activeSessions = activeSessions
        self.lastSyncAt = lastSyncAt
        self.syncInProgress = syncInProgress
    }
}

public struct Totals: Decodable, Sendable, Equatable {
    public let sessions: Int
    public let estimatedCostUSD: Double

    enum CodingKeys: String, CodingKey {
        case sessions
        case estimatedCostUSD = "estimated_cost_usd"
    }

    public init(sessions: Int, estimatedCostUSD: Double) {
        self.sessions = sessions
        self.estimatedCostUSD = estimatedCostUSD
    }
}

/// One currently-active session (its source file changed within the daemon's
/// 120 s activity window).
public struct NowSession: Decodable, Sendable, Equatable, Identifiable {
    public let tool: String
    public let sourcePath: String
    public let idleSeconds: Int
    public let title: String?
    public let project: String?

    /// Source path is unique per live session.
    public var id: String { sourcePath }

    /// Short human label for the source tool.
    public var toolLabel: String {
        tool == "claude_code" ? "Claude" : "Codex"
    }

    /// What the popover shows: project basename, else the file stem.
    public var displayName: String {
        if let project, !project.isEmpty {
            return (project as NSString).lastPathComponent
        }
        return ((sourcePath as NSString).lastPathComponent as NSString).deletingPathExtension
    }

    enum CodingKeys: String, CodingKey {
        case tool
        case sourcePath = "source_path"
        case idleSeconds = "idle_seconds"
        case title
        case project
    }

    public init(tool: String, sourcePath: String, idleSeconds: Int, title: String?, project: String?) {
        self.tool = tool
        self.sourcePath = sourcePath
        self.idleSeconds = idleSeconds
        self.title = title
        self.project = project
    }
}
