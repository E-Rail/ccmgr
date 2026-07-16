#!/usr/bin/env node
// Downloads the prebuilt `ccmgr` binary matching this platform from GitHub
// Releases, for the version tag matching this package's own version.
// Node built-ins only - no extra npm dependencies.

const fs = require("fs");
const https = require("https");
const os = require("os");
const path = require("path");
const { execFileSync } = require("child_process");

const REPO = "E-Rail/ccmgr";
const pkg = require("./package.json");

function targetTriple() {
  const platform = process.platform;
  const arch = process.arch;

  if (platform === "darwin" && arch === "x64") return "x86_64-apple-darwin";
  if (platform === "darwin" && arch === "arm64") return "aarch64-apple-darwin";
  if (platform === "linux" && arch === "x64") return "x86_64-unknown-linux-gnu";
  if (platform === "linux" && arch === "arm64") return "aarch64-unknown-linux-gnu";

  console.error(`ccmgr: unsupported platform/arch: ${platform}/${arch}`);
  process.exit(1);
}

function download(url, dest, redirectsLeft = 5) {
  return new Promise((resolve, reject) => {
    https
      .get(url, { headers: { "User-Agent": "ccmgr-postinstall" } }, (res) => {
        if (
          res.statusCode >= 300 &&
          res.statusCode < 400 &&
          res.headers.location &&
          redirectsLeft > 0
        ) {
          res.resume();
          resolve(download(res.headers.location, dest, redirectsLeft - 1));
          return;
        }
        if (res.statusCode !== 200) {
          reject(new Error(`request to ${url} failed with status ${res.statusCode}`));
          return;
        }
        const file = fs.createWriteStream(dest);
        res.pipe(file);
        file.on("finish", () => file.close(resolve));
        file.on("error", reject);
      })
      .on("error", reject);
  });
}

async function main() {
  const target = targetTriple();
  const asset = `ccmgr-${target}.tar.gz`;
  const url = `https://github.com/${REPO}/releases/download/v${pkg.version}/${asset}`;

  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "ccmgr-install-"));
  const tarPath = path.join(tmpDir, asset);
  const binDir = path.join(__dirname, "bin");
  const destBin = path.join(binDir, "ccmgr-bin");

  console.log(`ccmgr: downloading ${asset} from ${url}`);
  await download(url, tarPath);

  execFileSync("tar", ["-xzf", tarPath, "-C", tmpDir]);

  const extracted = path.join(tmpDir, "ccmgr");
  if (!fs.existsSync(extracted)) {
    throw new Error(`expected binary "ccmgr" not found after extracting ${asset}`);
  }

  fs.mkdirSync(binDir, { recursive: true });
  fs.copyFileSync(extracted, destBin);
  fs.chmodSync(destBin, 0o755);
  fs.rmSync(tmpDir, { recursive: true, force: true });

  console.log("ccmgr: installed successfully");
}

main().catch((err) => {
  console.error(`ccmgr: install failed: ${err.message}`);
  process.exit(1);
});
