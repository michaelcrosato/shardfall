import { defineTheme } from '@spawnforge/core';
import { z } from 'zod';

/** Fish, eels and the walkers that swim: fins, sleek bodies, countershaded blues and greys. */
export default defineTheme({
  id: 'aquatic',
  summary:
    'Sea and river creatures: sharks and fish with dorsal and tail fins, finned eels and sea serpents, and low armoured walkers that swim, in countershaded blues, teals and greys.',
  tags: ['fins', 'water'],
  params: z.strictObject({}),
  bias: {
    plans: {
      fish: {
        weight: 5,
        shape: {
          scale: { min: 0.6, max: 1.8 },
          body: {
            torso: {
              radius: [
                { min: 0.09, max: 0.13 },
                { min: 0.13, max: 0.18 },
                { min: 0.12, max: 0.17 },
                { min: 0.07, max: 0.11 },
              ],
            },
            head: { length: { min: 0.26, max: 0.34 }, radius: { min: 0.09, max: 0.12 } },
            tail: { length: { min: 0.6, max: 0.9 }, radius: [{ min: 0.07, max: 0.1 }, 0.025] },
          },
          limbs: [
            {
              id: 'pectoral',
              length: { min: 0.22, max: 0.4 },
              membrane: {
                type: 'membrane.fin',
                rays: { min: 5, max: 8 },
                width: { min: 0.5, max: 0.8 },
              },
            },
            { id: 'pelvic', length: { min: 0.12, max: 0.2 } },
          ],
        },
      },
      serpent: {
        weight: 2,
        shape: {
          scale: { min: 0.7, max: 1.5 },
          body: {
            torso: {
              radius: [
                { min: 0.08, max: 0.11 },
                { min: 0.1, max: 0.13 },
                { min: 0.1, max: 0.13 },
                { min: 0.09, max: 0.12 },
              ],
              crossSection: 'tall',
            },
            head: {
              shape: 'wedge',
              length: { min: 0.22, max: 0.3 },
              radius: { min: 0.08, max: 0.1 },
            },
            tail: {
              length: { min: 2, max: 3 },
              radius: [{ min: 0.1, max: 0.13 }, 0.02],
              crossSection: 'tall',
            },
          },
          limbs: [
            {
              id: 'pectoral',
              role: 'fin',
              attach: { on: 'torso', at: 0.12, side: 'both', angle: 115 },
              length: { min: 0.14, max: 0.24 },
              membrane: { type: 'membrane.fin', rays: { min: 4, max: 6 } },
            },
          ],
        },
      },
      quadruped: {
        weight: 2,
        shape: {
          scale: { min: 0.9, max: 1.8 },
          body: {
            torso: {
              radius: [
                { min: 0.11, max: 0.14 },
                { min: 0.15, max: 0.19 },
                { min: 0.14, max: 0.18 },
                { min: 0.1, max: 0.13 },
              ],
              crossSection: 'wide',
            },
            neck: { length: { min: 0.12, max: 0.2 }, pitch: { min: 5, max: 15 } },
            head: {
              shape: 'snout',
              length: { min: 0.36, max: 0.5 },
              radius: { min: 0.08, max: 0.1 },
              pitch: -5,
              crossSection: 'wide',
            },
            tail: {
              length: { min: 1.1, max: 1.5 },
              radius: [{ min: 0.1, max: 0.13 }, 0.015],
              pitch: -5,
              crossSection: 'tall',
              segments: 10,
            },
          },
          limbs: [
            {
              id: 'foreleg',
              length: { min: 0.3, max: 0.38 },
              radius: [0.06, 0.035],
              splay: { min: 30, max: 45 },
              foot: { type: 'foot.claw', toes: 5, clawLength: 0.03 },
            },
            {
              id: 'hindleg',
              length: { min: 0.34, max: 0.42 },
              radius: [0.07, 0.04],
              splay: { min: 30, max: 45 },
              foot: { type: 'foot.claw', toes: 5, clawLength: 0.03 },
            },
          ],
          // A walker that swims: crocodiles and marine iguanas.
          motion: { media: { water: true } },
        },
      },
    },
    parts: [
      {
        chance: 1,
        part: { id: 'eyes', params: { scale: { min: 0.8, max: 1.4 }, irisColor: '#101010' } },
      },
      {
        chance: 0.75,
        part: {
          id: 'teeth',
          type: 'teeth.row',
          params: { count: { min: 10, max: 18 }, scale: { min: 0.8, max: 1.3 } },
        },
      },
      {
        chance: 1,
        part: {
          id: 'dorsal',
          params: {
            height: { min: 0.12, max: 0.3 },
            length: { min: 0.15, max: 0.3 },
            sweep: { min: 15, max: 45 },
          },
        },
        plans: ['fish'],
      },
      {
        chance: 0.5,
        part: { id: 'tailfin', params: { shape: 'forked', size: { min: 0.3, max: 0.45 } } },
        plans: ['fish'],
      },
      {
        chance: 0.8,
        part: {
          id: 'dorsal',
          type: 'fin.dorsal',
          attach: { on: 'torso', at: 0.5, angle: 0 },
          params: {
            height: { min: 0.08, max: 0.16 },
            length: { min: 0.5, max: 0.9 },
            shape: 'sail',
          },
        },
        plans: ['serpent'],
      },
      {
        chance: 0.7,
        part: { id: 'tailfin', type: 'fin.tail', params: { shape: 'rounded', size: 0.25 } },
        plans: ['serpent'],
      },
      {
        chance: 0.7,
        part: {
          id: 'scutes',
          type: 'spikes.row',
          attach: { on: 'spine', from: 0.35, to: 0.95, angle: 0 },
          params: {
            count: { min: 12, max: 22 },
            height: { min: 0.02, max: 0.035 },
            width: 0.03,
            curve: 0,
          },
        },
        plans: ['quadruped'],
      },
    ],
    layers: {
      always: [{ type: 'countershade', height: { min: 0.05, max: 0.3 }, softness: 0.2 }],
      count: { min: 0, max: 2 },
      options: [
        {
          weight: 2,
          layer: {
            type: 'spots',
            size: { min: 0.02, max: 0.05 },
            density: { min: 0.2, max: 0.5 },
          },
        },
        {
          weight: 2,
          layer: { type: 'stripes', region: 'back', count: { min: 6, max: 14 }, width: 0.3 },
        },
        { weight: 1, layer: { type: 'mottle', scale: { min: 0.1, max: 0.25 } } },
        {
          weight: 1,
          layer: {
            type: 'bioluminescence',
            size: { min: 0.01, max: 0.025 },
            density: { min: 0.2, max: 0.5 },
          },
        },
      ],
    },
    palette: {
      base: {
        hue: { min: 150, max: 240 },
        saturation: { min: 0.15, max: 0.5 },
        lightness: { min: 0.25, max: 0.45 },
      },
    },
    materials: { skin: 2, scales: 2 },
    temperaments: { calm: 2, skittish: 2, stalking: 2, aggressive: 1 },
    names: {
      start: ['mar', 'thal', 'cor', 'nau', 'pel', 'syl', 'kai', 'bry', 'ondi', 'scy'],
      end: ['lis', 'fin', 'ra', 'gul', 'mora', 'rin', 'sha', 'tide', 'wyn', 'qua'],
    },
  },
});
