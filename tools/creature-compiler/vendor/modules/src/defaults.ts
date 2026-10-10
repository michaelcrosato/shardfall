import type { PackDefaults } from '@spawnforge/core';

/** What the basic pack gives a blueprint that leaves these out. */
export default {
  foot: { leg: 'foot.claw', arm: 'foot.claw' },
  membrane: { wing: 'membrane.bat', fin: 'membrane.fin' },
  layers: [{ type: 'countershade' }],
  bodyPlan: 'quadruped',
  lineage: ['bird', 'insect', 'snake'],
  habitat: { aquatic: 'water' },
} satisfies PackDefaults;
