# Distribution

Decant's TypeScript migration ships as one Bun-authored app through three
paths: npm, Docker, and source.

## npm

The npm package is a Node-compatible launcher. `npx` starts under Node, but
Decant uses `bun:sqlite`, so the launcher selects a platform package containing
a Bun-compiled standalone binary.

```sh
npx @dosu/decant --help
npx @dosu/decant sync
npx @dosu/decant serve
```

Package layout:

- `@dosu/decant`: thin CommonJS launcher at `npm/decant/bin/decant.cjs`.
- `@dosu/decant-darwin-arm64`
- `@dosu/decant-darwin-x64`
- `@dosu/decant-linux-arm64`
- `@dosu/decant-linux-x64`

Build native artifacts for a local smoke test:

```sh
bun run scripts/build-binaries.ts --target native --out-dir /tmp/decant-bin
TARGET=darwin-arm64
DECANT_BINARY_PATH="/tmp/decant-bin/$TARGET/decant" node npm/decant/bin/decant.cjs --help
bun run scripts/build-npm.ts --target native --binary-dir /tmp/decant-bin --out-dir /tmp/decant-npm --no-build --clean
```

Set `TARGET` to the emitted target key for your platform; `darwin-arm64` is the
native target on Apple Silicon Macs.

Build all release artifacts:

```sh
bun run scripts/build-npm.ts --target all --clean
```

The launcher prints a clear reinstall message if optional dependencies were
disabled and the matching platform package is missing. Windows packages are
deferred.

## Docker

The image compiles Decant in an `oven/bun` builder and runs the standalone
binary in a non-root Debian runtime. Docker Desktop file watching across bind
mounts is unreliable, so the default command disables native filesystem watches
and relies on the periodic sweep.

```sh
docker buildx build --platform linux/amd64,linux/arm64 -t ghcr.io/dosu-ai/decant:local .
```

Local run:

```sh
docker run --rm \
  -p 127.0.0.1:4577:4577 \
  -v decant-data:/var/lib/decant \
  -v "$HOME/.claude/projects:/sources/claude:ro" \
  -v "$HOME/.codex:/sources/codex:ro" \
  ghcr.io/dosu-ai/decant:local
```

The container binds `0.0.0.0` inside the network namespace so Docker port
publishing can reach it. Publish to `127.0.0.1` on the host, as shown above, to
keep the UI local-only.

## Source

Source remains the contributor path and the fastest dev loop:

```sh
bun install
bun run src/cli.ts sync
bun run src/cli.ts serve
```

Source installs require Bun. npm and Docker installs do not.
