import Foundation

/// Where the daemon lives and how to authenticate. Resolution mirrors the web
/// client's contract (AGENTS.md): `DECANT_DAEMON_URL` env else loopback on
/// `DECANT_DAEMON_PORT` (default 4577); `DECANT_DAEMON_TOKEN` env else the
/// token file at `~/.decant/daemon.token`. UserDefaults `daemonURL` /
/// `dashboardURL` override for local tweaking without a settings UI.
public struct DaemonConfig: Sendable, Equatable {
    public let baseURL: URL
    public let token: String?
    public let dashboardURL: URL

    public static let defaultDashboard = URL(string: "http://localhost:4000")!

    /// Pure resolution from injected environment + file reader, so tests never
    /// touch the real home directory.
    public static func resolve(
        env: [String: String] = ProcessInfo.processInfo.environment,
        defaults: UserDefaults = .standard,
        readFile: (URL) -> String? = { try? String(contentsOf: $0, encoding: .utf8) },
        home: URL = FileManager.default.homeDirectoryForCurrentUser
    ) -> DaemonConfig {
        let base: URL
        if let override = defaults.string(forKey: "daemonURL"), let url = URL(string: override) {
            base = url
        } else if let fromEnv = env["DECANT_DAEMON_URL"], let url = URL(string: fromEnv) {
            base = url
        } else {
            let port = env["DECANT_DAEMON_PORT"].flatMap(Int.init) ?? 4577
            base = URL(string: "http://127.0.0.1:\(port)")!
        }

        let token: String?
        if let fromEnv = env["DECANT_DAEMON_TOKEN"], !fromEnv.isEmpty {
            token = fromEnv
        } else {
            // The daemon writes its token under DECANT_CONFIG_DIR when set
            // (crates/decant-daemon config contract), defaulting to ~/.decant.
            let configDir =
                env["DECANT_CONFIG_DIR"].map(URL.init(fileURLWithPath:))
                ?? home.appendingPathComponent(".decant")
            let raw = readFile(configDir.appendingPathComponent("daemon.token"))?
                .trimmingCharacters(in: .whitespacesAndNewlines)
            // An empty file is no token at all.
            token = (raw?.isEmpty == false) ? raw : nil
        }

        let dashboard =
            defaults.string(forKey: "dashboardURL").flatMap(URL.init(string:))
            ?? defaultDashboard

        return DaemonConfig(baseURL: base, token: token, dashboardURL: dashboard)
    }

    public init(baseURL: URL, token: String?, dashboardURL: URL = DaemonConfig.defaultDashboard) {
        self.baseURL = baseURL
        self.token = token
        self.dashboardURL = dashboardURL
    }
}
