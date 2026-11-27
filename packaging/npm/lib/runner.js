const fs = require('fs');
const path = require('path');
const os = require('os');
const https = require('https');
const crypto = require('crypto');
const { spawn, execFileSync } = require('child_process');

const VERSION = require('../package.json').version;
const REPO = 'JunMystery/Agent-Guidance';

function getTargetInfo() {
  const platform = os.platform();
  const arch = os.arch();

  if (platform === 'win32' && arch === 'x64') {
    return {
      asset: 'agent-guidance-windows-x86_64.zip',
      binName: 'agent-guidance.exe',
      isZip: true,
    };
  }
  if (platform === 'darwin' && (arch === 'arm64' || arch === 'aarch64')) {
    return {
      asset: 'agent-guidance-macos-aarch64.tar.gz',
      binName: 'agent-guidance',
      isZip: false,
    };
  }
  if (platform === 'darwin' && arch === 'x64') {
    return {
      asset: 'agent-guidance-macos-x86_64.tar.gz',
      binName: 'agent-guidance',
      isZip: false,
    };
  }
  if (platform === 'linux' && arch === 'x64') {
    return {
      asset: 'agent-guidance-linux-x86_64.tar.gz',
      binName: 'agent-guidance',
      isZip: false,
    };
  }

  throw new Error(`Unsupported platform or architecture: ${platform}-${arch}. Build from source: cargo install --git https://github.com/${REPO}`);
}

function getCacheDir() {
  const home = os.homedir();
  if (process.platform === 'win32') {
    const localApp = process.env.LOCALAPPDATA || path.join(home, 'AppData', 'Local');
    return path.join(localApp, 'agent-guidance', 'bin', `v${VERSION}`);
  }
  return path.join(home, '.agent-guidance', 'bin', `v${VERSION}`);
}

function syncSystemPath(binPath) {
  if (process.platform !== 'win32') return;
  try {
    const localApp = process.env.LOCALAPPDATA || path.join(os.homedir(), 'AppData', 'Local');
    const targetDir = path.join(localApp, 'Programs', 'agent-guidance', 'bin');
    if (!fs.existsSync(targetDir)) {
      fs.mkdirSync(targetDir, { recursive: true });
    }
    const targetFile = path.join(targetDir, 'agent-guidance.exe');
    try {
      fs.copyFileSync(binPath, targetFile);
    } catch (_) {
      const oldFile = `${targetFile}.old`;
      try { if (fs.existsSync(oldFile)) fs.unlinkSync(oldFile); } catch (_) {}
      try {
        fs.renameSync(targetFile, oldFile);
        fs.copyFileSync(binPath, targetFile);
      } catch (_) {}
    }
  } catch (_) {}
}

function downloadFile(url, destPath) {
  return new Promise((resolve, reject) => {
    const file = fs.createWriteStream(destPath);
    function get(currentUrl) {
      https.get(currentUrl, (res) => {
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          return get(res.headers.location);
        }
        if (res.statusCode !== 200) {
          return reject(new Error(`Download failed with HTTP ${res.statusCode} from ${currentUrl}`));
        }
        res.pipe(file);
        file.on('finish', () => file.close(resolve));
      }).on('error', (err) => {
        fs.unlink(destPath, () => {});
        reject(err);
      });
    }
    get(url);
  });
}

function fetchTextOptional(url) {
  return new Promise((resolve) => {
    function get(currentUrl) {
      https.get(currentUrl, (res) => {
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          return get(res.headers.location);
        }
        if (res.statusCode !== 200) {
          res.resume();
          return resolve(null);
        }
        let body = '';
        res.setEncoding('utf8');
        res.on('data', (chunk) => { body += chunk; });
        res.on('end', () => resolve(body));
      }).on('error', () => resolve(null));
    }
    get(url);
  });
}

async function verifySha256IfAvailable(filePath, shaUrl) {
  const raw = await fetchTextOptional(shaUrl);
  if (!raw) return;
  const expected = (raw.trim().split(/\s+/)[0] || '').toLowerCase();
  if (!/^[a-f0-9]{64}$/.test(expected)) return;
  const actual = crypto.createHash('sha256').update(fs.readFileSync(filePath)).digest('hex').toLowerCase();
  if (actual !== expected) {
    try { fs.unlinkSync(filePath); } catch (_) {}
    throw new Error(`SHA-256 verification failed for ${path.basename(filePath)} (${actual} !== ${expected})`);
  }
}

function extractArchive(archivePath, targetDir, isZip) {
  if (isZip) {
    if (process.platform === 'win32') {
      try {
        execFileSync('tar.exe', ['-xf', archivePath, '-C', targetDir], { stdio: 'ignore' });
        return;
      } catch (_) {
        execFileSync(
          'powershell',
          [
            '-NoProfile',
            '-NonInteractive',
            '-Command',
            '& { param($src,$dst) Expand-Archive -LiteralPath $src -DestinationPath $dst -Force }',
            archivePath,
            targetDir,
          ],
          { stdio: 'inherit' }
        );
        return;
      }
    }
    execFileSync('unzip', ['-q', '-o', archivePath, '-d', targetDir], { stdio: 'inherit' });
  } else {
    execFileSync('tar', ['-xzf', archivePath, '-C', targetDir], { stdio: 'inherit' });
  }
}

async function ensureBinary() {
  const target = getTargetInfo();
  const cacheDir = getCacheDir();
  const binPath = path.join(cacheDir, target.binName);

  if (fs.existsSync(binPath)) {
    syncSystemPath(binPath);
    return binPath;
  }

  fs.mkdirSync(cacheDir, { recursive: true });
  const archivePath = path.join(cacheDir, target.asset);
  const downloadUrl = `https://github.com/${REPO}/releases/download/v${VERSION}/${target.asset}`;

  process.stderr.write(`[agent-guidance] Downloading v${VERSION} (${target.asset})...\n`);
  await downloadFile(downloadUrl, archivePath);
  await verifySha256IfAvailable(archivePath, `${downloadUrl}.sha256`);

  process.stderr.write(`[agent-guidance] Extracting ${target.asset}...\n`);
  extractArchive(archivePath, cacheDir, target.isZip);

  if (fs.existsSync(archivePath)) {
    try { fs.unlinkSync(archivePath); } catch (_) {}
  }

  if (process.platform !== 'win32' && fs.existsSync(binPath)) {
    fs.chmodSync(binPath, 0o755);
  }

  if (!fs.existsSync(binPath)) {
    throw new Error(`Extracted archive did not contain expected binary: ${binPath}`);
  }

  syncSystemPath(binPath);
  return binPath;
}

function spawnBinary(binPath, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(binPath, args, {
      stdio: 'inherit',
      env: process.env,
    });

    child.on('error', (err) => {
      console.error(`[agent-guidance error] Failed to execute ${binPath}:`, err.message);
      reject(err);
    });

    child.on('close', (code, signal) => {
      if (signal) {
        process.kill(process.pid, signal);
      } else if (code !== 0) {
        process.exit(code ?? 1);
      } else {
        resolve();
      }
    });

    ['SIGINT', 'SIGTERM', 'SIGHUP'].forEach((sig) => {
      process.on(sig, () => {
        if (child.pid) {
          try { child.kill(sig); } catch (_) {}
        }
      });
    });
  });
}

async function run(args) {
  const binPath = await ensureBinary();
  const isInteractive = args.includes('--interactive') || args.includes('-i');

  if (isInteractive || (args.length === 0 && process.stdin.isTTY)) {
    const { promptMenu } = require('./wizard');
    const steps = await promptMenu();
    for (const stepArgs of steps) {
      await spawnBinary(binPath, stepArgs);
    }
    return;
  }

  const effectiveArgs = args.length === 0 ? ['--setup'] : args;
  await spawnBinary(binPath, effectiveArgs);
}

module.exports = { run, ensureBinary, getTargetInfo, getCacheDir, syncSystemPath };
