/** Pinned SpawnForge -> Shardfall native data adapter, format 1. No renderer or DOM. */
import {
  applyPatch, bakeClips, bakeVertexColors, blueprintSchemaFor, compileCreature,
  createRegistry, generate, skinMaterialSpec, validateBlueprint,
} from '@spawnforge/core';
import { basicPack } from '@spawnforge/modules';
import { z } from 'zod';

export const GENERATOR_REVISION = '851880256987ecdb2895c6afd01f84df64199bdb';
export const ADAPTER_FORMAT = 1;
export const registry = createRegistry([basicPack]);
const cache = new Map();
const qualityNames = ['low', 'medium', 'high'];
const array = (a) => Array.from(a);
const stable = (value) => JSON.stringify(value, (_key, v) =>
  v && !Array.isArray(v) && typeof v === 'object'
    ? Object.fromEntries(Object.keys(v).sort().map((k) => [k, v[k]])) : v);
const issueText = (issue) => typeof issue === 'string' ? issue
  : `${issue.path ? `${issue.path}: ` : ''}${issue.message}${issue.fix ? ` (${issue.fix})` : ''}`;

class BuildError extends Error {
  constructor(message, issues = []) { super(message); this.issues = issues; }
}

export function catalog(request = {}) {
  if (request.kind && !registry.list().some((m) => m.kind === request.kind)) {
    throw new BuildError(`Unknown module kind: ${request.kind}`);
  }
  const modules = registry.catalog().filter((m) =>
    (!request.kind || m.kind === request.kind) && (!request.module || m.id === request.module))
    .map((entry) => {
      const module = registry.get(entry.kind, entry.id);
      const defaults = module.params.safeParse({});
      return {
        ...entry,
        ...(defaults.success ? { defaults: defaults.data } : {}),
        ...(module.example ? { example: module.example } : {}),
        ...(module.preset ? { blueprint: module.preset } : {}),
        ...(module.slot ? { slot: module.slot } : {}),
        ...(module.attach ? { attach: module.attach } : {}),
        ...(module.needs ? { needs: module.needs } : {}),
      };
    });
  if (request.module && !modules.length) throw new BuildError(`Unknown module: ${request.module}`);
  return {
    format: ADAPTER_FORMAT, generator_revision: GENERATOR_REVISION,
    blueprint_format: 'spawnforge/0.2', qualities: qualityNames, modules,
    defaults: registry.defaults(), hints: registry.hints(),
    patch_operations: ['set', 'add', 'remove', 'mirror', 'scale'],
    examples: [
      { op: 'set', path: 'parts[id=horns].params.length', value: 0.4 },
      { op: 'set', path: 'skin.palette.base', value: '#285a78' },
      { op: 'set', path: 'skin.layers[type=stripes].strength', value: 0.6 },
    ],
  };
}

export function blueprintSchema() {
  return z.toJSONSchema(blueprintSchemaFor(registry), { io: 'input', unrepresentable: 'any' });
}

function encodeMesh(mesh, baked, doubleSided) {
  return {
    positions: array(mesh.positions), normals: array(mesh.normals), indices: array(mesh.indices),
    colors: array(baked.color), skin_indices: array(mesh.skinIndex),
    skin_weights: array(mesh.skinWeight), double_sided: doubleSided,
  };
}

function warningsFor(checked, compiled, colors) {
  const messages = [...checked.warnings, ...(checked.notBuilt ?? []), ...compiled.warnings].map(issueText);
  messages.push('Native surface preview uses baked vertex colors. Fine relief, roughness, animated patterns, and shader-only breathing are not reproduced. Baked bone motion, including jaw and lid tracks, is retained.');
  if (compiled.material.fur) messages.push('Fur is present in the blueprint; shell fur is not drawn in the native preview.');
  if (compiled.membranes.indices.length) messages.push('Membranes are drawn as opaque double-sided surfaces; transmission and transparency are approximated.');
  if (colors.skin.glow > 0) messages.push('Emissive pattern glow is not included in the vertex-color preview.');
  return [...new Set(messages)];
}

export function build(request) {
  const start = performance.now();
  if (!/^[a-z][a-z0-9_-]{0,63}$/.test(request.name ?? '')) {
    throw new BuildError('name must be a safe lowercase leaf: a letter, then up to 63 letters, digits, underscores or hyphens');
  }
  const quality = request.quality ?? 'medium';
  if (!qualityNames.includes(quality)) throw new BuildError('quality must be low, medium, or high');
  if (request.blueprint && request.theme) throw new BuildError('Provide blueprint or theme, not both');
  let input = request.blueprint;
  const generationWarnings = [];
  if (request.theme) {
    if (!Number.isSafeInteger(request.seed ?? 1)) throw new BuildError('seed must be a safe integer');
    const generated = generate({ theme: request.theme, seed: request.seed ?? 1,
      constraints: request.constraints ?? {} }, registry);
    if (!generated.ok) throw new BuildError('The theme could not produce a valid creature', generated.errors);
    input = generated.blueprint;
    generationWarnings.push(...generated.warnings.map(issueText));
  }
  if (!input || typeof input !== 'object' || Array.isArray(input)) {
    throw new BuildError('Provide a blueprint object or a catalog theme');
  }
  if (request.ops !== undefined && !Array.isArray(request.ops)) throw new BuildError('ops must be an array');
  let diff = [];
  if (request.ops?.length) {
    const patched = applyPatch(input, request.ops, registry);
    if (!patched.ok) throw new BuildError('The named edit is not valid', patched.errors);
    input = patched.blueprint;
    diff = patched.diff;
  }
  const checked = validateBlueprint(input, registry);
  if (!checked.ok || !checked.creature) throw new BuildError('The blueprint is not valid', checked.errors);
  const spec = checked.creature;
  const validated = performance.now();

  // Parts and membranes can use palette colors when their geometry is built. Keep the
  // palette and material in this key: chitin also changes the skeleton. Skin-layer-only
  // changes reuse geometry and baked motion safely.
  const geometryKey = stable({ ...spec, quality, skin: { palette: spec.skin.palette, material: spec.skin.material } });
  let previous = cache.get(geometryKey);
  let compiled;
  let clips;
  let buildKind = 'full';
  if (previous) {
    cache.delete(geometryKey); cache.set(geometryKey, previous);
    compiled = { ...previous.compiled, material: skinMaterialSpec(spec.skin.palette.base,
      spec.skin.material, spec.skin.layers, spec.seed, spec.skin.fur) };
    clips = previous.clips;
    buildKind = 'surface';
  } else {
    compiled = compileCreature(spec, registry, { quality });
  }
  const geometryDone = performance.now();
  const colors = bakeVertexColors(compiled, registry);
  const colorsDone = performance.now();
  if (!clips) clips = bakeClips(compiled, registry, { fps: 30, idleSeconds: 4 });
  const motionDone = performance.now();
  if (!previous) {
    cache.set(geometryKey, { compiled, clips });
    while (cache.size > 3) cache.delete(cache.keys().next().value);
  }
  const warnings = [...new Set([...generationWarnings, ...warningsFor(checked, compiled, colors)])];
  const bones = compiled.bones;
  const asset = {
    format: ADAPTER_FORMAT, generator_revision: GENERATOR_REVISION,
    name: request.name, title: spec.name, source_revision: 'pending', quality,
    bones: {
      names: [...bones.names], parents: array(bones.parents), positions: array(bones.positions),
      rotations: array(bones.rotations), lengths: array(bones.lengths),
      ...(bones.rest ? { rest: array(bones.rest) } : {}),
    },
    meshes: Object.fromEntries(['skin', 'parts', 'eyes', 'membranes'].map((name) =>
      [name, encodeMesh(compiled[name], colors[name], name === 'membranes')])),
    clips: Object.fromEntries(clips.map((clip) => [clip.name, {
      duration: clip.duration, looping: clip.loop, frames: clip.frames,
      positions: array(clip.positions), rotations: array(clip.rotations),
      root_motion: clip.rootMotion ?? false, speed: clip.speed, distance: clip.distance,
      events: clip.events,
    }])),
    bounds: compiled.bounds, sockets: compiled.sockets, warnings,
  };
  const encoded = performance.now();
  return { blueprint: checked.blueprint, asset, warnings, diff, build_kind: buildKind,
    compile_ms: encoded - start,
    timings: { validate_ms: validated - start, geometry_ms: geometryDone - validated,
      colors_ms: colorsDone - geometryDone, motion_ms: motionDone - colorsDone,
      adapter_ms: encoded - motionDone },
    stats: { ...compiled.stats, clips: clips.length, frames: clips.reduce((n, c) => n + c.frames, 0) },
  };
}

export function handle(request) {
  const id = request?.id ?? null;
  try {
    if (!request || typeof request !== 'object' || Array.isArray(request)) throw new BuildError('Request must be an object');
    let result;
    if (request.op === 'catalog') result = catalog(request);
    else if (request.op === 'build') result = build(request);
    else throw new BuildError(`Unknown compiler operation: ${request.op}`);
    return { id, ok: true, result };
  } catch (error) {
    return { id, ok: false, error: error.message ?? String(error), issues: error.issues ?? [] };
  }
}
