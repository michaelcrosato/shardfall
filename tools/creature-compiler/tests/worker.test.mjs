import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import readline from 'node:readline';
import { build, handle, GENERATOR_REVISION } from '../dist/engine.mjs';

const template = JSON.parse(await readFile(new URL('../templates/ridgeback_stalker.json', import.meta.url)));
const request = { op: 'build', name: 'contract_ridgeback', quality: 'low', blueprint: template };

function contract(asset) {
  assert.equal(asset.format, 1);
  assert.equal(asset.generator_revision, GENERATOR_REVISION);
  const count = asset.bones.names.length;
  assert(count > 1);
  assert.equal(asset.bones.parents.length, count);
  assert.equal(asset.bones.positions.length, count * 3);
  assert.equal(asset.bones.rotations.length, count * 4);
  asset.bones.parents.forEach((parent, i) => assert(parent >= -1 && parent < i));
  for (const mesh of Object.values(asset.meshes)) {
    const vertices = mesh.positions.length / 3;
    assert.equal(mesh.normals.length, vertices * 3);
    assert.equal(mesh.colors.length, vertices * 3);
    assert.equal(mesh.skin_indices.length, vertices * 4);
    assert.equal(mesh.skin_weights.length, vertices * 4);
    assert.equal(mesh.indices.length % 3, 0);
    mesh.indices.forEach((i) => assert(i >= 0 && i < vertices));
    for (let v = 0; v < vertices; v++) {
      const weights = mesh.skin_weights.slice(v * 4, v * 4 + 4);
      assert(Math.abs(weights.reduce((a, b) => a + b, 0) - 1) < 0.001);
      mesh.skin_indices.slice(v * 4, v * 4 + 4).forEach((i) => assert(i < count));
    }
  }
  for (const clip of Object.values(asset.clips)) {
    assert(clip.duration > 0 && clip.frames > 1);
    assert.equal(clip.positions.length, clip.frames * count * 3);
    assert.equal(clip.rotations.length, clip.frames * count * 4);
    if (clip.looping) {
      const stride = count * 3;
      assert.deepEqual(clip.positions.slice(0, stride), clip.positions.slice(-stride));
    }
  }
  JSON.stringify(asset, (_key, value) => {
    if (typeof value === 'number') assert(Number.isFinite(value));
    return value;
  });
}

test('pinned native adapter preserves skeleton, skin, clips and deterministic results', () => {
  const first = build(request);
  contract(first.asset);
  assert(first.asset.clips.walk);
  assert(first.asset.clips.bite);
  const again = build(request);
  assert.equal(again.build_kind, 'surface');
  assert.deepEqual(again.asset, first.asset);
});

test('named part and surface patches work without changing the input; bad patches fail with paths', () => {
  const before = JSON.stringify(template);
  const original = build(request);
  const part = build({ ...request, ops: [{ op: 'set', path: 'parts[id=horns].params.length', value: 0.43 }] });
  assert.equal(part.blueprint.parts.find((p) => p.id === 'horns').params.length, 0.43);
  assert.equal(part.build_kind, 'full');
  const surface = build({ ...request, ops: [{ op: 'set', path: 'skin.layers[type=countershade].strength', value: 0.12 }] });
  assert.equal(surface.build_kind, 'surface');
  assert.deepEqual(surface.asset.meshes.skin.positions, original.asset.meshes.skin.positions);
  assert.notDeepEqual(surface.asset.meshes.skin.colors, original.asset.meshes.skin.colors);
  assert.deepEqual(surface.asset.clips, original.asset.clips);
  assert.equal(JSON.stringify(template), before);
  const bad = handle({ ...request, id: 4, ops: [{ op: 'set', path: 'parts[id=missing].params.length', value: 1 }] });
  assert.equal(bad.ok, false);
  assert(bad.issues.length > 0);
  assert(bad.issues.some((issue) => issue.path || issue.fix));
});

test('chitin material changes invalidate geometry and motion caches', () => {
  build(request);
  const changed = build({ ...request, ops: [{ op: 'set', path: 'skin.material', value: 'chitin' }] });
  assert.equal(changed.build_kind, 'full');
  contract(changed.asset);
});

test('winged template carries rest rotations and double-sided membranes', async () => {
  const blueprint = JSON.parse(await readFile(new URL('../templates/cave_bat.json', import.meta.url)));
  const result = build({ ...request, blueprint, name: 'contract_bat' });
  contract(result.asset);
  assert.equal(result.asset.bones.rest.length, result.asset.bones.names.length * 4);
  assert(result.asset.meshes.membranes.positions.length > 0);
  assert.equal(result.asset.meshes.membranes.double_sided, true);
  assert(result.asset.clips.fly);
});

test('theme with fixed seed generates a valid reproducible blueprint', () => {
  const themed = { op: 'build', name: 'theme_beast', theme: 'beast', seed: 51, quality: 'low', constraints: { bodyPlan: 'quadruped' } };
  const first = build(themed);
  contract(first.asset);
  assert.deepEqual(build(themed).blueprint, first.blueprint);
  const bad = handle({ ...themed, id: 9, theme: 'no_such_theme' });
  assert.equal(bad.ok, false);
});

test('packaged JSONL worker handles errors then valid requests in one process', async () => {
  const child = spawn(process.execPath, [new URL('../worker.mjs', import.meta.url).pathname], { stdio: ['pipe', 'pipe', 'pipe'] });
  const lines = readline.createInterface({ input: child.stdout });
  const replies = [];
  let errors = '';
  child.stderr.on('data', (s) => { errors += s; });
  child.stdin.end('not json\n' + JSON.stringify({ id: 2, op: 'unknown' }) + '\n' + JSON.stringify({ id: 3, op: 'catalog', kind: 'theme' }) + '\n');
  for await (const line of lines) replies.push(JSON.parse(line));
  const code = await new Promise((resolve) => child.exitCode === null ? child.once('exit', resolve) : resolve(child.exitCode));
  assert.equal(code, 0, errors);
  assert.equal(replies.length, 3);
  assert.equal(replies[0].ok, false);
  assert.equal(replies[1].id, 2);
  assert.equal(replies[1].ok, false);
  assert.equal(replies[2].id, 3);
  assert.equal(replies[2].ok, true);
  assert(replies[2].result.modules.every((module) => module.kind === 'theme'));
});
