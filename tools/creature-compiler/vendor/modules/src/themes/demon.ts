import { defineTheme } from '@spawnforge/core';
import { z } from 'zod';

/** Fiends and hellhounds: horns, fangs, spines, reds and blacks. */
export default defineTheme({
  id: 'demon',
  summary:
    'Horned fiends and hellhounds: heavy shoulders, big curled or swept horns, fangs, spine rows, goat or slit pupils, blood reds and soot blacks.',
  tags: ['horns', 'fangs'],
  params: z.strictObject({}),
  bias: {
    plans: {
      biped: {
        weight: 3,
        shape: {
          scale: { min: 0.7, max: 1 },
          body: {
            torso: {
              radius: [
                { min: 0.14, max: 0.17 },
                { min: 0.23, max: 0.28 },
                { min: 0.19, max: 0.23 },
                { min: 0.15, max: 0.17 },
              ],
              crossSection: 'wide',
            },
            neck: { length: { min: 0.2, max: 0.26 }, radius: [0.09, 0.12] },
            head: {
              shape: 'wedge',
              length: { min: 0.3, max: 0.36 },
              radius: { min: 0.13, max: 0.16 },
            },
            tail: { length: { min: 0.4, max: 0.8 }, pitch: -35, radius: [0.07, 0.012] },
          },
          limbs: [
            {
              id: 'leg',
              length: { min: 1.2, max: 1.4 },
              radius: [0.12, 0.07],
              segments: 3,
              splay: 4,
              attach: { angle: 105 },
              foot: { type: 'foot.claw', toes: 3, clawLength: 0.05 },
            },
            { id: 'arm', length: { min: 1, max: 1.2 }, radius: [0.09, 0.05] },
          ],
        },
      },
      quadruped: {
        weight: 2,
        shape: {
          scale: { min: 0.9, max: 1.4 },
          body: {
            torso: {
              radius: [
                { min: 0.13, max: 0.16 },
                { min: 0.18, max: 0.22 },
                { min: 0.14, max: 0.17 },
                { min: 0.1, max: 0.12 },
              ],
              arch: { min: 0.06, max: 0.14 },
            },
            neck: { length: { min: 0.3, max: 0.36 }, pitch: 25, radius: [0.08, 0.12] },
            head: { shape: 'snout', length: { min: 0.34, max: 0.4 }, radius: 0.115, pitch: -15 },
            tail: {
              length: { min: 0.5, max: 0.8 },
              radius: [0.07, 0.01],
              curl: { min: 10, max: 30 },
            },
          },
          limbs: [
            { id: 'foreleg', length: { min: 0.58, max: 0.66 }, radius: [0.055, 0.03] },
            { id: 'hindleg', length: { min: 0.62, max: 0.7 }, radius: [0.07, 0.03] },
          ],
        },
      },
    },
    parts: [
      {
        chance: 1,
        part: {
          id: 'eyes',
          params: { scale: { min: 1.1, max: 1.5 }, pupil: 'goat', irisColor: '#ffcc00' },
        },
      },
      {
        chance: 1,
        part: {
          id: 'teeth',
          type: 'teeth.row',
          params: { fangs: 2, fangScale: { min: 1, max: 1.5 }, spacing: { min: 0, max: 0.3 } },
        },
      },
      {
        chance: 0.8,
        part: {
          id: 'horns',
          type: 'horn.curved',
          attach: { on: 'head', at: 0.6, angle: { min: 60, max: 85 }, side: 'both' },
          params: {
            length: { min: 0.35, max: 0.75 },
            width: { min: 0.045, max: 0.065 },
            curve: { min: 60, max: 400 },
            turn: { min: -65, max: 0 },
            ridges: { min: 0, max: 12 },
            color: '#2a1a14',
            tipColor: '#d8c8a0',
          },
        },
      },
      {
        chance: 0.5,
        part: {
          id: 'spines',
          type: 'spikes.row',
          attach: { on: 'spine', from: 0.18, to: 0.82, angle: 0 },
          params: {
            count: { min: 10, max: 16 },
            height: [0.08, { min: 0.14, max: 0.2 }, 0.12, 0.05],
            width: 0.022,
            curve: { min: 20, max: 45 },
            jitter: { min: 0.3, max: 1 },
            color: '#3a2a26',
            tipColor: '#a82020',
          },
        },
        plans: ['quadruped'],
      },
      {
        chance: 0.4,
        part: {
          id: 'ears',
          type: 'ear.pointed',
          attach: { on: 'head', at: 0.85, angle: 45 },
          params: { length: { min: 0.1, max: 0.16 }, width: 0.05 },
        },
      },
    ],
    layers: {
      always: [{ type: 'countershade', strength: 0.5 }],
      count: { min: 1, max: 2 },
      options: [
        { weight: 2, layer: { type: 'stripes', count: { min: 10, max: 16 }, width: 0.3 } },
        { weight: 2, layer: { type: 'mottle', scale: { min: 0.1, max: 0.2 }, strength: 0.6 } },
        { weight: 2, layer: { type: 'grime', amount: { min: 0.5, max: 1 } } },
      ],
    },
    palette: {
      base: {
        hue: { min: -15, max: 20 },
        saturation: { min: 0.35, max: 0.75 },
        lightness: { min: 0.08, max: 0.38 },
      },
      accent: {
        hue: { min: 0, max: 20 },
        saturation: { min: 0, max: 0.3 },
        lightness: { min: 0.03, max: 0.08 },
      },
    },
    materials: { skin: 3, scales: 1 },
    temperaments: { aggressive: 4, stalking: 2 },
    names: {
      start: ['bal', 'mor', 'azh', 'gor', 'vex', 'ur', 'zul', 'kha', 'mal', 'nyx'],
      end: ['goth', 'zul', 'rak', 'moth', 'thul', 'gash', 'ror', 'baal', 'vor', 'xis'],
    },
  },
});
