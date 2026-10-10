import { defineBodyPlan } from '@spawnforge/core';

export default defineBodyPlan({
  id: 'wyvern',
  summary:
    'Two legs under a horizontal body, wings for forelimbs, a long neck and tail; for bats and wyverns.',
  tags: ['legs:2', 'wings:2', 'dragon', 'flyer'],
  preset: {
    scale: 1,
    body: {
      torso: { radius: [0.13, 0.15, 0.13, 0.1], arch: 0.06, pitch: 10, segments: 6 },
      neck: { length: 0.5, radius: [0.06, 0.09], pitch: 35, segments: 4 },
      head: { shape: 'wedge', length: 0.32, radius: 0.09, jaw: true, pitch: -5 },
      tail: { length: 1.6, radius: [0.09, 0.012], pitch: -5, curl: 15, segments: 14 },
    },
    limbs: [
      {
        id: 'leg',
        role: 'leg',
        attach: { on: 'torso', at: 0.56, side: 'both', angle: 125 },
        length: 0.75,
        segments: 3,
        radius: [0.08, 0.035],
        foot: { type: 'foot.claw', toes: 3 },
      },
      {
        id: 'wing',
        role: 'wing',
        attach: { on: 'torso', at: 0.15, side: 'both', angle: 45 },
        length: 1.6,
        segments: 3,
        radius: [0.05, 0.015],
        membrane: { type: 'membrane.bat', fingers: 4, trailing: 'body' },
      },
    ],
    parts: [
      {
        id: 'eyes',
        type: 'eye.basic',
        attach: { on: 'head', at: 0.35, angle: 55 },
        params: { pupil: 'slit' },
      },
      { id: 'teeth', type: 'teeth.row', params: { fangs: 2 } },
    ],
    skin: {
      palette: { base: '#5a3a3a', belly: '#c0a080', accent: '#2a1a1a' },
      material: 'scales',
      layers: [{ type: 'countershade' }],
    },
    motion: { temperament: 'aggressive' },
  },
});
