import Foundation
import Testing

@testable import DecantKit

@Suite("DaemonConfig.resolve")
struct ConfigTests {
    // A unique defaults domain per call: swift-testing runs tests in parallel,
    // so a shared suite would race the override test against the rest.
    private func cleanDefaults() -> UserDefaults {
        let name = "dev.decant.menubar.tests.\(UUID().uuidString)"
        let d = UserDefaults(suiteName: name)!
        d.removePersistentDomain(forName: name)
        return d
    }

    @Test func envURLWinsOverPortDefault() {
        let cfg = DaemonConfig.resolve(
            env: ["DECANT_DAEMON_URL": "http://127.0.0.1:9999"],
            defaults: cleanDefaults(),
            readFile: { _ in nil },
            home: URL(fileURLWithPath: "/nonexistent")
        )
        #expect(cfg.baseURL.absoluteString == "http://127.0.0.1:9999")
    }

    @Test func portEnvHonoredAndDefaultIs4577() {
        let withPort = DaemonConfig.resolve(
            env: ["DECANT_DAEMON_PORT": "4599"],
            defaults: cleanDefaults(),
            readFile: { _ in nil },
            home: URL(fileURLWithPath: "/nonexistent")
        )
        #expect(withPort.baseURL.absoluteString == "http://127.0.0.1:4599")

        let bare = DaemonConfig.resolve(
            env: [:],
            defaults: cleanDefaults(),
            readFile: { _ in nil },
            home: URL(fileURLWithPath: "/nonexistent")
        )
        #expect(bare.baseURL.absoluteString == "http://127.0.0.1:4577")
    }

    @Test func tokenEnvBeatsFileAndFileIsTrimmed() {
        let viaEnv = DaemonConfig.resolve(
            env: ["DECANT_DAEMON_TOKEN": "envtoken"],
            defaults: cleanDefaults(),
            readFile: { _ in "filetoken\n" },
            home: URL(fileURLWithPath: "/home/x")
        )
        #expect(viaEnv.token == "envtoken")

        var asked: URL?
        let viaFile = DaemonConfig.resolve(
            env: [:],
            defaults: cleanDefaults(),
            readFile: { url in
                asked = url
                return "  filetoken\n"
            },
            home: URL(fileURLWithPath: "/home/x")
        )
        #expect(viaFile.token == "filetoken")
        #expect(asked?.path == "/home/x/.decant/daemon.token")
    }

    @Test func configDirEnvRelocatesTokenFile() {
        // The daemon writes daemon.token under DECANT_CONFIG_DIR when set;
        // the app must read the same place or it 401s (PR #6 review finding).
        var asked: URL?
        let cfg = DaemonConfig.resolve(
            env: ["DECANT_CONFIG_DIR": "/tmp/decant-cfg"],
            defaults: cleanDefaults(),
            readFile: { url in
                asked = url
                return "tok\n"
            },
            home: URL(fileURLWithPath: "/home/x")
        )
        #expect(asked?.path == "/tmp/decant-cfg/daemon.token")
        #expect(cfg.token == "tok")
    }

    @Test func missingOrEmptyTokenFileMeansNil() {
        let missing = DaemonConfig.resolve(
            env: [:],
            defaults: cleanDefaults(),
            readFile: { _ in nil },
            home: URL(fileURLWithPath: "/home/x")
        )
        #expect(missing.token == nil)

        let empty = DaemonConfig.resolve(
            env: [:],
            defaults: cleanDefaults(),
            readFile: { _ in "  \n" },
            home: URL(fileURLWithPath: "/home/x")
        )
        #expect(empty.token == nil)
    }

    @Test func defaultsOverrideDaemonAndDashboard() {
        let d = cleanDefaults()
        d.set("http://127.0.0.1:5000", forKey: "daemonURL")
        d.set("http://localhost:4900", forKey: "dashboardURL")
        let cfg = DaemonConfig.resolve(
            env: ["DECANT_DAEMON_URL": "http://127.0.0.1:9999"],
            defaults: d,
            readFile: { _ in nil },
            home: URL(fileURLWithPath: "/nonexistent")
        )
        #expect(cfg.baseURL.absoluteString == "http://127.0.0.1:5000")
        #expect(cfg.dashboardURL.absoluteString == "http://localhost:4900")
    }
}

@Suite("API models")
struct ModelTests {
    static let nowJSON = """
        {
          "data": {
            "today": { "sessions": 7, "messages": 100, "tool_calls": 20,
                       "input_tokens": 1, "output_tokens": 2,
                       "cache_read_tokens": 0, "cache_creation_tokens": 0,
                       "estimated_cost_usd": 12.5 },
            "active_sessions": [
              { "tool": "claude_code",
                "source_path": "/Users/x/.claude/projects/p/abc.jsonl",
                "idle_seconds": 42,
                "title": "Fix the failing auth test",
                "project": "/Users/x/oss/decant",
                "model": "claude-fable-5",
                "started_at": "2026-06-10T18:00:00Z" }
            ],
            "last_sync_at": "2026-06-10T18:05:00Z",
            "sync_in_progress": false
          },
          "meta": { "timestamp": "2026-06-10T18:05:01Z" },
          "errors": []
        }
        """

    @Test func decodesNowEnvelope() throws {
        let env = try JSONDecoder().decode(
            Envelope<Now>.self, from: Data(Self.nowJSON.utf8))
        let now = env.data
        #expect(now.today.sessions == 7)
        #expect(now.today.estimatedCostUSD == 12.5)
        #expect(now.activeSessions.count == 1)
        #expect(now.activeSessions[0].tool == "claude_code")
        #expect(now.activeSessions[0].idleSeconds == 42)
        #expect(now.activeSessions[0].displayName == "decant")
        #expect(now.lastSyncAt == "2026-06-10T18:05:00Z")
        #expect(now.syncInProgress == false)
    }

    @Test func displayNameFallsBackToSourceStem() {
        let s = NowSession(
            tool: "codex", sourcePath: "/x/rollout-2026.jsonl",
            idleSeconds: 1, title: nil, project: nil)
        #expect(s.displayName == "rollout-2026")
        #expect(s.toolLabel == "Codex")
    }

    @Test func decodesEmptyNow() throws {
        let json = """
            { "data": { "today": { "sessions": 0, "estimated_cost_usd": 0.0 },
                        "active_sessions": [], "sync_in_progress": true },
              "errors": [] }
            """
        let env = try JSONDecoder().decode(Envelope<Now>.self, from: Data(json.utf8))
        #expect(env.data.activeSessions.isEmpty)
        #expect(env.data.lastSyncAt == nil)
        #expect(env.data.syncInProgress)
    }
}
