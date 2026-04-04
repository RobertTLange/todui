import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { PACKAGE_ROOT, readPackageVersion } from "./shared.mjs";

export function readCargoPackageVersion(cargoTomlContents) {
  let inPackageSection = false;

  for (const rawLine of cargoTomlContents.split(/\r?\n/u)) {
    const line = rawLine.trim();
    if (line.startsWith("[") && line.endsWith("]")) {
      inPackageSection = line === "[package]";
      continue;
    }

    if (!inPackageSection || !line.startsWith("version")) {
      continue;
    }

    const match = line.match(/^version\s*=\s*"([^"]+)"$/u);
    if (match) {
      return match[1];
    }
  }

  throw new Error("Unable to find the [package] version in Cargo.toml.");
}

export function assertVersionSync(packageRoot = PACKAGE_ROOT) {
  const cargoTomlPath = join(packageRoot, "Cargo.toml");
  const cargoVersion = readCargoPackageVersion(readFileSync(cargoTomlPath, "utf8"));
  const npmVersion = readPackageVersion(packageRoot);
  if (cargoVersion !== npmVersion) {
    throw new Error(
      `Version mismatch: Cargo.toml=${cargoVersion}, package.json=${npmVersion}`,
    );
  }

  return { cargoVersion, npmVersion };
}

const scriptPath = fileURLToPath(import.meta.url);

if (process.argv[1] && resolve(process.argv[1]) === scriptPath) {
  const { cargoVersion } = assertVersionSync();
  console.log(`Version sync OK: ${cargoVersion}`);
}
