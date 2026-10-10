import { defineBodyPlan } from '@spawnforge/core';

export default defineBodyPlan({
  id: 'hexapod',
  summary: 'Six sprawled legs on a low, wide body with chitin skin; runs a tripod gait.',
  tags: ['legs:6', 'insect', 'sprawl'],
  preset: {
    scale: 0.8,
    body: {
      // A thorax for the legs, a narrow waist, then a broad abdomen.
      torso: {
        radius: [0.1, 0.14, 0.085, 0.17, 0.15, 0.1],
        pitch: 0,
        segments: 8,
        crossSection: 'wide',
      },
      neck: { length: 0.06, radius: [0.07, 0.08], pitch: 0, segments: 1 },
      head: { shape: 'round', length: 0.22, radius: 0.09, jaw: true, pitch: -10 },
      tail: { length: 0 },
    },
    limbs: [
      {
        id: 'frontleg',
        role: 'leg',
        attach: { on: 'torso', at: 0.1, side: 'both', angle: 120 },
        length: 0.6,
        segments: 3,
        radius: [0.035, 0.015],
        splay: 55,
        foot: { type: 'foot.claw', toes: 1 },
      },
      {
        id: 'midleg',
        role: 'leg',
        attach: { on: 'torso', at: 0.22, side: 'both', angle: 120 },
        length: 0.62,
        segments: 3,
        radius: [0.035, 0.015],
        splay: 60,
        foot: { type: 'foot.claw', toes: 1 },
      },
      {
        id: 'hindleg',
        role: 'leg',
        attach: { on: 'torso', at: 0.34, side: 'both', angle: 120 },
        length: 0.7,
        segments: 3,
        radius: [0.035, 0.015],
        splay: 55,
        foot: { type: 'foot.claw', toes: 1 },
      },
    ],
    parts: [
      {
        id: 'eyes',
        type: 'eye.basic',
        attach: { on: 'head', at: 0.3, angle: 60 },
        params: { scale: 1.6, lids: false },
      },
    ],
    skin: {
      palette: { base: '#3d2f45', belly: '#8a7a6a', accent: '#1a141f' },
      material: 'chitin',
      layers: [{ type: 'countershade', softness: 0.2 }],
    },
    motion: { temperament: 'skittish' },
  },
});
