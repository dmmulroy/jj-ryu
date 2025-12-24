#!/usr/bin/env node

/**
 * Update version in main package.json and regenerate all platform package.json files.
 *
 * Usage: node update-versions.mjs <version>
 * Example: node update-versions.mjs 0.2.0
 */

import { writeFileSync, readFileSync, mkdirSync, existsSync } from "fs";
import { join, dirname } from "path";
import { fileURLToPath } from "url";
import { execSync } from "child_process";

const __dirname = dirname(fileURLToPath(import.meta.url));
const npmDir = join(__dirname, "..");
const platforms = JSON.parse(readFileSync(join(__dirname, "platforms.json"), "utf8"));

const [, , version] = process.argv;

if (!version) {
  console.error("Usage: update-versions.mjs <version>");
  console.error("Example: update-versions.mjs 0.2.0");
  process.exit(1);
}

// Validate semver using semver package
try {
  execSync(`npx --yes semver "${version}"`, { stdio: "pipe" });
} catch {
  console.error(`Invalid semver: ${version}`);
  process.exit(1);
}

// Update main package.json
const mainPkgPath = join(npmDir, "jj-ryu", "package.json");
const mainPkg = JSON.parse(readFileSync(mainPkgPath, "utf8"));

mainPkg.version = version;

// Update optionalDependencies versions
for (const dep of Object.keys(mainPkg.optionalDependencies)) {
  mainPkg.optionalDependencies[dep] = version;
}

writeFileSync(mainPkgPath, JSON.stringify(mainPkg, null, 2) + "\n");
console.log(`Updated ${mainPkgPath}`);

// Generate platform packages
for (const platform of Object.keys(platforms)) {
  const platformDir = join(npmDir, platform);
  if (!existsSync(platformDir)) {
    mkdirSync(platformDir, { recursive: true });
  }

  execSync(`node ${join(__dirname, "generate-platform-package.mjs")} ${platform} ${version}`, {
    stdio: "inherit",
  });
}

console.log(`\nAll packages updated to version ${version}`);
