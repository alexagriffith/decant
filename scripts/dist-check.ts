#!/usr/bin/env bun
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { buildTarget, nativeTarget, readTargets, stageNpmPackages } from "./distribution.ts";

const version = process.env.DECANT_DISTCHECK_VERSION ?? "0.0.0-check";
const outRoot = process.env.DECANT_DISTCHECK_DIR ?? "/tmp/decant-check";
const binaryDir = join(outRoot, "bin");
const npmDir = join(outRoot, "npm");
const target = nativeTarget(readTargets());

if (target == null) {
  throw new Error(`unsupported native target ${process.platform}/${process.arch}`);
}

const binary = buildTarget(target, { outDir: binaryDir, version });
assertVersion(binary, ["--version"], {}, version);

stageNpmPackages({
  outDir: npmDir,
  binaryDir,
  targets: [target],
  buildMissing: false,
  clean: true,
  version,
});

assertVersion(
  "node",
  [join(npmDir, "decant", "bin", "decant.cjs"), "--version"],
  { DECANT_BINARY_PATH: binary },
  version,
);

process.stdout.write(`dist check ok for ${target.key} (${version})\n`);

function assertVersion(
  command: string,
  args: string[],
  env: Record<string, string>,
  expected: string,
): void {
  const result = spawnSync(command, args, {
    encoding: "utf8",
    env: { ...process.env, ...env },
    stdio: "pipe",
  });
  if (result.error != null) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(
      `${command} ${args.join(" ")} failed with status ${result.status ?? 1}: ${result.stderr}`,
    );
  }
  if (!result.stdout.includes(expected)) {
    throw new Error(`expected ${expected} in version output, got: ${result.stdout.trim()}`);
  }
}
