import { defineBodyPlan } from '@spawnforge/core';

export default defineBodyPlan({
  id: 'centaur',
  summary:
    'A horse-like body on four legs with an upright, torso-like neck carrying two arms and the head.',
  tags: ['legs:4', 'arms:2', 'humanoid', 'mammal'],
  preset: {
    scale: 1,
    body: {
      muscle: 0.6,
      torso: { radius: [0.15, 0.17, 0.16, 0.13], arch: 0.04, pitch: 0, segments: 6 },
      // The upright front: a neck shaped like a human torso, from the neck under the head down
      // through the shoulders and chest to the waist; arms on it make it stand straight up and
      // give it shoulders and a chest (docs/design/9.2-legs-centaurs.md).
      neck: {
        length: 0.72,
        radius: [0.042, 0.045, 0.07, 0.135, 0.14, 0.125, 0.105, 0.1, 0.11, 0.125],
        pitch: 86,
        segments: 5,
        crossSection: 'wide',
      },
      head: {
        shape: 'round',
        length: 0.2,
        radius: 0.08,
        jaw: true,
        pitch: 0,
        lips: 0.5,
        brow: 0.5,
      },
      tail: { length: 0.6, radius: [0.05, 0.02], pitch: -40, segments: 8 },
    },
    limbs: [
      {
        id: 'foreleg',
        role: 'leg',
        attach: { on: 'torso', at: 0.12, side: 'both', angle: 115 },
        length: 0.8,
        segments: 3,
        radius: [0.06, 0.028],
        foot: 'foot.hoof',
      },
      {
        id: 'hindleg',
        role: 'leg',
        attach: { on: 'torso', at: 0.88, side: 'both', angle: 115 },
        length: 0.85,
        segments: 3,
        radius: [0.07, 0.028],
        foot: 'foot.hoof',
      },
      {
        id: 'arm',
        role: 'arm',
        attach: { on: 'neck', at: 0.3, side: 'both', angle: 90 },
        length: 0.62,
        segments: 3,
        radius: [0.055, 0.04, 0.03],
        foot: 'hand.grasp',
      },
    ],
    parts: [{ id: 'eyes', type: 'eye.basic', attach: { on: 'head', at: 0.3, angle: 62 } }],
    skin: {
      palette: { base: '#7a5a3a', belly: '#c8b090', accent: '#3a2a1a' },
      layers: [{ type: 'countershade' }],
    },
    motion: { temperament: 'calm' },
  },
});
