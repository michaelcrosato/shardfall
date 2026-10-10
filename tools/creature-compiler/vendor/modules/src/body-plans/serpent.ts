import { defineBodyPlan } from '@spawnforge/core';

export default defineBodyPlan({
  id: 'serpent',
  summary: 'Legless snake body with a long tail and scales; slithers with a wave down the spine.',
  tags: ['legs:0', 'reptile', 'snake'],
  preset: {
    scale: 0.6,
    body: {
      torso: { radius: [0.07, 0.085, 0.09, 0.085], pitch: 0, segments: 8 },
      neck: { length: 0.15, radius: [0.065, 0.07], pitch: 10, segments: 2 },
      head: {
        shape: 'wedge',
        length: 0.2,
        radius: 0.075,
        jaw: true,
        pitch: 0,
        lips: 0,
        tongue: 'forked',
      },
      tail: { length: 2.2, radius: [0.085, 0.01], pitch: 0, curl: 0, segments: 16 },
    },
    limbs: [],
    parts: [
      {
        id: 'eyes',
        type: 'eye.basic',
        attach: { on: 'head', at: 0.35, angle: 70 },
        params: { pupil: 'slit', lids: false },
      },
    ],
    skin: {
      palette: { base: '#4f6b3a', belly: '#d8d0a0', accent: '#24301a' },
      material: 'scales',
      layers: [{ type: 'countershade' }, { type: 'scales', size: 0.025 }],
    },
    motion: { temperament: 'stalking' },
  },
});
