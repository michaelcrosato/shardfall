import { defineBodyPlan } from '@spawnforge/core';

export default defineBodyPlan({
  id: 'quadruped',
  summary: 'Four legs under a horizontal torso, a raised neck and a tail; walks and trots.',
  tags: ['legs:4', 'mammal', 'reptile'],
  preset: {
    scale: 1,
    body: {
      torso: { radius: [0.14, 0.17, 0.16, 0.13], arch: 0.08, pitch: 0, segments: 6 },
      neck: { length: 0.32, radius: [0.07, 0.1], pitch: 30, segments: 3 },
      head: { shape: 'snout', length: 0.3, radius: 0.1, jaw: true, pitch: -10 },
      tail: { length: 0.7, radius: [0.07, 0.012], pitch: -20, curl: 10, segments: 8 },
    },
    limbs: [
      {
        id: 'foreleg',
        role: 'leg',
        attach: { on: 'torso', at: 0.12, side: 'both', angle: 115 },
        length: 0.55,
        segments: 3,
        radius: [0.06, 0.03],
        foot: { type: 'foot.claw', toes: 4 },
      },
      {
        id: 'hindleg',
        role: 'leg',
        attach: { on: 'torso', at: 0.88, side: 'both', angle: 115 },
        length: 0.6,
        segments: 3,
        radius: [0.07, 0.03],
        foot: { type: 'foot.claw', toes: 4 },
      },
    ],
    parts: [{ id: 'eyes', type: 'eye.basic', attach: { on: 'head', at: 0.35, angle: 55 } }],
    skin: {
      palette: { base: '#7a6a50', belly: '#d9cdb0', accent: '#3b2e22' },
      layers: [{ type: 'countershade' }],
    },
    motion: { temperament: 'calm' },
  },
});
