#!/usr/bin/env node
// Assemble the Tauri updater manifest (latest.json) from the release's updater
// artifacts. For each signed updater artifact found under --assets-dir, pair it
// with its sibling `.sig`, map it to a platform key, and emit the manifest shape
// tauri-plugin-updater expects (see app/AUTO_UPDATE.md).
//
//   node app/scripts/build-latest-json.mjs \
//     --version 0.2.0 --notes "..." \
//     --base-url https://github.com/<owner>/<repo>/releases/download/v0.2.0 \
//     --assets-dir staged --out staged/latest.json
import { readFileSync, readdirSync, writeFileSync, statSync } from "node:fs";
import { join } from "node:path";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, token, i, all) => {
    if (token.startsWith("--")) pairs.push([token.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
for (const required of ["version", "base-url", "assets-dir", "out"]) {
  if (!args[required]) { console.error(`missing --${required}`); process.exit(1); }
}

// Each updater artifact's filename -> the manifest platform key it belongs to.
// (Tauri writes <name>.sig next to each updater artifact; the artifact that has a
// .sig is the one the updater downloads.)
function platformOf(name) {
  if (/_aarch64\.app\.tar\.gz$/.test(name)) return "darwin-aarch64";
  if (/_(x64|x86_64)\.app\.tar\.gz$/.test(name)) return "darwin-x86_64";
  if (/-setup\.exe$/.test(name) || /-setup\.nsis\.zip$/.test(name)) return "windows-x86_64";
  if (/_(x64|x86_64)[-_].*\.msi$/.test(name) || /\.msi$/.test(name)) return "windows-x86_64";
  if (/_amd64\.AppImage$/.test(name) || /_(x86_64|amd64)\.AppImage$/.test(name)) return "linux-x86_64";
  return null;
}

// Walk the assets dir for every *.sig; the artifact is the file it signs.
function sigFiles(dir) {
  const found = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) found.push(...sigFiles(full));
    else if (entry.endsWith(".sig")) found.push(full);
  }
  return found;
}

const platforms = {};
for (const sig of sigFiles(args["assets-dir"])) {
  const artifact = sig.replace(/\.sig$/, "");
  const name = artifact.split("/").pop();
  const key = platformOf(name);
  if (!key) { console.warn(`skipping unmapped updater artifact: ${name}`); continue; }
  if (platforms[key]) { console.warn(`duplicate artifact for ${key}: keeping ${platforms[key].url.split("/").pop()}, ignoring ${name}`); continue; }
  platforms[key] = {
    signature: readFileSync(sig, "utf8").trim(),
    url: `${args["base-url"].replace(/\/$/, "")}/${name}`,
  };
}

if (Object.keys(platforms).length === 0) {
  console.error("no updater artifacts (*.sig) found — refusing to write an empty manifest");
  process.exit(1);
}

const manifest = {
  version: args.version,
  notes: args.notes ?? `Copilot ${args.version}`,
  pub_date: new Date().toISOString().replace(/\.\d+Z$/, "Z"),
  platforms,
};
writeFileSync(args.out, JSON.stringify(manifest, null, 2) + "\n");
console.error(`wrote ${args.out} with platforms: ${Object.keys(platforms).join(", ")}`);
