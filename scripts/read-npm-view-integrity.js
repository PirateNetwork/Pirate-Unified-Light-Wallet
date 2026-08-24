#!/usr/bin/env node

const fs = require('fs');

function fail(message) {
  process.stderr.write(`[read-npm-view-integrity][ERROR] ${message}\n`);
  process.exit(1);
}

if (process.argv.length !== 3) {
  fail('Usage: read-npm-view-integrity.js <npm-view-json>');
}

let parsed;
try {
  parsed = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
} catch (error) {
  fail(`Could not parse npm view response: ${error.message}`);
}

// npm 10 emits a JSON string for an exact package version. npm 12 may wrap
// that same result in a single-element array. Accept both shapes while
// rejecting ambiguous multi-result responses instead of silently choosing one.
const values = Array.isArray(parsed) ? parsed : [parsed];
if (values.length !== 1) {
  fail(`Expected exactly one registry integrity, received ${values.length}`);
}

const [integrity] = values;
if (typeof integrity !== 'string') {
  fail(`Expected a registry integrity string, received ${typeof integrity}`);
}
if (!/^sha512-[A-Za-z0-9+/]+={0,2}$/.test(integrity)) {
  fail('Registry integrity is not a valid SHA-512 Subresource Integrity value');
}

process.stdout.write(integrity);
