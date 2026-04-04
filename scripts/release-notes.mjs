import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const headingPattern = /^##\s+\[?([^\]\n]+)\]?(?:\s+-\s+.+)?$/gmu;

export function extractReleaseNotes(changelogContents, version) {
  const matches = [...changelogContents.matchAll(headingPattern)];
  const section = matches.find((match) => match[1].trim() === version);
  if (!section) {
    throw new Error(`Unable to find CHANGELOG section for version ${version}.`);
  }

  const startIndex = section.index + section[0].length;
  const currentIndex = matches.findIndex((match) => match.index === section.index);
  const endIndex =
    currentIndex + 1 < matches.length ? matches[currentIndex + 1].index : changelogContents.length;
  const notes = changelogContents.slice(startIndex, endIndex).trim();
  if (!notes) {
    throw new Error(`CHANGELOG section for version ${version} is empty.`);
  }

  return notes;
}

const scriptPath = fileURLToPath(import.meta.url);

if (process.argv[1] && resolve(process.argv[1]) === scriptPath) {
  const version = process.argv[2];
  if (!version) {
    console.error("Usage: node scripts/release-notes.mjs <version>");
    process.exit(1);
  }

  const changelogPath = join(process.cwd(), "CHANGELOG.md");
  const changelogContents = readFileSync(changelogPath, "utf8");
  process.stdout.write(`${extractReleaseNotes(changelogContents, version)}\n`);
}
