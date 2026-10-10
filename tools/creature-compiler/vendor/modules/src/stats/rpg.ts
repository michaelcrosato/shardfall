import { defineStats } from '@spawnforge/core';
import { z } from 'zod';

const params = z.strictObject({
  level: z
    .number()
    .min(0.1)
    .max(10)
    .default(1)
    .describe('Multiplies health, attack and defence (unitless; 1 is a normal encounter)'),
});

const round = (v: number) => Math.round(v);

/**
 * A generic action-RPG mapping, as an example of a per-game stats module: heavier creatures
 * have more health, longer legs make them faster, weapons (teeth, horns, claws) add attack, each
 * head attacks once a turn, and armour (shells, plates, chitin, scales, spikes) adds defence.
 */
export default defineStats({
  id: 'rpg',
  summary:
    'Generic action-RPG numbers: health from mass, speed from legs and gaits, attack from teeth, horns and claws, one attack per head, defence from shells, plates, chitin, scales and spikes, perception from eyes and ears.',
  tags: ['example'],
  params,
  outputs: {
    health:
      'Hit points: grows with the cube root of mass, so a creature twice as long is about twice as tough',
    speed: 'Top speed in m/s, from the fastest gait the legs allow',
    swim: 'Top swimming speed in m/s; 0 for a creature that does not swim',
    fly: 'Cruising speed in the air in m/s; 0 for a creature that does not fly',
    attack:
      'Damage per hit: one head’s teeth and horns, plus claws and other weapons, scaled by size',
    attacks: 'Attacks per turn: one per head',
    defence:
      'Damage reduction, added up: each shell, plate row or armour band 10 × its cover (its size over the torso length, at most 1) + 0.15 a piece; skin chitin 6, scales 3, hide 2; 0.25 a spike; plus the cube root of mass; times level',
    perception: 'How far it notices things, in metres: eye size and ears',
    threat: 'A one-number summary for encounter tables',
  },
  hooks: {
    compute(input, raw) {
      const { level } = raw as z.output<typeof params>;
      const size = Math.cbrt(Math.max(0.01, input.measurements.mass));
      // One hit comes from one head: weapons on the other heads are their own attacks.
      const weapons = input.parts.filter(
        (p) => p.tags.includes('weapon') && (p.head === undefined || p.head === 'head'),
      );
      const attacks = Math.max(1, input.heads);
      const teeth = weapons
        .filter((p) => p.tags.includes('mouth'))
        .reduce((s, p) => s + Math.min(p.count, 20) * 0.15 + p.size * 30, 0);
      const horns = weapons
        .filter((p) => !p.tags.includes('mouth'))
        .reduce((s, p) => s + p.size * 25, 0);
      const claws = input.claws.count * input.claws.length * 8;
      const armour = { chitin: 6, scales: 3, hide: 2, skin: 0 }[input.material] ?? 0;
      // Shells and bands cover the body; a row of plates covers it by their height.
      const torso = Math.max(0.01, input.measurements.torsoLength);
      const plating = input.parts
        .filter((p) => p.tags.includes('armor'))
        .reduce((s, p) => s + 10 * Math.min(1, p.size / torso) + Math.min(p.count, 30) * 0.15, 0);
      const spikes = input.parts
        .filter((p) => p.tags.includes('defence'))
        .reduce((s, p) => s + p.count * 0.25, 0);
      const eyes = input.parts.filter((p) => p.tags.includes('sense'));
      const sight = eyes.reduce((s, p) => s + p.size * 150, 0);
      const health = (10 + 12 * size) * level;
      const attack = (1 + (teeth + horns + claws) * (0.5 + 0.25 * size)) * level;
      const defence = (armour + plating + spikes + size) * level;
      const speed = input.speed.max;
      return {
        health: round(health),
        speed: Math.round(speed * 10) / 10,
        swim: Math.round((input.speed.swim ?? 0) * 10) / 10,
        fly: Math.round((input.speed.fly ?? 0) * 10) / 10,
        attack: round(attack),
        attacks,
        defence: round(defence),
        perception: round(8 + sight + input.measurements.height * 4),
        threat: round(
          (health * (attack * attacks + 1) * (1 + defence / 10) * (1 + speed / 5)) ** 0.5,
        ),
      };
    },
  },
});
