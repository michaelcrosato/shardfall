import { colorRef, definePart } from '@spawnforge/core';
import { Vector3 } from 'three';
import { z } from 'zod';

const params = z.strictObject({
  toes: z.number().int().min(1).max(6).default(3).describe('Toes (or fingers) per foot'),
  toeLength: z.number().min(0.01).max(0.5).default(0.08).describe('Toe length in torso lengths'),
  spread: z.number().min(0).max(150).default(50).describe('Degrees between the outermost toes'),
  clawLength: z
    .number()
    .min(0)
    .max(0.3)
    .default(0.035)
    .describe('Claw length in torso lengths; 0 for none'),
  clawCurve: z.number().min(0).max(180).default(70).describe('Degrees each claw bends down'),
  clawWidth: z.number().min(0.2).max(3).default(1).describe('Claw thickness relative to the toe'),
  clawColor: colorRef('#2a221c').describe('Claw colour: a palette name or a colour'),
});
type Params = z.output<typeof params>;

const Y = new Vector3(0, 1, 0);

export default definePart({
  id: 'foot.claw',
  summary:
    'Foot or hand of short toes, each tipped with a curved claw; set it as a limb\'s "foot".',
  tags: ['foot', 'hand', 'weapon'],
  slot: 'foot',
  material: 'horn',
  attach: { on: 'limb' },
  params,
  example: { type: 'foot.claw', toes: 3, clawLength: 0.05 },
  describe: (p) => `${p.toes as number}-toed clawed feet`,
  hooks: {
    claws: (raw) => {
      const p = raw as Params;
      return { count: p.clawLength > 0 ? p.toes : 0, length: p.clawLength };
    },
    toes(ctx, raw) {
      const p = raw as Params;
      const length = p.toeLength * ctx.scale;
      const radius = Math.min(ctx.tipRadius * (p.toes > 1 ? 0.42 : 0.6), length * 0.22);
      const chains = [];
      for (let i = 0; i < p.toes; i++) {
        const offset = p.toes > 1 ? (i / (p.toes - 1) - 0.5) * p.spread : 0;
        if (ctx.role === 'leg') {
          // Toes fan out over the ground from the ankle, turned outward on sprawlers.
          const centre = ctx.forward
            .clone()
            .addScaledVector(ctx.outward, Math.sin((ctx.splay * Math.PI) / 180) * 0.7)
            .normalize();
          const dir = centre.applyAxisAngle(Y, (offset * Math.PI) / 180);
          const start = ctx.ankle.clone().addScaledVector(dir, ctx.tipRadius * 0.3);
          const knuckle = ctx.ankle.clone().addScaledVector(dir, length * 0.45);
          knuckle.y = ctx.groundY + radius * 1.1;
          const tip = knuckle.clone().addScaledVector(dir, length * 0.55);
          tip.y = ctx.groundY + radius * 0.8;
          chains.push({
            points: [start, knuckle, tip],
            radii: [radius * 1.15, radius, radius * 0.8],
          });
        } else {
          // Fingers continue the arm, fanned across it and curling forward.
          const across = new Vector3().crossVectors(ctx.limbDir, ctx.outward).normalize();
          const dir = ctx.limbDir
            .clone()
            .applyAxisAngle(across.lengthSq() > 0 ? ctx.outward : Y, (offset * Math.PI) / 180)
            .normalize();
          const start = ctx.ankle.clone();
          const knuckle = start.clone().addScaledVector(dir, length * 0.5);
          const curl = dir.clone().addScaledVector(ctx.forward, 0.6).normalize();
          const tip = knuckle.clone().addScaledVector(curl, length * 0.5);
          chains.push({
            points: [start, knuckle, tip],
            radii: [radius * 1.1, radius, radius * 0.8],
          });
        }
      }
      return chains;
    },
    build(ctx, raw) {
      const p = raw as Params;
      if (p.clawLength <= 0) return;
      const length = p.clawLength * ctx.scale;
      const color = ctx.color(p.clawColor, '#2a221c');
      for (const toe of ctx.toes) {
        const base = Math.max(toe.toeRadius * 0.8, length * 0.12) * p.clawWidth;
        // Start raised by half the curve, so the tip ends level with the toe, not in the ground.
        const path = ctx.geo.arc(length, p.clawCurve, { segments: 6, lean: p.clawCurve * 0.5 });
        const piece = ctx.geo.sweep(path, (t) => base * (1 - t * 0.92), { sides: 7, tip: 'point' });
        ctx.emit(piece, toe, { color, tipColor: '#120e0b', sink: base * 0.6, bone: toe.bone });
      }
    },
  },
});
