#!/usr/bin/env node
import { existsSync } from 'node:fs';

const forbidden = [
  'static/splice-audio.wasm',
  'build/splice-audio.wasm',
  '.svelte-kit/output/client/splice-audio.wasm',
];

const found = forbidden.filter((path) => existsSync(path));
if (found.length) {
  console.error('\nRefusing to build with proprietary/local-only Splice WASM present:');
  for (const path of found) console.error(`  - ${path}`);
  console.error('\nRemove these files before building public packages.');
  process.exit(1);
}
