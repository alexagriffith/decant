import { describe, expect, test } from "bun:test";
import { chmodSync, mkdtempSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const launcher = require("../npm/decant/bin/decant.cjs") as {
  overrideEnvVar: string;
  platformKey(platform?: string, arch?: string): string;
  resolveBinary(options?: {
    env?: Record<string, string | undefined>;
    platform?: string;
    arch?: string;
  }): { binary: string; source: string; target: unknown };
  run(
    argv?: string[],
    options?: {
      env?: Record<string, string | undefined>;
      spawnSync?: (binary: string, argv: string[], options: { stdio: string }) => unknown;
      stdio?: string;
    },
  ): number;
  supportedTargetList(): string;
  targetForPlatform(platform?: string, arch?: string): { package: string } | null;
};

describe("npm launcher", () => {
  test("maps supported platforms to optional package names", () => {
    expect(launcher.platformKey("darwin", "arm64")).toBe("darwin-arm64");
    expect(launcher.targetForPlatform("darwin", "arm64")?.package).toBe(
      "@dosu/decant-darwin-arm64",
    );
    expect(launcher.targetForPlatform("darwin", "x64")?.package).toBe("@dosu/decant-darwin-x64");
    expect(launcher.targetForPlatform("linux", "arm64")?.package).toBe("@dosu/decant-linux-arm64");
    expect(launcher.targetForPlatform("linux", "x64")?.package).toBe("@dosu/decant-linux-x64");
    expect(launcher.targetForPlatform("win32", "x64")).toBeNull();
  });

  test("reports unsupported platforms with the supported matrix", () => {
    expect(() => launcher.resolveBinary({ platform: "win32", arch: "x64", env: {} })).toThrow(
      /Supported targets: darwin\/arm64, darwin\/x64, linux\/arm64, linux\/x64/,
    );
  });

  test("uses DECANT_BINARY_PATH for local package smoke tests", () => {
    const dir = mkdtempSync(join(tmpdir(), "decant-npm-launcher-"));
    const binary = join(dir, "decant");
    writeFileSync(binary, "#!/usr/bin/env sh\nexit 0\n");
    chmodSync(binary, 0o755);

    const resolved = launcher.resolveBinary({
      env: { [launcher.overrideEnvVar]: binary },
      platform: "win32",
      arch: "x64",
    });
    expect(resolved).toMatchObject({ binary, source: launcher.overrideEnvVar });

    const status = launcher.run(["--help"], {
      env: { [launcher.overrideEnvVar]: binary },
      stdio: "pipe",
      spawnSync: (cmd, argv, options) => {
        expect(cmd).toBe(binary);
        expect(argv).toEqual(["--help"]);
        expect(options.stdio).toBe("pipe");
        return { status: 0 };
      },
    });
    expect(status).toBe(0);
  });
});
