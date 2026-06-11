# macOS Menu Bar App — Design

**Goal:** A native, glanceable surface for the archive's "now": a menu bar icon
that lights up while an agent session is live, and a popover with today's
totals, the active sessions, and sync state — one `/analytics/now` call plus the
`session_activity` SSE stream, both shipped for exactly this client in PR #4.

**Approach:** A SwiftPM package at `macos/` with a tested library core
(**DecantKit**: config discovery, API client, SSE parser, observable store) and
a thin SwiftUI **MenuBarExtra** shell (**DecantBar**, display name "Decant").
No Xcode project — a bundle script assembles `Decant.app` from the SwiftPM
build, keeping everything diff-able and agent-friendly. Zero third-party
dependencies; URLSession for HTTP and SSE.

**Tech stack:** Swift 6 (strict concurrency), SwiftUI `MenuBarExtra`
(macOS 14+), swift-testing for DecantKit, `LSUIElement` app bundle with ad-hoc
codesign. New paths-gated GitHub Actions workflow on a macOS runner.

---

## 1. Decisions locked

1. **Monorepo, `macos/` top level** (precedent: `crates/`, `web/`). Package
   `decant-menubar`; targets `DecantKit` (library, tested) and `DecantBar`
   (executable, thin). The bundle is `Decant.app`.
2. **Config resolution mirrors the web client exactly** (CLAUDE.md contract):
   base URL from `DECANT_DAEMON_URL` env else `http://127.0.0.1:4577`
   (`DECANT_DAEMON_PORT` honored), token from `DECANT_DAEMON_TOKEN` env else
   `~/.decant/daemon.token`. A `UserDefaults` override for the daemon and
   dashboard URLs exists for headless tweaking; no settings UI in v1.
   Sandboxing is explicitly off — the app must read `~/.decant`.
3. **Refresh policy: SSE-driven refetch, not payload assembly.** Any
   `session_activity` / `archive_updated` event debounces (1 s) into a fresh
   `GET /analytics/now`; popover-open also refetches; a 60 s timer is the only
   poll, and only while the SSE stream is down (reconnect with capped backoff).
   The daemon's 15 s keep-alives hold the stream; events carry no state the
   client must merge — `/now` is always the truth.
4. **Icon states**: SF Symbol `drop` (idle, template) / `drop.fill` (≥1 active
   session) / `drop.triangle` (daemon unreachable). Chosen for brand fit
   (decanting) and availability since macOS 11 — no symbol-availability risk.
5. **Popover content (v1, in order):** today line (`N sessions · $X.XX`),
   active sessions (project basename or path stem, tool tag, idle seconds,
   "live" tint), then actions: Open Dashboard (`http://localhost:4000`,
   UserDefaults-overridable) and Quit. **No sync state and no refresh button**
   (product call, 2026-06-10): syncing is daemon plumbing — SSE plus
   popover-open refetch make the data just work. Daemon-down state shows the
   bind hint and `decant daemon install` as copyable text.
6. **No Dock icon** (`LSUIElement = true`); login-item registration via
   `SMAppService.mainApp.register()` behind a popover toggle is **v1.1** —
   v1 ships launch-on-demand (`just menubar`).
7. **CI:** `.github/workflows/macos.yml`, `runs-on: macos-15`, triggered on
   `macos/**` paths (public repo → free runners): `swift build` + `swift test`
   from `macos/`.

## 2. Components

- `DecantKit/DaemonConfig.swift` — pure resolution from injected
  `[String:String]` env + file-reader closure; testable without touching the
  real home dir.
- `DecantKit/Models.swift` — `Envelope<T>`, `Now`, `NowSession`, `Totals`
  matching `docs/api/openapi.yaml` field names via `CodingKeys` (snake_case).
- `DecantKit/DaemonClient.swift` — `now() async throws -> Now`,
  `health() async throws -> Bool`; bearer header; tiny typed errors
  (`.unreachable`, `.unauthorized`, `.decoding`).
- `DecantKit/SSE.swift` — `SSELineParser` (pure: feed lines, emit
  `ServerEvent(name:data:)`, ignore comments/keep-alives) +
  `SSEClient.events(url:token:)` returning an `AsyncThrowingStream` over
  `URLSession.bytes`.
- `DecantKit/NowStore.swift` — `@MainActor @Observable`; state
  `phase: .connecting | .ready(Now) | .down(message)`, `liveCount`,
  `lastRefresh`; consumes an injected client + event stream; owns debounce,
  reconnect backoff, and the 60 s fallback timer (injected clock for tests).
- `DecantBar/DecantBarApp.swift` + `PopoverView.swift` — MenuBarExtra(window
  style), icon switch on store state, the §1.5 layout; `NSWorkspace.shared.open`
  for the dashboard.
- `macos/bundle.sh` — release build → `Decant.app` (Info.plist with
  `LSUIElement`, `CFBundleIdentifier dev.decant.menubar`, min macOS 14) +
  ad-hoc `codesign`. `just menubar` = bundle + open.

## 3. Error handling

Unreachable daemon → `.down` with the install hint; 401 → `.down("token
mismatch — check ~/.decant/daemon.token")`; SSE drop → backoff reconnect
(1 s · 2ⁿ capped at 60 s) while the fallback timer keeps `/now` fresh; malformed
SSE frames are skipped (the parser never throws mid-stream).

## 4. Testing

swift-testing on DecantKit only (the shell is declarative):

- config: env-over-file precedence, port default, missing-token case
- models: decode fixtures for `/now` (active + empty variants) straight from
  OpenAPI-shaped JSON literals
- SSE parser: named events, multi-`data:` frames, `:` keep-alives ignored,
  partial-line buffering across chunks
- NowStore: event→debounced-refetch, reconnect backoff progression,
  down→ready transition, liveCount derivation (injected fake client/clock)

Manual verification on this machine: `swift test`, then `just menubar` against
the running daemon — icon flips when a Claude session writes, popover numbers
match `decant files`-era totals, dashboard button opens the web app.

## 5. Out of scope (v1)

Login-item toggle (v1.1), notifications/alerts, hotspots or recommendations in
the popover, Sparkle/updates, code-signing identity + notarization (ad-hoc
only; it's a local personal app), App Store sandboxing (incompatible with
reading `~/.decant`), Codex-vs-Claude per-tool filtering.
