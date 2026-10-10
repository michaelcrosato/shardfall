import { defineBodyPlan } from '@spawnforge/core';

export default defineBodyPlan({
  id: 'octopod',
  summary:
    'Eight sprawled legs on a small front body and a big round abdomen, like a spider or a scorpion.',
  tags: ['legs:8', 'arachnid', 'sprawl'],
  preset: {
    scale: 0.7,
    body: {
      // A cephalothorax carrying the legs, a narrow waist, then a bulbous abdomen.
      torso: {
        radius: [0.11, 0.13, 0.1, 0.07, 0.19, 0.22, 0.16],
        pitch: 0,
        segments: 8,
        crossSection: 'wide',
      },
      neck: { length: 0.03, radius: [0.07, 0.08], pitch: 0, segments: 1 },
      head: { shape: 'round', length: 0.16, radius: 0.08, jaw: true, pitch: -5 },
      tail: { length: 0 },
    },
    limbs: [
      {
        id: 'leg1',
        role: 'leg',
        attach: { on: 'torso', at: 0.04, side: 'both', angle: 115 },
        length: 1.1,
        segments: 3,
        radius: [0.03, 0.012],
        splay: 60,
        foot: { type: 'foot.claw', toes: 1 },
      },
      {
        id: 'leg2',
        role: 'leg',
        attach: { on: 'torso', at: 0.13, side: 'both', angle: 115 },
        length: 0.95,
        segments: 3,
        radius: [0.03, 0.012],
        splay: 65,
        foot: { type: 'foot.claw', toes: 1 },
      },
      {
        id: 'leg3',
        role: 'leg',
        attach: { on: 'torso', at: 0.22, side: 'both', angle: 115 },
        length: 0.85,
        segments: 3,
        radius: [0.03, 0.012],
        splay: 65,
        foot: { type: 'foot.claw', toes: 1 },
      },
      {
        id: 'leg4',
        role: 'leg',
        attach: { on: 'torso', at: 0.31, side: 'both', angle: 115 },
        length: 1,
        segments: 3,
        radius: [0.03, 0.012],
        splay: 60,
        foot: { type: 'foot.claw', toes: 1 },
      },
    ],
    parts: [
      {
        id: 'eyes',
        type: 'eye.basic',
        attach: { on: 'head', at: 0.25, angle: 35 },
        params: { scale: 1.3, lids: false, scleraColor: '#140c10', irisColor: '#401020' },
      },
      {
        id: 'side-eyes',
        type: 'eye.basic',
        attach: { on: 'head', at: 0.4, angle: 70 },
        params: { scale: 0.8, lids: false, scleraColor: '#140c10', irisColor: '#401020' },
      },
    ],
    skin: {
      palette: { base: '#3a3030', belly: '#6a5a50', accent: '#1a1414' },
      material: 'chitin',
      layers: [{ type: 'countershade', softness: 0.2 }],
    },
    motion: { temperament: 'stalking' },
  },
});
