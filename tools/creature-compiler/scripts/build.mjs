import { build } from 'esbuild';
import { readFile, writeFile, mkdir, readdir } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { fileURLToPath, pathToFileURL } from 'node:url';
import path from 'node:path';

const root = fileURLToPath(new URL('../', import.meta.url));
const pin = JSON.parse(await readFile(path.join(root, 'pin.json'), 'utf8'));
for (const [name, hash] of Object.entries(pin.source_files)) {
  const actual = createHash('sha256').update(await readFile(path.join(root, name))).digest('hex');
  if (actual !== hash) throw new Error(`Pinned SpawnForge source changed: ${name}. Update the explicit pin before building.`);
}
await mkdir(path.join(root, 'dist'), { recursive: true });
await build({ absWorkingDir: root, entryPoints: { worker: 'src/worker.mjs', engine: 'src/engine.mjs' },
  outdir: 'dist', outExtension: { '.js': '.mjs' }, bundle: true, platform: 'node',
  format: 'esm', target: 'node22', legalComments: 'linked', logLevel: 'warning' });
const { catalog, blueprintSchema } = await import(pathToFileURL(path.join(root, 'dist/engine.mjs')));
const data = catalog();
data.templates = [];
for (const filename of (await readdir(path.join(root, 'templates'))).filter((f) => f.endsWith('.json')).sort()) {
  const blueprint = JSON.parse(await readFile(path.join(root, 'templates', filename), 'utf8'));
  data.templates.push({ id: filename.slice(0, -5), name: blueprint.name,
    body_plan: blueprint.extends, file: `templates/${filename}` });
}
await writeFile(path.join(root, 'catalog.json'), `${JSON.stringify(data, null, 2)}\n`);
await writeFile(path.join(root, 'blueprint.schema.json'), `${JSON.stringify(blueprintSchema(), null, 2)}\n`);
const notices = ['# Third-party notices\n\nSee UPSTREAM.md for the pinned SpawnForge source and retained upstream metadata.'];
for (const [name, filename] of [['three', 'LICENSE'], ['zod', 'LICENSE']]) {
  notices.push(`\n## ${name}\n\n${await readFile(path.join(root, 'node_modules', name, filename), 'utf8')}`);
}
await writeFile(path.join(root, 'dist/THIRD_PARTY_NOTICES.txt'), `${notices.join('\n')}\n`);
console.log(`Pinned compiler ready: ${Object.keys(pin.source_files).length} source files, ${data.modules.length} modules, ${data.templates.length} templates`);
