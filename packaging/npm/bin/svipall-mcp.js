#!/usr/bin/env node
// A shim, not a wrapper: it execs the real binary with this process's arguments and stdio, so
// stdin stays a pipe and an MCP client sees no difference from running the binary directly.
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const exe = path.join(__dirname, '..', 'vendor', 'svipall-mcp' + (process.platform === 'win32' ? '.exe' : ''));
const r = spawnSync(exe, process.argv.slice(2), { stdio: 'inherit' });
if (r.error) {
  console.error('svipall: ' + r.error.message);
  console.error('svipall: the postinstall download may not have run; try reinstalling the package.');
  process.exit(1);
}
process.exit(r.status === null ? 1 : r.status);
