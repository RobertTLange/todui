import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const sharedPath = fileURLToPath(import.meta.url);

export const PACKAGE_ROOT = resolve(dirname(sharedPath), "..", "..");

const TARGET_TRIPLES = {
  darwin: {
    arm64: "aarch64-apple-darwin",
    x64: "x86_64-apple-darwin",
  },
  linux: {
    arm64: "aarch64-unknown-linux-gnu",
    x64: "x86_64-unknown-linux-gnu",
  },
};

export function supportedTargetList() {
  return Object.entries(TARGET_TRIPLES).flatMap(([platform, archMap]) =>
    Object.keys(archMap).map((arch) => `${platform}/${arch}`),
  );
}

export function resolveTargetTriple(platform = process.platform, arch = process.arch) {
  const targetTriple = TARGET_TRIPLES[platform]?.[arch];
  if (targetTriple) {
    return targetTriple;
  }

  throw new Error(
    `Unsupported platform/arch: ${platform}/${arch}. Supported targets: ${supportedTargetList().join(", ")}`,
  );
}

export function releaseAssetName(version, targetTriple) {
  return `todui-v${version}-${targetTriple}`;
}

export function releaseChecksumName(version, targetTriple) {
  return `${releaseAssetName(version, targetTriple)}.sha256`;
}

export function installedBinaryPath(packageRoot = PACKAGE_ROOT, targetTriple = resolveTargetTriple()) {
  return join(packageRoot, "vendor", targetTriple, "todui");
}

export function githubReleaseBaseUrl(version, repo = "RobertTLange/todui") {
  return `https://github.com/${repo}/releases/download/v${version}`;
}

export function parseChecksumFile(contents) {
  const expectedHash = contents.trim().split(/\s+/u)[0];
  if (!/^[0-9a-fA-F]{64}$/u.test(expectedHash)) {
    throw new Error(`Invalid checksum contents: ${contents.trim()}`);
  }

  return expectedHash.toLowerCase();
}

export function readPackageVersion(packageRoot = PACKAGE_ROOT) {
  const packageJsonPath = join(packageRoot, "package.json");
  const packageJson = JSON.parse(readFileSync(packageJsonPath, "utf8"));
  return packageJson.version;
}
