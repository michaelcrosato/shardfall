import { defineTheme } from '@spawnforge/core';
import { z } from 'zod';

/** Beetles and ants: six legs, chitin, wasp waists, horns and mandibles. */
export default defineTheme({
  id: 'insect',
  summary:
    'Six-legged beetles and ants: glossy chitin, a thorax and abdomen with a narrow waist, thin legs, horns or mandibles, dark or warning colours.',
  tags: ['chitin', 'six-legged'],
  params: z.strictObject({}),
  bias: {
    plans: { hexapod: { weight: 1 } },
    shape: {
      scale: { min: 0.35, max: 0.9 },
      body: {
        torso: {
          radius: [
            { min: 0.09, max: 0.12 },
            { min: 0.12, max: 0.16 },
            { min: 0.06, max: 0.09 },
            { min: 0.15, max: 0.21 },
            { min: 0.13, max: 0.18 },
            { min: 0.07, max: 0.1 },
          ],
        },
        head: { length: { min: 0.18, max: 0.3 }, radius: { min: 0.08, max: 0.15 } },
      },
      limbs: [
        { id: 'frontleg', length: { min: 0.55, max: 0.8 } },
        { id: 'midleg', length: { min: 0.6, max: 0.85 } },
        { id: 'hindleg', length: { min: 0.65, max: 0.95 } },
      ],
    },
    parts: [
      { chance: 1, part: { id: 'eyes', params: { scale: { min: 1.4, max: 2 }, lids: false } } },
      {
        chance: 0.4,
        part: {
          id: 'horn',
          type: 'horn.curved',
          attach: { on: 'head', at: 0.2, angle: 0, side: 'center' },
          params: {
            length: { min: 0.15, max: 0.35 },
            width: { min: 0.03, max: 0.05 },
            curve: { min: -80, max: -40 },
            ridges: { min: 0, max: 8 },
          },
        },
      },
      {
        chance: 0.35,
        part: {
          id: 'mandibles',
          type: 'mandible',
          params: { length: { min: 0.12, max: 0.25 }, teeth: { min: 0, max: 4 } },
        },
      },
    ],
    layers: {
      always: [{ type: 'countershade', softness: 0.2 }],
      count: { min: 0, max: 1 },
      options: [
        {
          weight: 2,
          layer: {
            type: 'spots',
            region: 'back',
            size: { min: 0.03, max: 0.06 },
            density: { min: 0.3, max: 0.5 },
          },
        },
        {
          weight: 1,
          layer: { type: 'stripes', region: 'back', count: { min: 4, max: 9 }, width: 0.3 },
        },
      ],
    },
    palette: {
      base: {
        hue: { min: 0, max: 360 },
        saturation: { min: 0.2, max: 0.75 },
        lightness: { min: 0.1, max: 0.32 },
      },
    },
    materials: { chitin: 1 },
    temperaments: { skittish: 3, aggressive: 2, calm: 1 },
    names: {
      start: ['kit', 'chi', 'zz', 'skri', 'tik', 'cha', 'myr', 'scar', 'klik', 'vesp'],
      end: ['tin', 'mex', 'rix', 'thrax', 'zid', 'kis', 'pod', 'mandi', 'ax', 'crik'],
    },
  },
});
