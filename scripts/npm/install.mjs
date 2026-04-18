import { createHash } from "node:crypto";
import { chmodSync, copyFileSync, existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  PACKAGE_ROOT,
  githubReleaseBaseUrl,
  installedBinaryPath,
  parseChecksumFile,
  readPackageVersion,
  releaseAssetName,
  releaseChecksumName,
  resolveTargetTriple,
} from "./shared.mjs";

function log(message) {
  console.log(`[todui] ${message}`);
}

async function download(url) {
  const response = await fetch(url, {
    headers: {
      "user-agent": "todui-npm-installer",
    },
  });

  if (!response.ok) {
    throw new Error(`download failed (${response.status} ${response.statusText}) for ${url}`);
  }

  return Buffer.from(await response.arrayBuffer());
}

function sha256(buffer) {
  return createHash("sha256").update(buffer).digest("hex");
}

function installBinary(destinationPath, contents) {
  mkdirSync(dirname(destinationPath), { recursive: true });
  rmSync(destinationPath, { force: true });
  writeFileSync(destinationPath, contents);
  chmodSync(destinationPath, 0o755);
}

export function isSourceCheckout(packageRoot = PACKAGE_ROOT) {
  return existsSync(join(packageRoot, "Cargo.toml"));
}

export async function main() {
  if (process.env.TODOUI_SKIP_DOWNLOAD === "1") {
    log("Skipping binary download because TODOUI_SKIP_DOWNLOAD=1.");
    return;
  }

  const targetTriple = resolveTargetTriple();
  const destinationPath = installedBinaryPath(PACKAGE_ROOT, targetTriple);
  const localBinaryPath = process.env.TODOUI_BINARY_PATH;

  if (localBinaryPath) {
    const sourcePath = resolve(localBinaryPath);
    mkdirSync(dirname(destinationPath), { recursive: true });
    rmSync(destinationPath, { force: true });
    copyFileSync(sourcePath, destinationPath);
    chmodSync(destinationPath, 0o755);
    log(`Installed local binary from ${sourcePath}.`);
    return;
  }

  if (isSourceCheckout()) {
    log("Skipping binary download in a source checkout; build with Cargo or set TODOUI_BINARY_PATH.");
    return;
  }

  const version = readPackageVersion();
  const baseUrl = process.env.TODOUI_RELEASE_BASE_URL ?? githubReleaseBaseUrl(version);
  const assetName = releaseAssetName(version, targetTriple);
  const checksumName = releaseChecksumName(version, targetTriple);
  const binaryUrl = `${baseUrl}/${assetName}`;
  const checksumUrl = `${baseUrl}/${checksumName}`;

  log(`Downloading ${assetName}.`);
  const [binaryContents, checksumContents] = await Promise.all([
    download(binaryUrl),
    download(checksumUrl),
  ]);

  const expectedHash = parseChecksumFile(checksumContents.toString("utf8"));
  const actualHash = sha256(binaryContents);
  if (actualHash !== expectedHash) {
    throw new Error(
      `checksum mismatch for ${assetName}: expected ${expectedHash}, got ${actualHash}`,
    );
  }

  installBinary(destinationPath, binaryContents);
  log(`Installed ${assetName} to ${destinationPath}.`);
}

const scriptPath = fileURLToPath(import.meta.url);

if (process.argv[1] && resolve(process.argv[1]) === scriptPath) {
  main().catch((error) => {
    console.error(`[todui] Failed to install binary: ${error.message}`);
    process.exit(1);
  });
}
