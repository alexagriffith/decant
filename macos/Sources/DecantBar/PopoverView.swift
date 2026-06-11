import DecantKit
import SwiftUI

/// The glanceable popover: today line, live sessions, actions.
struct PopoverView: View {
    let store: NowStore
    let dashboardURL: URL

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            header

            switch store.phase {
            case .connecting:
                Text("Connecting to the daemon…")
                    .foregroundStyle(.secondary)
            case .down(let hint):
                downView(hint)
            case .ready(let now):
                readyView(now)
            }

            Divider()
            actions
        }
        .padding(12)
        .frame(width: 300)
        .onAppear { store.refreshNow() }
    }

    private var header: some View {
        HStack {
            Text("Decant").font(.headline)
            Spacer()
        }
    }

    private func downView(_ hint: String) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Label("Daemon unreachable", systemImage: "exclamationmark.triangle")
                .foregroundStyle(.orange)
            Text(hint)
                .font(.caption)
                .foregroundStyle(.secondary)
                .textSelection(.enabled)
        }
    }

    @ViewBuilder
    private func readyView(_ now: Now) -> some View {
        HStack(spacing: 4) {
            Text("Today:")
                .foregroundStyle(.secondary)
            Text("\(now.today.sessions) sessions")
            Text("·").foregroundStyle(.secondary)
            Text(now.today.estimatedCostUSD, format: .currency(code: "USD"))
                .monospacedDigit()
        }
        .font(.callout)

        if now.activeSessions.isEmpty {
            Text("No live sessions")
                .font(.callout)
                .foregroundStyle(.secondary)
        } else {
            VStack(alignment: .leading, spacing: 6) {
                ForEach(now.activeSessions) { session in
                    HStack(spacing: 6) {
                        Circle().fill(.green).frame(width: 7, height: 7)
                        VStack(alignment: .leading, spacing: 1) {
                            Text(session.displayName)
                                .font(.callout)
                                .lineLimit(1)
                            if let title = session.title {
                                Text(title)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(1)
                            }
                        }
                        Spacer()
                        Text(session.toolLabel)
                            .font(.caption2)
                            .padding(.horizontal, 5)
                            .padding(.vertical, 1)
                            .background(.quaternary, in: Capsule())
                        Text("\(session.idleSeconds)s")
                            .font(.caption)
                            .monospacedDigit()
                            .foregroundStyle(.secondary)
                    }
                }
            }
        }

    }

    // No sync state, no refresh button: SSE keeps the data live and opening
    // the popover refetches — it should just work.
    private var actions: some View {
        HStack {
            Button("Open Dashboard") {
                NSWorkspace.shared.open(dashboardURL)
            }
            Spacer()
            Button("Quit") { NSApplication.shared.terminate(nil) }
        }
        .font(.callout)
        .buttonStyle(.borderless)
    }
}
