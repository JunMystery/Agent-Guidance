#!/usr/bin/env node

const { run } = require('../lib/runner');

run(process.argv.slice(2)).catch((err) => {
  console.error('[agent-guidance error]', err.message || err);
  process.exit(1);
});
