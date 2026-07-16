#!/usr/bin/env node
// Thin shim: hands off to the native ccmgr binary downloaded by postinstall.js.
// stdio must stay "inherit" so the TUI can control the real terminal.

const path = require("path");
const { spawnSync } = require("child_process");

const bin = path.join(__dirname, "ccmgr-bin");
const result = spawnSync(bin, process.argv.slice(2), { stdio: "inherit" });

if (result.error) {
  console.error(`ccmgr: failed to launch native binary: ${result.error.message}`);
  process.exit(1);
}

process.exit(result.status === null ? 1 : result.status);
