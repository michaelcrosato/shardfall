import { defineBodyPlan } from '@spawnforge/core';

export default defineBodyPlan({
  id: 'fish',
  summary:
    'A legless, deep-bodied swimmer with pectoral and pelvic fins, a dorsal fin and a tail fin; lives in water.',
  tags: ['legs:0', 'fins:4', 'aquatic'],
  preset: {
    scale: 0.8,
    body: {
      torso: { radius: [0.12, 0.17, 0.16, 0.11], pitch: 0, segments: 6, crossSection: 'tall' },
      neck: { length: 0 },
      head: {
        shape: 'wedge',
        length: 0.3,
        radius: 0.12,
        jaw: true,
        pitch: 0,
        crossSection: 'tall',
      },
      tail: { length: 0.8, radius: [0.1, 0.03], pitch: 0, segments: 8, crossSection: 'tall' },
    },
    limbs: [
      {
        id: 'pectoral',
        role: 'fin',
        attach: { on: 'torso', at: 0.2, side: 'both', angle: 120 },
        length: 0.3,
        membrane: { type: 'membrane.fin', rays: 7 },
      },
      {
        id: 'pelvic',
        role: 'fin',
        attach: { on: 'torso', at: 0.55, side: 'both', angle: 125 },
        length: 0.18,
        membrane: { type: 'membrane.fin', rays: 5 },
      },
    ],
    parts: [
      {
        id: 'eyes',
        type: 'eye.basic',
        attach: { on: 'head', at: 0.3, angle: 70 },
        params: { scale: 1.3, lids: false },
      },
      { id: 'dorsal', type: 'fin.dorsal', attach: { on: 'torso', at: 0.4, angle: 0 } },
      { id: 'tailfin', type: 'fin.tail' },
    ],
    skin: {
      palette: { base: '#506a80', belly: '#e0e8e8', accent: '#203040' },
      material: 'scales',
      layers: [{ type: 'countershade' }],
    },
    motion: { temperament: 'calm' },
  },
});
