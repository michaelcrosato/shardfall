import { defineTheme } from '@spawnforge/core';
import { z } from 'zod';

/** Wolves, bears, boars and big cats: muscled, furred, on paws, pads or hooves. */
export default defineTheme({
  id: 'beast',
  summary:
    'Muscled furred mammals: wolves and big cats on paws, boars and bulls on hooves, bears on pads, with ears, fangs, now and then horns or tusks, in tawny, brown, grey or black coats.',
  tags: ['fur', 'mammal'],
  params: z.strictObject({}),
  bias: {
    plans: {
      quadruped: {
        weight: 6,
        shape: {
          scale: { min: 0.7, max: 1.6 },
          body: {
            muscle: { min: 0.5, max: 0.9 },
            torso: {
              radius: [
                { min: 0.13, max: 0.17 },
                { min: 0.16, max: 0.22 },
                { min: 0.14, max: 0.19 },
                { min: 0.1, max: 0.14 },
              ],
              arch: { min: 0.03, max: 0.1 },
            },
            neck: {
              length: { min: 0.2, max: 0.35 },
              radius: [
                { min: 0.09, max: 0.12 },
                { min: 0.13, max: 0.18 },
              ],
              pitch: { min: 15, max: 30 },
            },
            head: {
              shape: 'snout',
              length: { min: 0.32, max: 0.45 },
              radius: { min: 0.1, max: 0.13 },
              jaw: true,
              pitch: { min: -15, max: -5 },
            },
            tail: {
              length: { min: 0.2, max: 0.8 },
              radius: [{ min: 0.05, max: 0.1 }, 0.02],
              pitch: { min: -35, max: -10 },
              curl: { min: 0, max: 20 },
            },
          },
          limbs: [
            {
              id: 'foreleg',
              length: { min: 0.55, max: 0.7 },
              radius: [{ min: 0.06, max: 0.08 }, 0.032],
              foot: { type: 'foot.paw', toes: 4, claws: 'short' },
            },
            {
              id: 'hindleg',
              length: { min: 0.57, max: 0.72 },
              radius: [{ min: 0.07, max: 0.09 }, 0.032],
              foot: { type: 'foot.paw', toes: 4, claws: 'short' },
            },
          ],
        },
      },
    },
    limbs: [
      // Hooves on every leg: boars, bulls and horses.
      {
        chance: 0.35,
        limbs: [
          { id: 'foreleg', foot: { type: 'foot.hoof', cloven: true } },
          { id: 'hindleg', foot: { type: 'foot.hoof', cloven: true } },
        ],
      },
    ],
    parts: [
      { chance: 1, part: { id: 'eyes', params: { scale: { min: 0.9, max: 1.2 } } } },
      {
        chance: 0.9,
        part: {
          id: 'ears',
          type: 'ear.pointed',
          attach: { on: 'head', at: 0.85, angle: 45 },
          params: {
            length: { min: 0.07, max: 0.13 },
            width: { min: 0.04, max: 0.07 },
            droop: { min: 0, max: 0.4 },
          },
        },
      },
      {
        chance: 0.9,
        part: {
          id: 'teeth',
          type: 'teeth.row',
          params: { fangs: 1, scale: { min: 0.9, max: 1.3 } },
        },
      },
      {
        chance: 0.25,
        part: {
          id: 'horns',
          type: 'horn.curved',
          attach: { on: 'head', at: 0.8, angle: { min: 50, max: 70 }, side: 'both' },
          params: {
            length: { min: 0.15, max: 0.35 },
            width: { min: 0.03, max: 0.05 },
            curve: { min: 40, max: 120 },
            aim: 'out',
            color: '#d8ccb0',
          },
        },
      },
      {
        chance: 0.2,
        part: {
          id: 'tusks',
          type: 'horn.curved',
          attach: { on: 'jaw', at: 0.25, angle: 60 },
          params: {
            length: { min: 0.12, max: 0.24 },
            width: 0.03,
            curve: { min: -70, max: -40 },
            color: '#efe6cc',
            tipColor: '#d8ccaa',
          },
        },
      },
      {
        chance: 0.2,
        part: {
          id: 'bristles',
          type: 'spikes.row',
          attach: { on: 'spine', from: 0.05, to: 0.6, angle: 0 },
          params: { count: { min: 10, max: 16 }, height: 0.04, width: 0.02, color: 'accent' },
        },
      },
    ],
    layers: {
      always: [{ type: 'countershade', strength: { min: 0.5, max: 0.8 } }],
      count: { min: 0, max: 2 },
      options: [
        {
          weight: 3,
          layer: {
            type: 'mottle',
            scale: { min: 0.06, max: 0.2 },
            coverage: { min: 0.3, max: 0.6 },
            contrast: 0.25,
          },
        },
        {
          weight: 2,
          layer: { type: 'stripes', color: 'accent', count: { min: 10, max: 18 }, width: 0.3 },
        },
        {
          weight: 2,
          layer: {
            type: 'rosettes',
            size: { min: 0.03, max: 0.06 },
            density: { min: 0.4, max: 0.7 },
          },
        },
        { weight: 1, layer: { type: 'spots', size: { min: 0.02, max: 0.04 } } },
        {
          weight: 1,
          layer: { type: 'scars', count: { min: 3, max: 7 }, region: 'torso' },
        },
      ],
    },
    palette: {
      base: {
        hue: { min: 15, max: 45 },
        saturation: { min: 0.1, max: 0.5 },
        lightness: { min: 0.2, max: 0.5 },
      },
    },
    skin: {
      fur: { length: { min: 0.015, max: 0.05 }, density: { min: 0.65, max: 0.95 } },
    },
    materials: { hide: 2, skin: 1 },
    temperaments: { lumbering: 2, stalking: 2, aggressive: 2, skittish: 1, calm: 1 },
    names: {
      start: ['gr', 'ur', 'bar', 'tor', 'ha', 'ka', 'vol', 'ruf', 'mor', 'bru'],
      end: ['ok', 'sa', 'gan', 'dur', 'rik', 'hun', 'fang', 'mar', 'lok', 'tusk'],
    },
  },
});
