import { defineTheme } from '@spawnforge/core';
import { z } from 'zod';

/** Winged dragons and wyverns: long necks and tails, wings, horns, crests and scales. */
export default defineTheme({
  id: 'dragon',
  summary:
    'Winged dragons and wyverns: four legs and a pair of wings or wings for arms, long curved necks and tails, horns, crests, fangs, scales, in ember, emerald, gold or soot.',
  tags: ['wings', 'scales', 'horns'],
  params: z.strictObject({}),
  bias: {
    plans: {
      quadruped: {
        weight: 3,
        shape: {
          scale: { min: 1.1, max: 2 },
          body: {
            torso: {
              radius: [
                { min: 0.12, max: 0.15 },
                { min: 0.16, max: 0.2 },
                { min: 0.15, max: 0.18 },
                { min: 0.1, max: 0.13 },
              ],
              arch: { min: 0.04, max: 0.1 },
            },
            neck: {
              length: { min: 0.45, max: 0.7 },
              radius: [0.06, 0.09],
              pitch: { min: 25, max: 40 },
              curve: { min: 10, max: 30 },
              segments: 5,
            },
            head: {
              shape: 'wedge',
              length: { min: 0.26, max: 0.34 },
              radius: { min: 0.075, max: 0.095 },
              jaw: true,
              brow: { min: 0.3, max: 0.6 },
            },
            tail: {
              length: { min: 1.2, max: 1.8 },
              radius: [{ min: 0.09, max: 0.11 }, 0.012],
              curl: { min: 0, max: 20 },
              segments: 12,
            },
          },
          limbs: [
            {
              id: 'foreleg',
              length: { min: 0.45, max: 0.55 },
              radius: [0.06, 0.03],
              foot: { type: 'foot.claw', toes: 4 },
            },
            {
              id: 'hindleg',
              length: { min: 0.5, max: 0.6 },
              radius: [0.075, 0.035],
              foot: { type: 'foot.claw', toes: 4 },
            },
            {
              id: 'wing',
              role: 'wing',
              attach: { on: 'torso', at: { min: 0.12, max: 0.2 }, side: 'both', angle: 35 },
              length: { min: 1.3, max: 1.8 },
              segments: 3,
              radius: [0.045, 0.014],
              membrane: {
                type: 'membrane.bat',
                fingers: { min: 3, max: 5 },
                span: { min: 0.95, max: 1.2 },
                trailing: 'body',
                scallop: { min: 0.3, max: 0.7 },
                translucency: { min: 0.3, max: 0.6 },
              },
            },
          ],
        },
      },
      wyvern: {
        weight: 2,
        shape: {
          scale: { min: 1, max: 1.6 },
          body: {
            neck: { length: { min: 0.45, max: 0.6 }, curve: { min: 5, max: 25 } },
            head: { length: { min: 0.3, max: 0.36 }, brow: { min: 0.3, max: 0.6 } },
            tail: { length: { min: 1.4, max: 1.9 } },
          },
          limbs: [
            {
              id: 'wing',
              length: { min: 1.4, max: 1.8 },
              membrane: {
                type: 'membrane.bat',
                fingers: { min: 3, max: 5 },
                span: { min: 1, max: 1.2 },
                trailing: 'body',
                scallop: { min: 0.3, max: 0.7 },
                translucency: { min: 0.3, max: 0.6 },
              },
            },
          ],
        },
      },
    },
    limbs: [
      // Now and then a feathered dragon, its wings a bird's.
      {
        chance: 0.2,
        limbs: [{ id: 'wing', membrane: { type: 'membrane.feather', length: 0.4 } }],
      },
    ],
    parts: [
      {
        chance: 1,
        part: {
          id: 'eyes',
          params: {
            pupil: 'slit',
            squint: { min: 0.2, max: 0.5 },
            irisColor: { min: '#d06010', max: '#f0d040' },
          },
        },
      },
      {
        chance: 1,
        part: { id: 'teeth', type: 'teeth.row', params: { fangs: { min: 1, max: 2 } } },
      },
      {
        chance: 0.85,
        part: {
          id: 'horns',
          type: 'horn.curved',
          attach: { on: 'head', at: 0.85, angle: { min: 25, max: 40 }, side: 'both' },
          params: {
            length: { min: 0.2, max: 0.4 },
            curve: { min: 20, max: 70 },
            ridges: { min: 0, max: 8 },
            color: { min: '#1e1a18', max: '#d8d0bc' },
          },
        },
      },
      {
        chance: 0.6,
        part: {
          id: 'crest',
          type: 'spikes.row',
          attach: { on: 'spine', from: 0.3, to: 0.95, angle: 0 },
          params: {
            count: { min: 6, max: 14 },
            height: [
              { min: 0.04, max: 0.07 },
              { min: 0.07, max: 0.12 },
              { min: 0.03, max: 0.06 },
            ],
            curve: { min: 15, max: 40 },
          },
        },
      },
      {
        chance: 0.35,
        part: {
          id: 'tailspikes',
          type: 'spikes.row',
          attach: { on: 'tail', from: 0.7, to: 1, angle: 0 },
          params: { count: { min: 3, max: 6 }, height: [0.05, 0.09], curve: 10 },
        },
      },
      {
        chance: 0.12,
        part: {
          id: 'frill',
          type: 'frill',
          params: { radius: { min: 0.2, max: 0.32 }, spines: { min: 8, max: 14 } },
        },
        plans: ['quadruped'],
      },
    ],
    layers: {
      always: [
        { type: 'countershade', strength: { min: 0.6, max: 0.9 } },
        { type: 'scales', size: { min: 0.015, max: 0.03 }, bump: { min: 0.3, max: 0.6 } },
      ],
      count: { min: 0, max: 2 },
      options: [
        {
          weight: 2,
          layer: {
            type: 'veins',
            color: 'accent',
            region: 'wings',
            strength: { min: 0.4, max: 0.7 },
          },
        },
        {
          weight: 2,
          layer: {
            type: 'stripes',
            region: 'back',
            count: { min: 8, max: 14 },
            jitter: { min: 0.2, max: 0.6 },
          },
        },
        {
          weight: 1,
          layer: { type: 'bands', color: 'accent', region: 'wings', count: { min: 2, max: 4 } },
        },
        { weight: 1, layer: { type: 'mottle', scale: { min: 0.1, max: 0.2 } } },
      ],
    },
    palette: {
      base: {
        hue: { min: 0, max: 360 },
        saturation: { min: 0.15, max: 0.55 },
        lightness: { min: 0.16, max: 0.36 },
      },
    },
    materials: { scales: 1 },
    temperaments: { aggressive: 3, stalking: 2, calm: 1 },
    names: {
      start: ['dra', 'vor', 'ign', 'sca', 'ky', 'tha', 'pyr', 'bal', 'zor', 'aur'],
      end: ['gon', 'rax', 'thar', 'vyr', 'drix', 'mor', 'ith', 'gorn', 'leth', 'zar'],
    },
  },
});
