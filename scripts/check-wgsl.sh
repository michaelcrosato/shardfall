#!/usr/bin/env bash
# Compiles every WGSL module the renderer uses with the browser's WebGPU (Tint, in headless
# Chromium on SwiftShader): naga, which wgpu uses natively, accepts code that Tint rejects, and
# the browser build runs on Tint. Run it after every WGSL change.
#
#   scripts/check-wgsl.sh            # dump (cargo example dump_wgsl), then compile
#   scripts/check-wgsl.sh DIR        # compile the *.wgsl files already in DIR
#
# Needs Node with Playwright (PW_NODE_MODULES, default /opt/node-tools/node_modules) and its
# Chromium (PLAYWRIGHT_BROWSERS_PATH, default /opt/pw-browsers). Exits non-zero on any error.
set -euo pipefail
cd "$(dirname "$0")/.."

dir="${1:-}"
if [ -z "$dir" ]; then
  dir="$(mktemp -d)"
  trap 'rm -rf "$dir"' EXIT
  cargo run -q -p pav_render --example dump_wgsl -- "$dir" >/dev/null
fi

export NODE_PATH="${PW_NODE_MODULES:-/opt/node-tools/node_modules}${NODE_PATH:+:$NODE_PATH}"
export PLAYWRIGHT_BROWSERS_PATH="${PLAYWRIGHT_BROWSERS_PATH:-/opt/pw-browsers}"

node - "$dir" <<'JS'
const { chromium } = require('playwright');
const fs = require('fs');
const http = require('http');
const path = require('path');

(async () => {
  const dir = process.argv[2];
  const files = fs.readdirSync(dir).filter((f) => f.endsWith('.wgsl')).sort();
  if (files.length === 0) throw new Error(`no .wgsl files in ${dir}`);
  const sources = files.map((f) => [f, fs.readFileSync(path.join(dir, f), 'utf8')]);
  // WebGPU needs a secure context: serve a blank page from localhost.
  const server = http.createServer((_, res) => {
    res.writeHead(200, { 'content-type': 'text/html' });
    res.end('<!doctype html><title>check-wgsl</title>');
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const browser = await chromium.launch({
    args: ['--enable-unsafe-webgpu', '--enable-features=Vulkan', '--use-vulkan=swiftshader', '--use-angle=swiftshader'],
  });
  let failed = false;
  try {
    const page = await browser.newPage();
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    const result = await page.evaluate(async (sources) => {
      if (!navigator.gpu) return { fatal: 'navigator.gpu is missing (WebGPU off?)' };
      const adapter = await navigator.gpu.requestAdapter();
      if (!adapter) return { fatal: 'no WebGPU adapter' };
      const device = await adapter.requestDevice();
      const modules = [];
      for (const [name, code] of sources) {
        device.pushErrorScope('validation');
        const module = device.createShaderModule({ label: name, code });
        const info = await module.getCompilationInfo();
        const error = await device.popErrorScope();
        modules.push({
          name,
          messages: info.messages.map((m) => ({ type: m.type, line: m.lineNum, col: m.linePos, text: m.message })),
          error: error ? error.message : null,
        });
      }
      return { modules };
    }, sources);
    if (result.fatal) {
      console.error(`check-wgsl: ${result.fatal}`);
      failed = true;
    } else {
      for (const m of result.modules) {
        const errors = m.messages.filter((x) => x.type === 'error');
        const status = errors.length || m.error ? 'FAIL' : 'ok';
        console.log(`${status.padEnd(4)} ${m.name}: ${errors.length} errors, ${m.messages.length - errors.length} other messages`);
        for (const x of m.messages) console.log(`     ${x.type} ${m.name}:${x.line}:${x.col} ${x.text}`);
        if (m.error && errors.length === 0) console.log(`     validation: ${m.error.split('\n')[0]}`);
        if (status === 'FAIL') failed = true;
      }
    }
  } finally {
    await browser.close();
    server.close();
  }
  process.exit(failed ? 1 : 0);
})().catch((e) => {
  console.error(`check-wgsl: ${e.stack || e}`);
  process.exit(2);
});
JS
