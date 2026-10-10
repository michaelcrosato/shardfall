// Portable entry point. Build once with `npm ci && npm run build` in this directory.
try {
  await import('./dist/worker.mjs');
} catch (error) {
  process.stderr.write(`Creature compiler is not ready: ${error.message}\nRun npm ci && npm run build in tools/creature-compiler.\n`);
  process.exitCode = 1;
}
