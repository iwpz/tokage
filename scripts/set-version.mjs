#!/usr/bin/env node
/**
 * Sync the application version across package.json, package-lock.json,
 * src-tauri/tauri.conf.json, src-tauri/Cargo.toml, and workspace crates.
 *
 * Usage:
 *   node scripts/set-version.mjs 0.1.0-beta
 *   node scripts/set-version.mjs v0.1.0-beta
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const rootDir = path.resolve(__dirname, "..");

const rawInput = (process.argv[2] || "").trim();
if (!rawInput) {
  console.error("Usage: node scripts/set-version.mjs <version>");
  process.exit(1);
}

// Strip optional leading 'v' or 'V' so SemVer files receive e.g. "0.1.0-beta"
const version = rawInput.replace(/^[vV]/, "");
if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(version)) {
  console.error(`Invalid SemVer version: "${rawInput}" (normalized: "${version}")`);
  process.exit(1);
}

function updateJsonFile(relPath, updater) {
  const fullPath = path.join(rootDir, relPath);
  if (!fs.existsSync(fullPath)) return;
  const data = JSON.parse(fs.readFileSync(fullPath, "utf8"));
  updater(data);
  fs.writeFileSync(fullPath, JSON.stringify(data, null, 2) + "\n", "utf8");
  console.log(`[set-version] updated ${relPath} -> ${version}`);
}

function updateCargoToml(relPath) {
  const fullPath = path.join(rootDir, relPath);
  if (!fs.existsSync(fullPath)) return;
  const content = fs.readFileSync(fullPath, "utf8");
  const updated = content.replace(
    /^(\[package\][\s\S]*?^version\s*=\s*)"[^"]*"/m,
    `$1"${version}"`
  );
  fs.writeFileSync(fullPath, updated, "utf8");
  console.log(`[set-version] updated ${relPath} -> ${version}`);
}

updateJsonFile("package.json", (pkg) => {
  pkg.version = version;
});

updateJsonFile("package-lock.json", (lock) => {
  lock.version = version;
  if (lock.packages && lock.packages[""]) {
    lock.packages[""].version = version;
  }
});

updateJsonFile("src-tauri/tauri.conf.json", (conf) => {
  conf.version = version;
});

updateCargoToml("src-tauri/Cargo.toml");

const cratesDir = path.join(rootDir, "src-tauri", "crates");
if (fs.existsSync(cratesDir)) {
  for (const entry of fs.readdirSync(cratesDir, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      updateCargoToml(path.join("src-tauri", "crates", entry.name, "Cargo.toml"));
    }
  }
}

console.log(`[set-version] Done. Normalized version: ${version} (tag: v${version})`);
