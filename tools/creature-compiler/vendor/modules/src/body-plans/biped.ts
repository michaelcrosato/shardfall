import { defineBodyPlan } from '@spawnforge/core';

export default defineBodyPlan({
  id: 'biped',
  summary:
    'Upright torso on two legs with two free arms; walks. Lean the torso forward and add a tail for a raptor.',
  tags: ['legs:2', 'arms:2', 'humanoid'],
  preset: {
    scale: 0.7,
    body: {
      torso: {
        radius: [0.13, 0.2, 0.18, 0.16],
        arch: 0.04,
        pitch: 82,
        segments: 5,
        crossSection: 'wide',
      },
      neck: { length: 0.2, radius: [0.065, 0.075], pitch: 82, segments: 2 },
      head: { shape: 'round', length: 0.3, radius: 0.13, jaw: true, pitch: 0 },
      tail: { length: 0 },
    },
    limbs: [
      {
        id: 'leg',
        role: 'leg',
        attach: { on: 'torso', at: 0.94, side: 'both', angle: 130 },
        length: 1.3,
        segments: 2,
        radius: [0.1, 0.05],
        foot: { type: 'foot.claw', toes: 3 },
      },
      {
        id: 'arm',
        role: 'arm',
        attach: { on: 'torso', at: 0.12, side: 'both', angle: 92 },
        length: 1.05,
        segments: 2,
        radius: [0.065, 0.04],
        foot: { type: 'foot.claw', toes: 4 },
      },
    ],
    parts: [{ id: 'eyes', type: 'eye.basic', attach: { on: 'head', at: 0.3, angle: 62 } }],
    skin: {
      palette: { base: '#6f7a4a', belly: '#c9c39a', accent: '#2f3320' },
      layers: [{ type: 'countershade' }],
    },
    motion: { temperament: 'calm' },
  },
});
