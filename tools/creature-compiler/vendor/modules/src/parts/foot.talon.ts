import { colorRef, definePart } from '@spawnforge/core';
import { Vector3 } from 'three';
import { z } from 'zod';
import { onGround, toeCentre, turned, Y } from './_feet.ts';

const params = z.strictObject({
  toeLength: z.number().min(0.02).max(0.5).default(0.12).describe('Toe length in torso lengths'),
  talonLength: z.number().min(0).max(0.3).default(0.05).describe('Talon length in torso lengths'),
  grip: z.number().min(0).max(1).default(0.3).describe('How curled the toes are at rest'),
  color: colorRef('#c8a040').describe('Scaly toe colour: a palette name or a colour'),
  talonColor: colorRef('#1e1a16').describe('Talon colour'),
});
type Params = z.output<typeof params>;

export default definePart({
  id: 'foot.talon',
  summary: 'A bird-like foot: three long toes forward, one back, each with a hooked talon.',
  tags: ['foot', 'bird', 'weapon'],
  slot: 'foot',
  material: 'horn',
  stance: 'digitigrade',
  attach: { on: 'limb' },
  params,
  example: { type: 'foot.talon', toeLength: 0.14, talonLength: 0.06 },
  describe: (_, { count }) => (count === 1 ? 'a taloned foot' : 'talons'),
  hooks: {
    footHeight: (ctx) => 1.3 * ctx.tipRadius,
    claws: (raw) => {
      const p = raw as Params;
      return { count: p.talonLength > 0 ? 4 : 0, length: p.talonLength };
    },
    toes(ctx, raw) {
      const p = raw as Params;
      const length = p.toeLength * ctx.scale;
      const rho = Math.min(0.32 * ctx.tipRadius, 0.11 * length);
      const chain = (dir: Vector3, reach: number) => {
        // Three joints along the ground; grip curls the tip down and back.
        const tipH = Math.max(0.25, 0.9 - 0.6 * p.grip) * rho;
        return {
          points: [
            ctx.ankle.clone().addScaledVector(dir, 0.15 * ctx.tipRadius),
            onGround(ctx, dir, 0.35 * reach, rho * 1.1),
            onGround(ctx, dir, 0.7 * reach, rho),
            onGround(ctx, dir, reach * (1 - 0.12 * p.grip), tipH),
          ],
          radii: [rho * 1.2, rho * 1.05, rho * 0.9, rho * 0.7],
        };
      };
      if (ctx.role !== 'leg') {
        // On an arm (a wing's hand, a harpy's), three long fingers continue it.
        const across = new Vector3().crossVectors(ctx.limbDir, ctx.forward);
        return [-20, 0, 20].map((deg) => {
          const dir = ctx.limbDir
            .clone()
            .applyAxisAngle(across.lengthSq() > 1e-8 ? ctx.forward : Y, (deg * Math.PI) / 180);
          const points = [0, 0.4, 0.75, 1].map((f) =>
            ctx.ankle.clone().addScaledVector(dir, f * length),
          );
          return { points, radii: [rho * 1.15, rho, rho * 0.85, rho * 0.65] };
        });
      }
      const centre = toeCentre(ctx);
      // The back toe points behind and a little inward, toward the other foot.
      const inward = ctx.outward.clone().negate();
      const back = centre.clone().negate().addScaledVector(inward, 0.3).normalize();
      return [
        chain(turned(centre, -28), length),
        chain(centre, length * 1.08),
        chain(turned(centre, 28), length),
        chain(back, length * 0.5),
      ];
    },
    build(ctx, raw) {
      const p = raw as Params;
      const scales = ctx.color(p.color, '#c8a040');
      const talon = ctx.color(p.talonColor, '#1e1a16');
      for (const toe of ctx.toes) {
        // A scaly sheath over each toe bone, ringed like a bird's.
        toe.bones.forEach((bone, k) => {
          const a = toe.points[k] as Vector3;
          const b = toe.points[k + 1] as Vector3;
          const along = new Vector3().subVectors(b, a);
          const length = along.length();
          if (length < 1e-6) return;
          const r0 = toe.toeRadius * (k === 0 ? 1.2 : 1.05);
          const r1 = toe.toeRadius * (k === toe.bones.length - 1 ? 0.8 : 1);
          const piece = ctx.geo.sweep(
            [new Vector3(), new Vector3(0, length, 0)],
            (t) => r0 + (r1 - r0) * t,
            {
              sides: 8,
              tip: 'flat',
              ridges: Math.max(2, Math.round(length / r0)),
              ridgeDepth: 0.1,
            },
          );
          ctx.emit(piece, ctx.frame(a, along, Y, bone, r0), { color: scales, bone });
        });
        if (p.talonLength <= 0) continue;
        // A hooked talon at the tip.
        const length = p.talonLength * ctx.scale;
        const base = Math.max(toe.toeRadius * 0.85, length * 0.13);
        const path = ctx.geo.arc(length, 110 + 40 * p.grip, { segments: 7, lean: 55 });
        const piece = ctx.geo.sweep(path, (t) => base * (1 - t * 0.93), { sides: 7, tip: 'point' });
        ctx.emit(piece, toe, {
          color: talon,
          tipColor: '#0c0a08',
          sink: base * 0.6,
          bone: toe.bone,
        });
      }
    },
  },
});
