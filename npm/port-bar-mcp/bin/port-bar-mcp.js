#!/usr/bin/env node
'use strict';

const path = require('path');
const { spawnSync } = require('child_process');

const PLATFORM_PACKAGES = {
  'darwin-arm64': '@gooliya/port-bar-mcp-darwin-arm64',
  'darwin-x64': '@gooliya/port-bar-mcp-darwin-x64',
};

function resolveBinaryPath() {
  const key = `${process.platform}-${process.arch}`;
  const packageName = PLATFORM_PACKAGES[key];
  if (!packageName) return null;

  try {
    const packageJsonPath = require.resolve(`${packageName}/package.json`);
    return path.join(path.dirname(packageJsonPath), 'port-bar-mcp');
  } catch (err) {
    return null;
  }
}

function fail(message) {
  process.stderr.write(`port-bar-mcp: ${message}\n`);
  process.exit(1);
}

const binaryPath = resolveBinaryPath();

if (!binaryPath) {
  fail(
    `no prebuilt binary for this platform (${process.platform}-${process.arch}). ` +
      'Only macOS is supported (Apple Silicon / arm64 or Intel / x64).'
  );
}

const result = spawnSync(binaryPath, process.argv.slice(2), { stdio: 'inherit' });

if (result.error) {
  fail(`failed to launch the port-bar-mcp binary (${result.error.message}).`);
}

process.exit(result.status === null ? 1 : result.status);
