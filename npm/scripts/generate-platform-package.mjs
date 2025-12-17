#!/usr/bin/env node

/**
 * Generate package.json for a platform-specific npm package.
 *
 * Usage: node generate-platform-package.mjs <platform> <version>
 * Example: node generate-platform-package.mjs darwin-arm64 0.1.0
 */

import { writeFileSync, readFileSync } from "fs";
import { join, dirname } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const platforms = JSON.parse(readFileSync(join(__dirname, "platforms.json"), "utf8"));

const [, , platform, version] = process.argv;

if (!platform || !version) {
  console.error("Usage: generate-platform-package.mjs <platform> <version>");
  console.error("Example: generate-platform-package.mjs darwin-arm64 0.1.0");
  process.exit(1);
}

const config = platforms[platform];
if (!config) {
  console.error(`Unknown platform: ${platform}`);
  console.error(`Available: ${Object.keys(platforms).join(", ")}`);
  process.exit(1);
}

const packageJson = {
  name: `jj-ryu-${platform}`,
  version,
  description: `ryu binary for ${config.os} ${config.cpu}${config.libc ? ` (${config.libc})` : ""}`,
  repository: {
    type: "git",
    url: "git+https://github.com/dmmulroy/jj-ryu.git",
  },
  license: "MIT",
  os: [config.os],
  cpu: [config.cpu],
  preferUnplugged: true,
  publishConfig: {
    access: "public",
    provenance: true,
  },
};

// Add libc field for musl variants
if (config.libc) {
  packageJson.libc = [config.libc];
}

const outDir = join(__dirname, "..", platform);
const outPath = join(outDir, "package.json");

writeFileSync(outPath, JSON.stringify(packageJson, null, 2) + "\n");
console.log(`Generated ${outPath}`);
