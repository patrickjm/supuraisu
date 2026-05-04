#!/usr/bin/env node
import { readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const version = process.argv[2];
if (!version || !/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
  console.error('Usage: bun run version:set <semver>');
  process.exit(1);
}

async function updateJson(file, update) {
  const full = path.join(root, file);
  const json = JSON.parse(await readFile(full, 'utf8'));
  update(json);
  await writeFile(full, `${JSON.stringify(json, null, 2)}\n`);
}

await updateJson('package.json', (json) => { json.version = version; });
await updateJson('src-tauri/tauri.conf.json', (json) => { json.version = version; });

const cargoPath = path.join(root, 'src-tauri/Cargo.toml');
let cargo = await readFile(cargoPath, 'utf8');
cargo = cargo.replace(/^(version\s*=\s*")[^"]+("\s*)$/m, `$1${version}$2`);
await writeFile(cargoPath, cargo);

console.log(`Supuraisu version set to ${version}`);
