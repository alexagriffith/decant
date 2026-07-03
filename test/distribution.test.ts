import { describe, expect, test } from "bun:test";
import {
  packageDirName,
  parseDistributionArgs,
  readTargets,
  selectTargets,
  targetKeys,
} from "../scripts/distribution.ts";

describe("distribution helpers", () => {
  test("loads the npm binary target matrix", () => {
    const targets = readTargets();
    expect(targets.map((target) => target.key)).toEqual([
      "darwin-arm64",
      "darwin-x64",
      "linux-arm64",
      "linux-x64",
    ]);
    expect(targets.map(packageDirName)).toEqual([
      "decant-darwin-arm64",
      "decant-darwin-x64",
      "decant-linux-arm64",
      "decant-linux-x64",
    ]);
    expect(targetKeys(targets)).toBe("darwin-arm64 | darwin-x64 | linux-arm64 | linux-x64");
  });

  test("selects all targets or one named target", () => {
    const targets = readTargets();
    expect(selectTargets("all", targets)).toHaveLength(4);
    expect(selectTargets("linux-x64", targets).map((target) => target.package)).toEqual([
      "@dosu/decant-linux-x64",
    ]);
    expect(() => selectTargets("windows-x64", targets)).toThrow(/unknown target/);
  });

  test("parses build arguments", () => {
    expect(
      parseDistributionArgs([
        "--target",
        "native",
        "--out-dir",
        "/tmp/npm",
        "--binary-dir",
        "/tmp/bin",
        "--no-build",
        "--clean",
      ]),
    ).toEqual({
      target: "native",
      outDir: "/tmp/npm",
      binaryDir: "/tmp/bin",
      buildMissing: false,
      clean: true,
    });
  });
});
