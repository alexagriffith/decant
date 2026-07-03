import { spawnSync } from "node:child_process";
import { chmodSync, copyFileSync, existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

export interface DistributionTarget {
  key: string;
  platform: NodeJS.Platform;
  arch: NodeJS.Architecture;
  package: string;
  binary: string;
  bunTarget: string;
}

export interface BuildOptions {
  root?: string;
  outDir?: string;
}

export interface StageOptions {
  root?: string;
  outDir?: string;
  binaryDir?: string;
  targets?: DistributionTarget[];
  buildMissing?: boolean;
  clean?: boolean;
}

export const repoRoot = resolve(import.meta.dir, "..");

export function readTargets(root = repoRoot): DistributionTarget[] {
  const metadata = JSON.parse(
    readFileSync(join(root, "npm", "decant", "targets.json"), "utf8"),
  ) as {
    targets: DistributionTarget[];
  };
  return metadata.targets;
}

export function nativeTarget(targets = readTargets()): DistributionTarget | null {
  const key = `${process.platform}-${process.arch}`;
  return targets.find((target) => target.key === key) ?? null;
}

export function selectTargets(selector: string, targets = readTargets()): DistributionTarget[] {
  if (selector === "all") {
    return targets;
  }
  if (selector === "native") {
    const target = nativeTarget(targets);
    if (target == null) {
      throw new Error(`unsupported native target ${process.platform}/${process.arch}`);
    }
    return [target];
  }
  const target = targets.find((candidate) => candidate.key === selector);
  if (target == null) {
    throw new Error(
      `unknown target ${JSON.stringify(selector)} (expected: all | native | ${targetKeys(targets)})`,
    );
  }
  return [target];
}

export function targetKeys(targets = readTargets()): string {
  return targets.map((target) => target.key).join(" | ");
}

export function packageDirName(target: DistributionTarget): string {
  return target.package.replace("@dosu/", "");
}

export function binaryOutputPath(target: DistributionTarget, options: BuildOptions = {}): string {
  return join(
    resolve(options.root ?? repoRoot, options.outDir ?? "dist/bin"),
    target.key,
    "decant",
  );
}

export function buildTarget(target: DistributionTarget, options: BuildOptions = {}): string {
  const root = options.root ?? repoRoot;
  const outPath = binaryOutputPath(target, { root, outDir: options.outDir });
  mkdirSync(dirname(outPath), { recursive: true });
  const result = spawnSync(
    "bun",
    ["build", "--compile", "--target", target.bunTarget, "src/cli.ts", "--outfile", outPath],
    { cwd: root, stdio: "inherit" },
  );
  if (result.error != null) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(`bun build failed for ${target.key} with status ${result.status ?? 1}`);
  }
  chmodSync(outPath, 0o755);
  return outPath;
}

export function stageNpmPackages(options: StageOptions = {}): string {
  const root = options.root ?? repoRoot;
  const outDir = resolve(root, options.outDir ?? "dist/npm");
  const binaryDir = options.binaryDir ?? "dist/bin";
  const targets = options.targets ?? readTargets(root);
  if (options.clean === true) {
    rmSync(outDir, { recursive: true, force: true });
  }
  stageLauncherPackage(root, outDir);
  for (const target of targets) {
    stagePlatformPackage(target, { root, outDir, binaryDir, buildMissing: options.buildMissing });
  }
  return outDir;
}

function stageLauncherPackage(root: string, outDir: string): void {
  const source = join(root, "npm", "decant");
  const dest = join(outDir, "decant");
  mkdirSync(join(dest, "bin"), { recursive: true });
  for (const file of ["package.json", "README.md", "targets.json"]) {
    copyFileSync(join(source, file), join(dest, file));
  }
  copyFileSync(join(source, "bin", "decant.cjs"), join(dest, "bin", "decant.cjs"));
  copyFileSync(join(root, "LICENSE"), join(dest, "LICENSE"));
}

function stagePlatformPackage(
  target: DistributionTarget,
  options: {
    root: string;
    outDir: string;
    binaryDir: string;
    buildMissing?: boolean;
  },
): void {
  const source = join(options.root, "npm", packageDirName(target));
  const dest = join(options.outDir, packageDirName(target));
  const builtBinary = binaryOutputPath(target, { root: options.root, outDir: options.binaryDir });
  if (!existsSync(builtBinary)) {
    if (options.buildMissing === false) {
      throw new Error(`missing compiled binary for ${target.key}: ${builtBinary}`);
    }
    buildTarget(target, { root: options.root, outDir: options.binaryDir });
  }

  mkdirSync(join(dest, dirname(target.binary)), { recursive: true });
  copyFileSync(join(source, "package.json"), join(dest, "package.json"));
  copyFileSync(builtBinary, join(dest, target.binary));
  chmodSync(join(dest, target.binary), 0o755);
}

export function parseDistributionArgs(
  args: string[],
  defaults: { target?: string; outDir?: string; binaryDir?: string; buildMissing?: boolean } = {},
): { target: string; outDir: string; binaryDir: string; buildMissing: boolean; clean: boolean } {
  let target = defaults.target ?? "all";
  let outDir = defaults.outDir ?? "dist/npm";
  let binaryDir = defaults.binaryDir ?? "dist/bin";
  let buildMissing = defaults.buildMissing ?? true;
  let clean = false;

  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    switch (arg) {
      case "--target":
        index += 1;
        target = requiredValue(args, index, arg);
        break;
      case "--out-dir":
        index += 1;
        outDir = requiredValue(args, index, arg);
        break;
      case "--binary-dir":
        index += 1;
        binaryDir = requiredValue(args, index, arg);
        break;
      case "--build-missing":
        buildMissing = true;
        break;
      case "--no-build":
        buildMissing = false;
        break;
      case "--clean":
        clean = true;
        break;
      default:
        throw new Error(`unknown option ${arg}`);
    }
  }

  return { target, outDir, binaryDir, buildMissing, clean };
}

function requiredValue(args: string[], index: number, flag: string): string {
  const value = args[index];
  if (value == null || value.startsWith("--")) {
    throw new Error(`${flag} requires a value`);
  }
  return value;
}
