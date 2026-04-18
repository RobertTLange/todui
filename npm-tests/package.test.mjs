import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

import { localBuildBinaryPaths, resolveBinaryPath } from "../bin/todui.js";
import { readCargoPackageVersion, assertVersionSync } from "../scripts/npm/check-version-sync.mjs";
import { isSourceCheckout } from "../scripts/npm/install.mjs";
import { extractReleaseNotes } from "../scripts/release-notes.mjs";
import {
  parseChecksumFile,
  releaseAssetName,
  releaseChecksumName,
  resolveTargetTriple,
  supportedTargetList,
} from "../scripts/npm/shared.mjs";

test("resolveTargetTriple maps supported platforms", () => {
  assert.equal(resolveTargetTriple("darwin", "arm64"), "aarch64-apple-darwin");
  assert.equal(resolveTargetTriple("darwin", "x64"), "x86_64-apple-darwin");
  assert.equal(resolveTargetTriple("linux", "arm64"), "aarch64-unknown-linux-gnu");
  assert.equal(resolveTargetTriple("linux", "x64"), "x86_64-unknown-linux-gnu");
});

test("resolveTargetTriple rejects unsupported platforms with the full matrix", () => {
  assert.throws(
    () => resolveTargetTriple("win32", "x64"),
    new RegExp(supportedTargetList().join(", ").replaceAll("/", "\\/")),
  );
});

test("asset helpers use deterministic release names", () => {
  const version = "0.1.0";
  const targetTriple = "aarch64-apple-darwin";
  assert.equal(releaseAssetName(version, targetTriple), "todui-v0.1.0-aarch64-apple-darwin");
  assert.equal(
    releaseChecksumName(version, targetTriple),
    "todui-v0.1.0-aarch64-apple-darwin.sha256",
  );
});

test("parseChecksumFile accepts standard sha256sum output", () => {
  const hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
  assert.equal(parseChecksumFile(`${hash}  todui-v0.1.0-aarch64-apple-darwin`), hash);
});

test("readCargoPackageVersion finds the top-level package version", () => {
  const cargoToml = `
[workspace]
members = ["crates/a"]

[package]
name = "todui"
version = "0.1.0"
edition = "2024"
`;
  assert.equal(readCargoPackageVersion(cargoToml), "0.1.0");
});

test("assertVersionSync matches Cargo.toml and package.json", () => {
  assert.deepEqual(assertVersionSync(), {
    cargoVersion: "0.1.1",
    npmVersion: "0.1.1",
  });
});

test("extractReleaseNotes returns the requested changelog section", () => {
  const changelog = `
# Changelog

## [Unreleased]

## [0.1.0] - 2026-04-04

### Added

- Initial release

## [0.0.1] - 2026-03-01

- Earlier release
`;
  assert.equal(extractReleaseNotes(changelog, "0.1.0"), "### Added\n\n- Initial release");
});

test("isSourceCheckout detects repo installs from a Cargo checkout", () => {
  assert.equal(isSourceCheckout(), true);
  assert.equal(isSourceCheckout("/definitely/not/a/todui/repo"), false);
});

test("package files include assets referenced by the published README", () => {
  const packageJson = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8"));
  assert.ok(packageJson.files.includes("config.example.toml"));
  assert.ok(packageJson.files.includes("docs/logo.png"));
});

test("release workflow creates the GitHub release before publishing to npm", () => {
  const workflow = readFileSync(new URL("../.github/workflows/release.yml", import.meta.url), "utf8");
  const createReleaseIndex = workflow.indexOf("- name: Create GitHub release");
  const publishNpmIndex = workflow.indexOf("- name: Publish to npm");

  assert.notEqual(createReleaseIndex, -1);
  assert.notEqual(publishNpmIndex, -1);
  assert.ok(createReleaseIndex < publishNpmIndex);
});

test("release workflow pins the aarch64 linux linker for cross-compiles", () => {
  const workflow = readFileSync(new URL("../.github/workflows/release.yml", import.meta.url), "utf8");

  assert.match(
    workflow,
    /CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER:\s*aarch64-linux-gnu-gcc/,
  );
});

test("launcher prefers a local cargo build in a source checkout", () => {
  const packageRoot = mkdtempSync(join(tmpdir(), "todui-launcher-"));
  writeFileSync(join(packageRoot, "Cargo.toml"), "[package]\nname = \"todui\"\nversion = \"0.1.0\"\n");

  const binaryPath = join(packageRoot, "target", "debug", "todui");
  mkdirSync(join(packageRoot, "target", "debug"), { recursive: true });
  writeFileSync(binaryPath, "");

  assert.equal(
    resolveBinaryPath({}, packageRoot, "darwin", "x64"),
    binaryPath,
  );
});

test("launcher checks target-specific local release builds first", () => {
  const packageRoot = mkdtempSync(join(tmpdir(), "todui-launcher-"));
  writeFileSync(join(packageRoot, "Cargo.toml"), "[package]\nname = \"todui\"\nversion = \"0.1.0\"\n");

  const [targetReleasePath] = localBuildBinaryPaths(packageRoot, "x86_64-apple-darwin");
  mkdirSync(join(packageRoot, "target", "x86_64-apple-darwin", "release"), { recursive: true });
  writeFileSync(targetReleasePath, "");

  assert.equal(
    resolveBinaryPath({}, packageRoot, "darwin", "x64"),
    targetReleasePath,
  );
});
