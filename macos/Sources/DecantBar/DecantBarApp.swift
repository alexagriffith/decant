import DecantKit
import SwiftUI

@main
struct DecantBarApp: App {
    /// Resolved once at launch; every component (client, SSE, popover links)
    /// sees the same values — no per-view re-resolution drift.
    private let config: DaemonConfig
    @State private var store: NowStore

    init() {
        let config = DaemonConfig.resolve()
        self.config = config
        let client = DaemonClient(config: config)
        let sse = SSEClient()
        let store = NowStore(
            client: client,
            events: { sse.events(config: config) }
        )
        _store = State(initialValue: store)
        store.start()
    }

    var body: some Scene {
        MenuBarExtra {
            PopoverView(store: store, dashboardURL: config.dashboardURL)
        } label: {
            Image(systemName: iconName)
                .accessibilityLabel("Decant")
        }
        .menuBarExtraStyle(.window)
    }

    private var iconName: String {
        switch store.phase {
        case .down: return "drop.triangle"
        case .ready where store.liveCount > 0: return "drop.fill"
        case .ready, .connecting: return "drop"
        }
    }
}
