#!/usr/bin/env bun
import {
  buildTarget,
  parseDistributionArgs,
  readTargets,
  selectTargets,
  targetKeys,
} from "./distribution.ts";

try {
  const args = parseDistributionArgs(process.argv.slice(2), {
    target: "all",
    outDir: "dist/bin",
    binaryDir: "dist/bin",
  });
  const allTargets = readTargets();
  const targets = selectTargets(args.target, allTargets);
  for (const target of targets) {
    const outPath = buildTarget(target, { outDir: args.outDir, version: args.version });
    process.stdout.write(`built ${target.key}: ${outPath}\n`);
  }
} catch (error) {
  process.stderr.write(
    `${error instanceof Error ? error.message : String(error)}\nSupported targets: all | native | ${targetKeys()}\n`,
  );
  process.exitCode = 1;
}
