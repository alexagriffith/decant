const args = Bun.argv.slice(2);
if (args[0] === "--") {
  args.shift();
}

async function run(command: string[]): Promise<void> {
  const proc = Bun.spawn(command, {
    stdin: "inherit",
    stdout: "inherit",
    stderr: "inherit",
  });
  const exitCode = await proc.exited;
  if (exitCode !== 0) {
    process.exit(exitCode);
  }
}

await run(["bun", "install", "--frozen-lockfile"]);

const serve = Bun.spawn(["bun", "run", "src/cli.ts", "serve", ...args], {
  stdin: "inherit",
  stdout: "inherit",
  stderr: "inherit",
});

const forwardSignal = (signal: "SIGINT" | "SIGTERM"): void => {
  serve.kill(signal);
};

process.once("SIGINT", () => forwardSignal("SIGINT"));
process.once("SIGTERM", () => forwardSignal("SIGTERM"));

process.exit(await serve.exited);
