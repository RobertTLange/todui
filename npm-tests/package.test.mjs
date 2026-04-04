import test from "node:test";
import assert from "node:assert/strict";

import { readCargoPackageVersion, assertVersionSync } from "../scripts/npm/check-version-sync.mjs";
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
    cargoVersion: "0.1.0",
    npmVersion: "0.1.0",
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
