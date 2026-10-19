#!/usr/bin/env node
// Generates src/metadata_generated.rs from the upstream Tanzil metadata file, and
// checks every byte-level pin this project depends on.
//
// The upstream metadata is JavaScript, not JSON: it assigns QuranData.Sura and
// friends, with comments, single quoted strings and a sparse placeholder at index
// 0 of every array. It is therefore evaluated in a sandbox rather than parsed.
// Only Node built-in modules are used, because Node is a build-time tool for this
// project and never a runtime dependency.
//
// Usage:
//   node scripts/convert-quran-data.mjs [--data-dir <path>] [--check]
//
//   (default)  verify the pins, then write src/metadata_generated.rs
//   --check    verify the pins, then fail if the generated file is not current
//
// The upstream data directory is resolved in this order: --data-dir, then
// WIN_QURAN_DATA_DIR, then the locations the data is commonly installed in. A copy
// only counts once its SHA-256 matches the pin, so any location is equally
// trustworthy.

import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..');

// Pinned SHA-256 values. The text, the metadata and the Amiri Quran font are
// pinned by upstream Tanzil (the same values the quran-turn plugin ships in its
// data/SHA256SUMS). The Manrope and DM Mono pins are this project's own, recorded
// when those files were first copied, so any later drift is caught.
const ASSET_PINS = {
  'assets/quran-uthmani.txt': '6933e133dd56db778c801bf738848454e43648105a151e8d84d86a7cae39ec5f',
  'assets/fonts/AmiriQuran.woff2': '2c01d1ccdc75bf003a6fd1cae6ea8603ee5e49dea1e4fb6780ab75b30af27128',
  'assets/fonts/Manrope.woff2': '06ad9fd931db2eb624081c2b41265d84f232ccbd5a019dd086c6c8bad0ea687b',
  'assets/fonts/DMMono-Regular.woff2': '3d687db9d0fcdf38ece05ee79577e4d6fa60165ce4a9e37eaa15f8cc38e58d2c',
};

const METADATA_FILENAME = 'quran-data.js';
const METADATA_SHA256 = '9e9930c592aaa34b5c7a79ff4e99ffdc782263a266090bfe2c49e4ed4bac4ca4';
const GENERATED_PATH = 'src/metadata_generated.rs';
const SURA_COUNT = 114;
const AYAH_COUNT = 6236;

const problems = [];
const notes = [];

function fail(message) {
  problems.push(message);
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

// ── Pins ────────────────────────────────────────────────────────────────────

function checkAssetPins() {
  for (const [relative, expected] of Object.entries(ASSET_PINS)) {
    const path = join(REPO, relative);
    if (!existsSync(path)) {
      fail(`${relative}: missing`);
      continue;
    }
    const actual = sha256(readFileSync(path));
    if (actual !== expected) {
      fail(`${relative}: SHA-256 ${actual} does not match the pin ${expected}`);
      continue;
    }
    notes.push(`${relative}: pinned SHA-256 matches`);
  }
}

// ── Upstream metadata ───────────────────────────────────────────────────────

function candidateDataDirs(explicit) {
  const dirs = [];
  if (explicit) dirs.push(resolve(explicit));
  if (process.env.WIN_QURAN_DATA_DIR) dirs.push(resolve(process.env.WIN_QURAN_DATA_DIR));
  const home = homedir();
  dirs.push(join(home, '.zcode', 'cli', 'plugins', 'marketplaces', 'quran-turn', 'data'));
  const cacheRoot = join(home, '.zcode', 'cli', 'plugins', 'cache', 'quran-turn', 'quran-turn');
  try {
    for (const entry of readdirSync(cacheRoot)) dirs.push(join(cacheRoot, entry, 'data'));
  } catch {
    // The plugin has never been installed through that layout; the other
    // candidates still apply.
  }
  return dirs;
}

// Returns the first candidate whose metadata file matches the pin, or null.
function readUpstreamMetadata(explicit) {
  const tried = [];
  for (const dir of candidateDataDirs(explicit)) {
    const path = join(dir, METADATA_FILENAME);
    if (!existsSync(path)) continue;
    const bytes = readFileSync(path);
    const digest = sha256(bytes);
    if (digest !== METADATA_SHA256) {
      tried.push(`${path} (SHA-256 ${digest} does not match the pin)`);
      continue;
    }
    notes.push(`${path}: pinned SHA-256 matches`);
    return { path, source: bytes.toString('utf8') };
  }
  fail(
    `no ${METADATA_FILENAME} with the pinned SHA-256 was found. Pass --data-dir <path> or set ` +
      `QURAN_TURN_DATA_DIR. Checked: ${tried.length ? tried.join('; ') : candidateDataDirs(explicit).join('; ')}`,
  );
  return null;
}

function evaluateMetadata(source, filename) {
  const context = vm.createContext({});
  try {
    vm.runInContext(source, context, { filename, timeout: 5000 });
  } catch (error) {
    fail(`${filename}: could not be evaluated: ${error.message}`);
    return null;
  }
  const data = context.QuranData;
  if (!data || typeof data !== 'object') {
    fail(`${filename}: did not define QuranData`);
    return null;
  }
  return data;
}

// The sura table is 1-based with a dummy entry at index 0, and each row is
// [start, ayas, order, rukus, name, tname, ename, type].
function readSuras(data, filename) {
  const rows = data.Sura;
  if (!Array.isArray(rows)) {
    fail(`${filename}: QuranData.Sura is not an array`);
    return null;
  }
  // The table is 1-based and carries two extra rows: an empty placeholder at
  // index 0, and an end sentinel at index SURA_COUNT + 1 whose first field is the
  // total ayat count (so "start of the next sura" lookups work past the last one).
  const expectedRows = SURA_COUNT + 2;
  if (rows.length !== expectedRows) {
    fail(`${filename}: QuranData.Sura has ${rows.length} entries, expected ${expectedRows}`);
    return null;
  }
  if (!Array.isArray(rows[0]) || rows[0].length !== 0) {
    fail(`${filename}: QuranData.Sura[0] should be the empty placeholder, got ${JSON.stringify(rows[0])}`);
    return null;
  }
  const sentinel = rows[SURA_COUNT + 1];
  if (!Array.isArray(sentinel) || sentinel[0] !== AYAH_COUNT) {
    fail(
      `${filename}: QuranData.Sura[${SURA_COUNT + 1}] should end the table at ${AYAH_COUNT} ayat, ` +
        `got ${JSON.stringify(sentinel)}`,
    );
    return null;
  }
  const suras = [];
  let total = 0;
  for (let number = 1; number <= SURA_COUNT; number += 1) {
    const row = rows[number];
    if (!Array.isArray(row) || row.length < 8) {
      fail(`${filename}: QuranData.Sura[${number}] is not an 8 field row`);
      return null;
    }
    const ayahCount = row[1];
    const arabicName = row[4];
    const transliteration = row[5];
    const englishName = row[6];
    if (!Number.isInteger(ayahCount) || ayahCount < 1) {
      fail(`${filename}: sura ${number} has an invalid ayat count (${ayahCount})`);
      return null;
    }
    for (const [label, value] of [['name', arabicName], ['tname', transliteration], ['ename', englishName]]) {
      if (typeof value !== 'string' || value.length === 0) {
        fail(`${filename}: sura ${number} has an invalid ${label}`);
        return null;
      }
    }
    total += ayahCount;
    suras.push({ number, ayahCount, arabicName, transliteration, englishName });
  }
  if (total !== AYAH_COUNT) {
    fail(`${filename}: the suras add up to ${total} ayat, expected ${AYAH_COUNT}`);
    return null;
  }
  notes.push(`${filename}: ${SURA_COUNT} suras, ${total} ayat`);
  return suras;
}

// ── Code generation ─────────────────────────────────────────────────────────

function rustString(value) {
  return `"${String(value).replaceAll('\\', '\\\\').replaceAll('"', '\\"')}"`;
}

function generate(suras) {
  const lines = [
    '// Generated by scripts/convert-quran-data.mjs. Do not edit by hand; change the',
    '// script and regenerate so this file and the script cannot disagree.',
    '//',
    '// Source: Tanzil Quran Metadata (ver 1.0), Copyright (C) 2008-2009 Tanzil.info,',
    '// licensed CC BY 3.0. Generated from the upstream metadata file whose SHA-256 is',
    `// ${METADATA_SHA256}.`,
    '',
    '/// One sura of the mushaf, in mushaf order.',
    'pub struct Sura {',
    '    /// 1-based sura number.',
    '    pub number: u16,',
    '    /// Number of ayat in this sura, as counted by Tanzil.',
    '    pub ayah_count: u16,',
    '    /// Arabic name, exactly as Tanzil ships it.',
    '    pub arabic_name: &\'static str,',
    '    /// Latin transliteration, for example `Al-Baqara`.',
    '    pub transliteration: &\'static str,',
    '    /// English meaning, for example `The Cow`.',
    '    pub english_name: &\'static str,',
    '}',
    '',
    '/// The 114 suras of the mushaf, index 0 is sura 1.',
    'pub static SURAS: &[Sura] = &[',
  ];
  for (const sura of suras) {
    lines.push(
      `    Sura { number: ${sura.number}, ayah_count: ${sura.ayahCount}, ` +
        `arabic_name: ${rustString(sura.arabicName)}, ` +
        `transliteration: ${rustString(sura.transliteration)}, ` +
        `english_name: ${rustString(sura.englishName)} },`,
    );
  }
  lines.push('];', '');
  lines.push('/// Total number of ayat in the mushaf, as counted by Tanzil.');
  lines.push(`pub const AYAH_COUNT: u32 = ${AYAH_COUNT};`);
  lines.push('');
  return lines.join('\n');
}

// ── Entry point ─────────────────────────────────────────────────────────────

const argv = process.argv.slice(2);
const check = argv.includes('--check');
const dataDirIndex = argv.indexOf('--data-dir');
const explicitDataDir = dataDirIndex >= 0 ? argv[dataDirIndex + 1] : undefined;

if (argv.includes('--help') || argv.includes('-h')) {
  console.log('usage: convert-quran-data.mjs [--data-dir <path>] [--check]');
  process.exit(0);
}
if (dataDirIndex >= 0 && !explicitDataDir) {
  console.error('convert-quran-data: --data-dir needs a path');
  process.exit(2);
}

checkAssetPins();

const upstream = readUpstreamMetadata(explicitDataDir);
let generated = null;
if (upstream) {
  const data = evaluateMetadata(upstream.source, upstream.path);
  const suras = data ? readSuras(data, upstream.path) : null;
  if (suras) generated = generate(suras);
}

if (check) {
  if (generated) {
    const path = join(REPO, GENERATED_PATH);
    const normalize = (text) => text.replaceAll('\r\n', '\n');
    if (!existsSync(path)) {
      fail(`${GENERATED_PATH}: missing, run the script without --check`);
    } else if (normalize(readFileSync(path, 'utf8')) !== normalize(generated)) {
      fail(`${GENERATED_PATH}: is not current, run the script without --check`);
    } else {
      notes.push(`${GENERATED_PATH}: current`);
    }
  }
} else if (generated) {
  writeFileSync(join(REPO, GENERATED_PATH), generated);
  notes.push(`${GENERATED_PATH}: written (${generated.split('\n').length - 1} lines)`);
}

for (const note of notes) console.log(`ok    ${note}`);
for (const problem of problems) console.error(`FAIL  ${problem}`);
console.log(problems.length ? `\n${problems.length} problem(s)` : '\nall checks passed');
process.exit(problems.length ? 1 : 0);
