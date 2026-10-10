import { defineTheme } from '@spawnforge/core';
import { z } from 'zod';

/** Things that should not be: tentacles, too many eyes, more than one head. */
export default defineTheme({
  id: 'eldritch',
  summary:
    'Things that should not be: tentacled krakens, many-headed hounds, faces of tentacles and clusters of eyes, slimy veined skin with a cold glow, in bruise purples and deep blacks.',
  tags: ['tentacles', 'eyes', 'heads'],
  params: z.strictObject({}),
  bias: {
    plans: {
      serpent: {
        weight: 3,
        shape: {
          scale: { min: 0.9, max: 1.6 },
          body: {
            torso: {
              radius: [
                { min: 0.18, max: 0.22 },
                { min: 0.26, max: 0.32 },
                { min: 0.28, max: 0.34 },
                { min: 0.22, max: 0.28 },
                { min: 0.1, max: 0.14 },
              ],
              pitch: 0,
            },
            neck: { length: 0.05, radius: [0.17, 0.19], pitch: 0, count: { min: 1, max: 2 } },
            head: {
              shape: 'round',
              length: { min: 0.28, max: 0.34 },
              radius: { min: 0.17, max: 0.22 },
              jaw: true,
              lips: 0,
              tongue: 'none',
            },
            tail: { length: 0 },
          },
          limbs: [
            {
              id: 'arm1',
              role: 'tentacle',
              attach: { on: 'head', at: 0.35, side: 'both', angle: { min: 20, max: 35 } },
              length: { min: 1.3, max: 2 },
              segments: 12,
              radius: [0.07, 0.03, 0.008],
              curl: { min: 100, max: 180 },
              curlStart: 0.3,
            },
            {
              id: 'arm2',
              role: 'tentacle',
              attach: { on: 'head', at: 0.35, side: 'both', angle: { min: 80, max: 110 } },
              length: { min: 1.3, max: 2 },
              segments: 12,
              radius: [0.07, 0.03, 0.008],
              curl: { min: 120, max: 200 },
              curlStart: 0.3,
            },
          ],
        },
      },
      quadruped: {
        weight: 3,
        shape: {
          scale: { min: 0.9, max: 1.5 },
          body: {
            muscle: { min: 0.3, max: 0.6 },
            torso: {
              radius: [
                { min: 0.15, max: 0.19 },
                { min: 0.2, max: 0.25 },
                { min: 0.18, max: 0.23 },
                { min: 0.13, max: 0.16 },
              ],
              arch: { min: 0.04, max: 0.12 },
            },
            neck: {
              count: { min: 2, max: 3 },
              spread: { min: 55, max: 80 },
              length: { min: 0.4, max: 0.6 },
              radius: [0.06, 0.1],
              pitch: { min: 30, max: 45 },
              curve: { min: 0, max: 25 },
            },
            head: {
              shape: 'snout',
              length: { min: 0.28, max: 0.34 },
              radius: { min: 0.08, max: 0.1 },
              jaw: true,
              pitch: -10,
            },
            tail: { length: { min: 0.5, max: 1 }, radius: [0.08, 0.012], pitch: -20 },
          },
          limbs: [
            { id: 'foreleg', length: { min: 0.5, max: 0.6 }, foot: { type: 'foot.claw', toes: 4 } },
            {
              id: 'hindleg',
              length: { min: 0.52, max: 0.62 },
              foot: { type: 'foot.claw', toes: 4 },
            },
          ],
        },
      },
      octopod: {
        weight: 2,
        shape: { scale: { min: 0.6, max: 1.2 } },
      },
    },
    limbs: [
      // A face of tentacles hanging from the jaw.
      {
        chance: 0.6,
        limbs: [
          {
            id: 'feelers',
            role: 'tentacle',
            attach: { on: 'head', at: 0.15, side: 'both', angle: { min: 120, max: 150 } },
            length: { min: 0.35, max: 0.6 },
            segments: 8,
            radius: [0.03, 0.006],
            curl: { min: 60, max: 140 },
          },
        ],
        plans: ['quadruped', 'octopod'],
      },
      // A third pair of arms for the kraken.
      {
        chance: 0.5,
        limbs: [
          {
            id: 'arm3',
            role: 'tentacle',
            attach: { on: 'head', at: 0.35, side: 'both', angle: { min: 140, max: 160 } },
            length: { min: 1.4, max: 2.2 },
            segments: 12,
            radius: [0.06, 0.025, 0.008],
            curl: { min: 150, max: 220 },
            curlStart: 0.3,
          },
        ],
        plans: ['serpent'],
      },
    ],
    parts: [
      {
        chance: 1,
        part: {
          id: 'eyes',
          params: {
            scale: { min: 1.2, max: 2 },
            pupil: 'slit',
            irisColor: { min: '#c0d020', max: '#f0e080' },
            lids: false,
          },
        },
      },
      {
        chance: 0.7,
        part: {
          id: 'eyes2',
          type: 'eye.basic',
          attach: { on: 'head', at: 0.72, angle: { min: 25, max: 35 }, side: 'both' },
          params: { scale: { min: 0.5, max: 0.8 }, pupil: 'slit', lids: false },
        },
      },
      {
        chance: 0.35,
        part: {
          id: 'eyes3',
          type: 'eye.basic',
          attach: { on: 'head', at: 0.5, angle: { min: 55, max: 70 }, side: 'both' },
          params: { scale: { min: 0.4, max: 0.6 }, pupil: 'round', lids: false },
        },
      },
      {
        chance: 0.8,
        part: {
          id: 'teeth',
          type: 'teeth.row',
          params: { fangs: { min: 1, max: 2 }, count: { min: 12, max: 22 }, spacing: 0.2 },
        },
        plans: ['quadruped', 'serpent'],
      },
      {
        chance: 0.3,
        part: {
          id: 'spines',
          type: 'spikes.row',
          attach: { on: 'spine', from: 0.3, to: 0.9, angle: 0 },
          params: { count: { min: 8, max: 14 }, height: [0.03, 0.08, 0.03], jitter: 0.5 },
        },
        plans: ['quadruped'],
      },
    ],
    layers: {
      always: [{ type: 'countershade', softness: 0.4, strength: 0.6 }],
      count: { min: 1, max: 2 },
      options: [
        {
          weight: 2,
          layer: { type: 'veins', color: 'accent', density: { min: 0.3, max: 0.7 }, raised: 0.5 },
        },
        { weight: 2, layer: { type: 'slime', wetness: { min: 0.4, max: 0.8 } } },
        {
          weight: 2,
          layer: {
            type: 'bioluminescence',
            color: { min: '#40d0c0', max: '#a0f060' },
            size: { min: 0.01, max: 0.03 },
          },
        },
        { weight: 1, layer: { type: 'warts', size: { min: 0.01, max: 0.03 } } },
        { weight: 1, layer: { type: 'mottle', color: 'accent', scale: { min: 0.08, max: 0.18 } } },
      ],
    },
    palette: {
      base: {
        hue: { min: 250, max: 340 },
        saturation: { min: 0.12, max: 0.45 },
        lightness: { min: 0.14, max: 0.32 },
      },
    },
    materials: { skin: 3, hide: 1 },
    temperaments: { stalking: 3, skittish: 1, aggressive: 1, calm: 1 },
    names: {
      start: ['cth', 'yog', 'nyar', 'az', 'sho', 'dag', 'ith', 'zhul', 'gla', 'xo'],
      end: ['ulhu', 'goth', 'lath', 'oth', 'gaal', 'thuu', 'qua', 'mog', 'ruk', 'zath'],
    },
  },
});
