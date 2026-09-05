// Download the release build for this platform.
//
// npm is not how this is built; it is how a lot of people already run an MCP server, and `npx
// svipall-mcp` is the cheapest possible answer to "how do I try this". So the package carries no
// code of its own: it fetches the same archive install.sh does, checks it against the same
// published sha256sums.txt, and unpacks it next to these two shims.
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const crypto = require('node:crypto');
const { execFileSync } = require('node:child_process');

const REPO = 'ilien-dev/svipall';
const VERSION = require('./package.json').version;

const TARGETS = {
  'linux-x64': 'x86_64-unknown-linux-gnu',
  'linux-arm64': 'aarch64-unknown-linux-gnu',
  'darwin-x64': 'x86_64-apple-darwin',
  'darwin-arm64': 'aarch64-apple-darwin',
  'win32-x64': 'x86_64-pc-windows-msvc',
};

async function download(url) {
  const res = await fetch(url, { redirect: 'follow' });
  if (!res.ok) throw new Error(`${res.status} ${res.statusText} for ${url}`);
  return Buffer.from(await res.arrayBuffer());
}

async function main() {
  const key = `${process.platform}-${process.arch}`;
  const target = TARGETS[key];
  if (!target) {
    // Not a hard failure: npm install should not break somebody's whole project over this.
    console.error(`svipall: no release build for ${key}.`);
    console.error('svipall: see https://github.com/' + REPO + '/blob/main/docs/install.md');
    return;
  }

  const ext = process.platform === 'win32' ? 'zip' : 'tar.gz';
  const name = `svipall-${VERSION}-${target}.${ext}`;
  const base = `https://github.com/${REPO}/releases/download/v${VERSION}`;
  const dest = path.join(__dirname, 'vendor');
  fs.mkdirSync(dest, { recursive: true });

  console.error(`svipall: downloading ${name}`);
  const archive = await download(`${base}/${name}`);

  // Verified, or the reason it could not be. A silent skip is how a swapped download becomes a
  // binary somebody runs.
  try {
    const sums = (await download(`${base}/sha256sums.txt`)).toString('utf8');
    const line = sums.split('\n').find((l) => l.trim().endsWith(name));
    const want = line && line.trim().split(/\s+/)[0];
    const got = crypto.createHash('sha256').update(archive).digest('hex');
    if (!want) {
      console.error(`svipall: warning: ${name} is not listed in sha256sums.txt; not verified`);
    } else if (want.toLowerCase() !== got) {
      throw new Error(`checksum mismatch for ${name} (expected ${want}, got ${got})`);
    }
  } catch (e) {
    if (String(e.message).startsWith('checksum mismatch')) throw e;
    console.error(`svipall: warning: could not verify the download (${e.message})`);
  }

  const file = path.join(dest, name);
  fs.writeFileSync(file, archive);
  // tar is in Windows since 2018 and everywhere else forever; PowerShell handles the zip.
  if (ext === 'zip') {
    execFileSync('powershell', ['-NoProfile', '-Command',
      `Expand-Archive -LiteralPath '${file}' -DestinationPath '${dest}' -Force`], { stdio: 'inherit' });
  } else {
    execFileSync('tar', ['-xzf', file, '-C', dest], { stdio: 'inherit' });
  }
  fs.rmSync(file, { force: true });

  if (process.platform !== 'win32') {
    for (const b of ['svipall', 'svipall-mcp']) {
      const p = path.join(dest, b);
      if (fs.existsSync(p)) fs.chmodSync(p, 0o755);
    }
  }
  console.error('svipall: installed. Run `npx svipall doctor` to see what it can do here.');
}

main().catch((e) => {
  console.error(`svipall: ${e.message}`);
  console.error('svipall: install it another way - https://github.com/' + REPO + '/blob/main/docs/install.md');
  process.exitCode = 1;
});
