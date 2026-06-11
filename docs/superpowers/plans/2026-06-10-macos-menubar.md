# macOS Menu Bar App Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> *Note:* executed inline by the authoring session. DecantKit tasks carry full
> code; the declarative SwiftUI shell and scripts carry exact specs.

**Goal:** Ship `macos/` — DecantKit (tested client core) + DecantBar (MenuBarExtra shell) + `Decant.app` bundling + paths-gated macOS CI.

**Architecture:** Spec `docs/superpowers/specs/2026-06-10-macos-menubar-design.md` §2: pure config resolution → typed client → SSE AsyncStream → @Observable NowStore (debounce/backoff/fallback, injected clock+client) → thin MenuBarExtra.

**Tech Stack:** Swift 6 strict concurrency, swift-testing, URLSession only, SwiftPM executable + bundle.sh, GitHub Actions macos-15.

---

## File Structure

**Create:**
- `macos/Package.swift` — swift-tools 6.0; library `DecantKit`, executable `DecantBar`, test target.
- `macos/Sources/DecantKit/{DaemonConfig,Models,DaemonClient,SSE,NowStore}.swift`
- `macos/Sources/DecantBar/{DecantBarApp,PopoverView}.swift`
- `macos/Tests/DecantKitTests/{ConfigTests,ModelsTests,SSETests,NowStoreTests}.swift`
- `macos/bundle.sh`, `macos/Info.plist.template`, `macos/README.md`
- `.github/workflows/macos.yml`

**Modify:** `justfile` (`menubar` recipe group), root `README`/AGENTS pointers only if conventions require (skip otherwise).

## Task 1: package + DaemonConfig + Models (TDD)

- [ ] Package.swift (platforms: [.macOS(.v14)]); empty targets compile: `cd macos && swift build`.
- [ ] Failing tests: ConfigTests (env URL wins; port env honored; default 4577; token env over file; nil token when file missing), ModelsTests (decode `/now` envelope fixture with one active session + empty variant; snake_case fields → properties).
- [ ] Implement `DaemonConfig.resolve(env:readFile:)` and Codable models per spec §2. Run `swift test` → green. Commit `feat(macos): DecantKit config + API models`.

## Task 2: SSE parser + client (TDD)

- [ ] Failing SSETests: `event:`+`data:` frame emits ServerEvent; multiple `data:` lines join with \n; `:` keep-alive and unknown fields ignored; partial chunk buffering (feed "eve","nt: x\n\n"); blank-line dispatch only.
- [ ] Implement `SSELineParser` (stateful struct, `mutating func feed(_ line: String) -> ServerEvent?` + a byte-chunk wrapper) and `SSEClient` AsyncThrowingStream over `URLSession.AsyncBytes.lines` with bearer header. Parser tests green (client itself exercised via NowStore fakes). Commit `feat(macos): SSE stream parsing`.

## Task 3: NowStore (TDD)

- [ ] Failing NowStoreTests with injected `FakeClient` (scripted now()/failures) + `AsyncStream` of fake events + manual clock: initial load → .ready; client error → .down(hint); event burst → exactly one refetch after debounce; SSE failure → backoff schedule 1,2,4…≤60 and fallback refetch at 60 s; liveCount == active_sessions.count.
- [ ] Implement per spec §2/§3. Green. Commit `feat(macos): NowStore state machine`.

## Task 4: DecantBar shell + bundling

- [ ] `DecantBarApp`: `MenuBarExtra` (window style), icon per phase (`drop`/`drop.fill`/`drop.triangle`), store wired to real client+SSE from `DaemonConfig.resolve()`.
- [ ] `PopoverView` per spec §1.5 (fixed ~300 pt width; monospaced digits; Open Dashboard via NSWorkspace; Refresh; Quit `NSApplication.shared.terminate`).
- [ ] `bundle.sh`: release build → `build/Decant.app` (Contents/MacOS/DecantBar + generated Info.plist: LSUIElement true, CFBundleIdentifier dev.decant.menubar, LSMinimumSystemVersion 14.0) + `codesign --force -s -`. `swift build` clean of warnings.
- [ ] justfile: group "macos": `menubar-build`, `menubar` (bundle + `open build/Decant.app`), `menubar-test`.
- [ ] Commit `feat(macos): DecantBar menu bar shell + app bundling`.

## Task 5: CI + verification + PR

- [ ] `.github/workflows/macos.yml`: on push/PR with `paths: ["macos/**", ".github/workflows/macos.yml"]`; macos-15; `swift build -c release` + `swift test` in `macos/`.
- [ ] Local verification: `swift test` all green; `bash bundle.sh`; launch against the live daemon, observe icon + popover with real numbers (manual, not CI); screenshot if capturable.
- [ ] Push branch `menubar-app`, PR, address bot review, CI green → ready for merge call.

## Self-review

Spec §1.1→T1 layout, §1.2→T1 config, §1.3/§3→T3, §1.4-1.5→T4, §1.7→T5, §2 components→T1-4, §4 tests→T1-3 + T5 manual. Names consistent: DecantKit/DecantBar/NowStore/SSELineParser/DaemonConfig.
