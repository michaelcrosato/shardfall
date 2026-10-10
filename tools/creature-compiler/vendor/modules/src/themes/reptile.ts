import { defineTheme } from '@spawnforge/core';
import { z } from 'zod';

/** Lizards, raptors and snakes: scaled skin, long tails, earthy greens and browns. */
export default defineTheme({
  id: 'reptile',
  summary:
    'Scaled lizards, raptors and snakes: low sprawling bodies, long tails, wedge or snout heads, slit pupils, greens and browns.',
  tags: ['scales', 'tail'],
  params: z.strictObject({}),
  bias: {
    plans: {
      quadruped: {
        weight: 5,
        shape: {
          scale: { min: 0.7, max: 1.6 },
          body: {
            torso: {
              radius: [
                { min: 0.12, max: 0.16 },
                { min: 0.15, max: 0.22 },
                { min: 0.14, max: 0.2 },
                { min: 0.11, max: 0.15 },
              ],
              arch: { min: 0, max: 0.06 },
              crossSection: 'wide',
            },
            neck: { length: { min: 0.18, max: 0.32 }, pitch: { min: 8, max: 25 } },
            head: { shape: 'wedge', length: { min: 0.26, max: 0.36 }, pitch: -5 },
            tail: {
              length: { min: 0.9, max: 1.6 },
              radius: [{ min: 0.09, max: 0.13 }, 0.012],
              pitch: -8,
              curl: { min: 0, max: 12 },
              segments: 10,
            },
          },
          limbs: [
            {
              id: 'foreleg',
              length: { min: 0.38, max: 0.5 },
              radius: [{ min: 0.06, max: 0.09 }, 0.04],
              splay: { min: 20, max: 40 },
              foot: { type: 'foot.claw', toes: 5, clawLength: { min: 0.03, max: 0.05 } },
            },
            {
              id: 'hindleg',
              length: { min: 0.42, max: 0.55 },
              radius: [{ min: 0.07, max: 0.1 }, 0.045],
              splay: { min: 20, max: 40 },
              foot: { type: 'foot.claw', toes: 5, clawLength: { min: 0.03, max: 0.05 } },
            },
          ],
        },
      },
      biped: {
        weight: 2,
        shape: {
          scale: { min: 0.7, max: 1.2 },
          body: {
            torso: {
              radius: [
                { min: 0.1, max: 0.13 },
                { min: 0.16, max: 0.2 },
                { min: 0.15, max: 0.19 },
                { min: 0.1, max: 0.12 },
              ],
              pitch: { min: 6, max: 18 },
              crossSection: 'round',
              segments: 6,
            },
            neck: {
              length: { min: 0.25, max: 0.35 },
              radius: [0.06, 0.09],
              pitch: { min: 35, max: 55 },
            },
            head: { shape: 'snout', length: { min: 0.28, max: 0.36 }, radius: 0.095, pitch: -5 },
            tail: {
              length: { min: 1.3, max: 1.8 },
              radius: [{ min: 0.09, max: 0.11 }, 0.012],
              pitch: 0,
              curl: 0,
              segments: 10,
            },
          },
          limbs: [
            {
              id: 'leg',
              attach: { at: 0.6, angle: 104 },
              length: { min: 1.05, max: 1.25 },
              segments: 3,
              radius: [0.115, 0.055, 0.03],
              splay: 8,
              foot: { type: 'foot.claw', toes: 3, toeLength: 0.12, clawLength: 0.05, spread: 40 },
            },
            {
              id: 'arm',
              attach: { at: 0.1, angle: 100 },
              length: { min: 0.32, max: 0.45 },
              radius: [0.04, 0.022],
              lift: 45,
              foot: { type: 'foot.claw', toes: 3, toeLength: 0.05, clawLength: 0.04 },
            },
          ],
        },
      },
      serpent: {
        weight: 3,
        shape: {
          scale: { min: 0.5, max: 1.1 },
          body: {
            torso: {
              radius: [
                { min: 0.09, max: 0.12 },
                { min: 0.11, max: 0.15 },
                { min: 0.12, max: 0.16 },
                { min: 0.11, max: 0.14 },
              ],
            },
            neck: {
              radius: [
                { min: 0.08, max: 0.1 },
                { min: 0.085, max: 0.11 },
              ],
            },
            head: {
              shape: 'wedge',
              length: { min: 0.2, max: 0.28 },
              radius: { min: 0.09, max: 0.11 },
            },
            tail: { length: { min: 1.8, max: 2.8 }, radius: [{ min: 0.11, max: 0.14 }, 0.012] },
          },
        },
      },
    },
    parts: [
      {
        chance: 1,
        part: { id: 'eyes', params: { pupil: 'slit', scale: { min: 0.8, max: 1.3 } } },
      },
      {
        chance: 0.8,
        part: {
          id: 'teeth',
          type: 'teeth.row',
          params: { fangs: 0, scale: { min: 0.8, max: 1.3 }, spacing: { min: 0.1, max: 0.6 } },
        },
        plans: ['quadruped', 'biped'],
      },
      {
        chance: 0.7,
        part: {
          id: 'fangs',
          type: 'teeth.row',
          params: { incisors: 0, fangs: 1, fangScale: { min: 0.9, max: 1.4 }, lower: false },
        },
        plans: ['serpent'],
      },
      {
        chance: 0.45,
        part: {
          id: 'dorsal',
          type: 'spikes.row',
          attach: { on: 'spine', from: 0.1, to: 0.9, angle: 0 },
          params: {
            count: { min: 8, max: 16 },
            height: [
              { min: 0.05, max: 0.1 },
              { min: 0.1, max: 0.2 },
              { min: 0.06, max: 0.12 },
            ],
            width: { min: 0.025, max: 0.045 },
            curve: { min: 10, max: 35 },
            jitter: { min: 0, max: 0.3 },
          },
        },
        plans: ['quadruped', 'biped'],
      },
      {
        chance: 0.25,
        part: {
          id: 'brow',
          type: 'horn.curved',
          attach: { on: 'head', at: 0.7, angle: 40 },
          params: { length: { min: 0.06, max: 0.12 }, width: 0.035, curve: { min: 20, max: 50 } },
        },
        plans: ['quadruped'],
      },
    ],
    layers: {
      always: [{ type: 'countershade' }, { type: 'scales', size: { min: 0.02, max: 0.05 } }],
      count: { min: 0, max: 2 },
      options: [
        {
          weight: 3,
          layer: {
            type: 'stripes',
            region: 'back',
            count: { min: 8, max: 18 },
            width: { min: 0.25, max: 0.4 },
          },
        },
        {
          weight: 2,
          layer: { type: 'spots', size: { min: 0.03, max: 0.07 }, density: { min: 0.3, max: 0.6 } },
        },
        { weight: 2, layer: { type: 'mottle', scale: { min: 0.1, max: 0.25 } } },
        { weight: 1, layer: { type: 'grime', amount: { min: 0.2, max: 0.5 } } },
      ],
    },
    palette: {
      base: {
        hue: { min: 40, max: 130 },
        saturation: { min: 0.2, max: 0.55 },
        lightness: { min: 0.22, max: 0.42 },
      },
    },
    materials: { scales: 1 },
    temperaments: { stalking: 3, calm: 1, aggressive: 2 },
    names: {
      start: ['sca', 'vel', 'ser', 'kra', 'ith', 'zar', 'sil', 've', 'ash', 'tor'],
      end: ['lith', 'krax', 'ssen', 'drak', 'vex', 'rath', 'zis', 'thar', 'nok', 'skel'],
    },
  },
});
