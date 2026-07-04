# Local Serve Routes

`decant serve` runs the CLI, watcher, JSON routes, SSE stream, and React UI in
one Bun process. These routes are internal app routes, not a versioned public
contract.

## UI

- `GET /`
- `GET /sessions/:id`
- `GET /search`
- `GET /analytics`
- `GET /insights`
- `GET /tools`
- `GET /files`
- `GET /settings`

## JSON

- `GET /api/health`
- `GET /api/config`
- `GET /api/settings`
- `POST /api/settings`
- `GET /api/sync-status`
- `GET /api/metadata/sync-status`
- `POST /api/sync`
- `GET /api/sessions`
- `GET /api/sessions/:id`
- `POST /api/search`
- `GET /api/stats/summary`
- `GET /api/stats/by-dimension?dim=tool|model|project|day`
- `GET /api/analytics/activity`
- `GET /api/analytics/model-sparklines`
- `GET /api/analytics/now`
- `GET /api/date-bounds`
- `GET /api/metadata/date-bounds`
- `GET /api/files?group=path|ext&op=read|edit|write|delete`
- `GET /api/tools/usage`
- `GET /api/tools/mcp-usage`
- `GET /api/recommendations?status=open|implemented|all`
- `POST /api/recommendations/mark`
- `POST /api/launch/agent`
- `POST /api/launch/ide`

## Events

- `GET /api/events` returns an SSE stream.

Current event names:

- `hello`
- `ready`
- `sync`
- `archive_updated`
- `error`
- `stopped`
