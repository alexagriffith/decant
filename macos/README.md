# decant menu bar app

A glanceable macOS menu bar surface for the decant daemon: the drop icon fills
while an agent session is writing, and the popover shows today's totals and
live sessions — all from `GET /analytics/now` plus the `session_activity` SSE
stream.

```sh
just menubar          # build Decant.app and open it
just menubar-test     # swift test (DecantKit)
```

Or directly: `cd macos && bash bundle.sh && open build/Decant.app`.

- **Config:** daemon URL from `DECANT_DAEMON_URL` (default `http://127.0.0.1:4577`;
  `DECANT_DAEMON_PORT` also honored, which the web client doesn't read), token
  from `DECANT_DAEMON_TOKEN` else `daemon.token` under `DECANT_CONFIG_DIR`
  (default `~/.decant`) — matching where the daemon writes it. Overrides for
  tinkering: `defaults write dev.decant.menubar daemonURL http://127.0.0.1:4599`
  (and `dashboardURL`, default `http://localhost:4000`).
- **Layout:** `DecantKit` (config, client, SSE, store — tested with
  swift-testing) and `DecantBar` (thin SwiftUI `MenuBarExtra`).
- **Refresh model:** SSE events debounce into a `/now` refetch; a 60s poll runs
  only while the stream is down; popover-open refetches.
- Design + plan: `docs/superpowers/{specs,plans}/2026-06-10-macos-menubar*.md`.
