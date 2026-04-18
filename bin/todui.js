#!/usr/bin/env node

import { existsSync } from "node:fs";
import { spawn } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  installedBinaryPath,
  resolveTargetTriple,
  supportedTargetList,
} from "../scripts/npm/shared.mjs";

const scriptPath = fileURLToPath(import.meta.url);
const PACKAGE_ROOT = resolve(dirname(scriptPath), "..");

function fail(message) {
  console.error(`[todui] ${message}`);
  process.exit(1);
}

export function isSourceCheckout(packageRoot = PACKAGE_ROOT) {
  return existsSync(join(packageRoot, "Cargo.toml"));
}

export function localBuildBinaryPaths(
  packageRoot = PACKAGE_ROOT,
  targetTriple = resolveTargetTriple(),
) {
  return [
    join(packageRoot, "target", targetTriple, "release", "todui"),
    join(packageRoot, "target", "release", "todui"),
    join(packageRoot, "target", "debug", "todui"),
  ];
}

export function resolveBinaryPath(
  env = process.env,
  packageRoot = PACKAGE_ROOT,
  platform = process.platform,
  arch = process.arch,
) {
  if (env.TODOUI_BINARY_PATH) {
    return resolve(env.TODOUI_BINARY_PATH);
  }

  let targetTriple;
  try {
    targetTriple = resolveTargetTriple(platform, arch);
  } catch (error) {
    throw error;
  }

  const installedPath = installedBinaryPath(packageRoot, targetTriple);
  if (existsSync(installedPath)) {
    return installedPath;
  }

  if (!isSourceCheckout(packageRoot)) {
    return installedPath;
  }

  return localBuildBinaryPaths(packageRoot, targetTriple).find((path) => existsSync(path)) ?? installedPath;
}

export function main(argv = process.argv.slice(2), env = process.env) {
  let binaryPath;
  try {
    binaryPath = resolveBinaryPath(env);
  } catch (error) {
    fail(error.message);
  }

  if (!existsSync(binaryPath)) {
    fail(
      [
        `Missing installed todui binary at ${binaryPath}.`,
        isSourceCheckout()
          ? "Build with Cargo, reinstall the package, or set TODOUI_BINARY_PATH for local development."
          : "Reinstall the package or set TODOUI_BINARY_PATH for local development.",
        `Supported targets: ${supportedTargetList().join(", ")}`,
      ].join(" "),
    );
  }

  const child = spawn(binaryPath, argv, {
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
}

if (process.argv[1] && resolve(process.argv[1]) === scriptPath) {
  main();
}
