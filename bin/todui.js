#!/usr/bin/env node

import { existsSync } from "node:fs";
import { spawn } from "node:child_process";
import { resolve } from "node:path";

import {
  installedBinaryPath,
  resolveTargetTriple,
  supportedTargetList,
} from "../scripts/npm/shared.mjs";

function fail(message) {
  console.error(`[todui] ${message}`);
  process.exit(1);
}

function resolveBinaryPath() {
  if (process.env.TODOUI_BINARY_PATH) {
    return resolve(process.env.TODOUI_BINARY_PATH);
  }

  try {
    return installedBinaryPath(undefined, resolveTargetTriple());
  } catch (error) {
    return fail(error.message);
  }
}

const binaryPath = resolveBinaryPath();
if (!existsSync(binaryPath)) {
  fail(
    [
      `Missing installed todui binary at ${binaryPath}.`,
      "Reinstall the package or set TODOUI_BINARY_PATH for local development.",
      `Supported targets: ${supportedTargetList().join(", ")}`,
    ].join(" "),
  );
}

const child = spawn(binaryPath, process.argv.slice(2), {
  stdio: "inherit",
});

child.on("error", (error) => {
  fail(`Unable to launch ${binaryPath}: ${error.message}`);
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }

  process.exit(code ?? 1);
});
