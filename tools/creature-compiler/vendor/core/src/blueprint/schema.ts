import { z } from 'zod';
import { FORMAT } from '../format.ts';
import type { PackDefaults } from '../registry.ts';
import { HARMONIES, isColor } from './colors.ts';

/**
 * The blueprint schema. Everything has a default except item ids and part types, so short
 * blueprints are valid. Module-specific fields (part `params`, foot and layer fields) are checked
 * against each module's own schema in a second pass; see validate.ts.
 */

export const SECTIONS = ['torso', 'neck', 'head', 'tail'] as const;
/** `spine` runs from the head end of the neck (0) through the torso to the tail tip (1). */
export const VIRTUAL_SECTIONS = ['spine'] as const;
export type Section = (typeof SECTIONS)[number];
export const SIDES = ['both', 'left', 'right', 'center'] as const;
export type Side = (typeof SIDES)[number];
export const HEAD_SHAPES = ['round', 'snout', 'flat', 'wedge'] as const;
export const CROSS_SECTIONS = ['round', 'tall', 'wide'] as const;
export const TEMPERAMENTS = ['calm', 'stalking', 'skittish', 'aggressive', 'lumbering'] as const;
export const REGIONS = ['all', 'back', 'belly', 'head', 'torso', 'limbs', 'tail', 'wings'] as const;
export const SKIN_MATERIALS = ['skin', 'scales', 'chitin', 'hide'] as const;
export const LIMB_ROLES = ['leg', 'arm', 'wing', 'fin', 'tentacle'] as const;
export type LimbRoleName = (typeof LIMB_ROLES)[number];
/** Fields only some roles take; every limb also takes the common ones (id, attach, length, …). */
export const ROLE_FIELDS: Record<LimbRoleName, readonly string[]> = {
  leg: ['splay', 'lift', 'stance', 'foot'],
  arm: ['splay', 'lift', 'foot'],
  wing: ['membrane', 'foot'],
  fin: ['membrane', 'foot'],
  tentacle: ['curl', 'curlStart', 'foot'],
};
const ROLE_ONLY = ['splay', 'lift', 'stance', 'membrane', 'curl', 'curlStart'];

/** Removes the fields a limb's role does not take (for writing a resolved limb back out). */
export function roleFieldsOnly(limb: Record<string, unknown>): void {
  const role = (typeof limb.role === 'string' ? limb.role : 'leg') as LimbRoleName;
  const own = ROLE_FIELDS[role] ?? [];
  for (const key of ROLE_ONLY) if (!own.includes(key)) delete limb[key];
  for (const [key, value] of Object.entries(limb)) if (value === undefined) delete limb[key];
}

/** Where an area-slot part sits on what it attaches to. */
export const AREAS = ['back', 'belly', 'sides', 'all'] as const;
export const STANCES = ['plantigrade', 'digitigrade', 'unguligrade'] as const;
export const TONGUES = ['none', 'flat', 'forked'] as const;
export const MEDIA = ['land', 'water', 'air'] as const;

/**
 * Defaults per limb role. Legs and arms keep format 0.1's; the new roles get shapes that work
 * as they are, so `{ "id": "wing", "role": "wing" }` is a usable wing.
 */
export const ROLE_DEFAULTS: Record<
  LimbRoleName,
  {
    readonly at: number;
    readonly angle: number;
    readonly length: number;
    readonly segments: number;
    readonly radius: readonly number[];
  }
> = {
  leg: { at: 0.5, angle: 100, length: 0.5, segments: 3, radius: [0.06, 0.03] },
  arm: { at: 0.5, angle: 100, length: 0.5, segments: 3, radius: [0.06, 0.03] },
  wing: { at: 0.2, angle: 40, length: 1.2, segments: 3, radius: [0.05, 0.015] },
  fin: { at: 0.25, angle: 115, length: 0.35, segments: 2, radius: [0.05, 0.02] },
  tentacle: { at: 0.9, angle: 150, length: 1.5, segments: 10, radius: [0.06, 0.008] },
};

/** Item ids: lowercase, no dots (mirrored copies get `.L` and `.R`). */
export const ITEM_ID = /^[a-z][a-z0-9_-]*$/;
/** Palette colour names, e.g. `base`, `belly`, `accent`, `hornTip`. */
export const PALETTE_NAME = /^[a-z][a-zA-Z0-9]*$/;

export const DEFAULT_PALETTE = { base: '#7a6a50', belly: '#d9cdb0', accent: '#3b2e22' };

/** Module ids the schema offers as enums. Empty lists fall back to plain strings. */
export interface ModuleIds {
  bodyPlan: readonly string[];
  part: readonly string[];
  foot: readonly string[];
  membrane?: readonly string[];
  pattern: readonly string[];
  gait: readonly string[];
  action: readonly string[];
}

const NO_IDS: ModuleIds = {
  bodyPlan: [],
  part: [],
  foot: [],
  membrane: [],
  pattern: [],
  gait: [],
  action: [],
};

function idEnum(ids: readonly string[], what: string) {
  return ids.length > 0
    ? z.enum(ids as [string, ...string[]]).describe(`${what} id`)
    : z.string().describe(`${what} id`);
}

const range = (min: number, max: number) => z.number().min(min).max(max);

/** One number, or a list spread evenly along the section and smoothly interpolated. */
function profile(min: number, max: number, description: string) {
  const value = range(min, max);
  return z.union([value, z.array(value).min(1).max(16)]).describe(description);
}

export const colorSchema = z
  .string()
  .refine(isColor, { message: 'not a colour' })
  .describe('Colour: "#rrggbb", "#rgb" or a CSS colour name');

/**
 * A palette name such as "accent", or a literal colour. Module params holding colours must be
 * named `color` or end in `Color` so validation and expansion resolve them.
 */
export function colorRef(fallback: string) {
  return z
    .string()
    .min(1)
    .default(fallback)
    .describe('A palette name such as "accent", or a colour such as "#2a1e14"');
}

/**
 * A gait setting that may change with speed: one number, or `[slowest, fastest]` across the
 * gait's speed range, interpolated in Froude number (docs/design/10.1-gaits.md).
 */
export function speedProfile(min: number, max: number) {
  const n = z.number().min(min).max(max);
  return z.union([n, z.array(n).length(2)]);
}

/** `defaults` come from the packs (`Registry.defaults()`): the foot and layers a blueprint gets. */
export function buildBlueprintSchema(ids: ModuleIds = NO_IDS, defaults: PackDefaults = {}) {
  const torso = z
    .strictObject({
      radius: profile(
        0.02,
        1,
        'Radius from the neck end to the tail end, in torso lengths',
      ).default([0.14, 0.18, 0.16, 0.12]),
      arch: range(-0.5, 0.5)
        .default(0)
        .describe('Upward bow of the spine, as a share of torso length; negative sags'),
      pitch: range(-30, 90)
        .default(0)
        .describe(
          'Degrees the torso tilts nose-up: 0 is horizontal, about 75 for an upright biped',
        ),
      crossSection: z.enum(CROSS_SECTIONS).default('round').describe('Shape across the torso'),
      segments: z.number().int().min(3).max(12).default(6).describe('Spine bones in the torso'),
    })
    .describe('The main body. Its length is the blueprint `scale`.');

  const neck = z
    .strictObject({
      length: range(0, 1.5).default(0.3).describe('Neck length in torso lengths; 0 for no neck'),
      radius: profile(
        0.01,
        0.6,
        'Radius from the head end to the torso end, in torso lengths',
      ).default([0.07, 0.09]),
      pitch: range(-60, 90).default(20).describe('Degrees the neck rises above horizontal'),
      crossSection: z.enum(CROSS_SECTIONS).default('round').describe('Shape across the neck'),
      segments: z.number().int().min(1).max(8).default(3).describe('Bones in the neck'),
      count: z
        .number()
        .int()
        .min(1)
        .max(9)
        .default(1)
        .describe('Necks, each with its own head shaped like body.head (a hydra has several)'),
      spread: range(0, 170)
        .optional()
        .describe('Degrees between the outermost necks as they fan out; default 25 per extra neck'),
      curve: range(-90, 90)
        .default(0)
        .describe('Degrees of S-bend: forward at the base and back below the head (a swan)'),
    })
    .describe('Joins the head to the front of the torso.');

  const head = z
    .strictObject({
      shape: z.enum(HEAD_SHAPES).default('round').describe('Overall head shape'),
      length: range(0.05, 1).default(0.28).describe('Snout tip to back of skull, in torso lengths'),
      radius: range(0.02, 0.6).default(0.1).describe('Skull radius in torso lengths'),
      jaw: z
        .boolean()
        .default(true)
        .describe('A hinged lower jaw, needed for bite, roar and teeth'),
      pitch: range(-60, 60).default(0).describe('Degrees the snout points above horizontal'),
      crossSection: z.enum(CROSS_SECTIONS).default('round').describe('Shape across the head'),
      lips: range(0, 1).default(0.3).describe('Thickness of the lips along the mouth'),
      tongue: z.enum(TONGUES).default('flat').describe('The tongue in the mouth'),
      brow: range(0, 1).default(0.2).describe('How heavy the brow ridge is'),
    })
    .describe('The head. `at` runs from the snout tip (0) to the back of the skull (1).');

  const tail = z
    .strictObject({
      length: range(0, 4).default(0.6).describe('Tail length in torso lengths; 0 for no tail'),
      radius: profile(0.005, 0.6, 'Radius from the root to the tip, in torso lengths').default([
        0.08, 0.012,
      ]),
      curl: range(-360, 360)
        .default(0)
        .describe('Total degrees the tail bends upward along its length; negative curls down'),
      curlStart: range(0, 0.95)
        .default(0)
        .describe(
          'Share of the tail that stays straight before the curl begins; 0.6 curls only the end',
        ),
      crossSection: z.enum(CROSS_SECTIONS).default('round').describe('Shape across the tail'),
      pitch: range(-90, 60)
        .default(-10)
        .describe('Degrees the tail root points above horizontal; negative droops'),
      segments: z.number().int().min(2).max(24).default(8).describe('Bones in the tail'),
      count: z
        .number()
        .int()
        .min(1)
        .max(9)
        .default(1)
        .describe('Tails, each shaped like this one (a two-tailed fox has 2)'),
      spread: range(0, 170)
        .optional()
        .describe('Degrees between the outermost tails as they fan out; default 20 per extra tail'),
      forkAt: range(0, 0.95)
        .default(0)
        .describe(
          'Where along the tail several tails branch: 0 leaves the torso separately, 0.7 forks near the tip',
        ),
    })
    .describe('Runs back from the torso. `at` runs from the root (0) to the tip (1).');

  const side = z.enum(SIDES).describe('"both" makes a mirrored pair with ids ending .L and .R');

  const limbAttach = (role: LimbRoleName) =>
    z.strictObject({
      on: z.string().default('torso').describe('Body section the limb grows from'),
      at: range(0, 1)
        .default(ROLE_DEFAULTS[role].at)
        .describe('Where along the section: 0 is the snout end, 1 the tail end'),
      side: side.default('both'),
      angle: range(0, 180)
        .default(ROLE_DEFAULTS[role].angle)
        .describe('Degrees around the section from the top: 90 is the side, 180 the belly'),
    });

  /** A slot field on a limb (`foot`, `membrane`): a module object, its id, or null for none. */
  const slotField = (
    ids: readonly string[],
    what: string,
    fallback: string | undefined,
    description: string,
  ) => {
    const type = idEnum(ids, what);
    const object = z
      .object({ type: fallback ? type.default(fallback) : type })
      .catchall(z.unknown())
      .describe(`${what} module; its parameters sit beside \`type\``);
    return z
      .union([object, type, z.null()])
      .prefault((fallback ? {} : null) as never)
      .describe(
        fallback
          ? `${description} (default { "type": "${fallback}" }); the id alone also works; null for none`
          : `${description}; the id alone also works; null for none`,
      );
  };
  const footField = (role: LimbRoleName) =>
    slotField(
      ids.foot,
      'Foot part',
      defaults.foot?.[role],
      role === 'wing' ? "Claw at the wrist (a bat's thumb)" : 'Foot or hand part at the limb tip',
    );
  const membraneField = (role: LimbRoleName) =>
    slotField(
      ids.membrane ?? [],
      'Membrane',
      defaults.membrane?.[role],
      'The surface the limb carries (skin, feathers, a fin)',
    );

  const limbCommon = (role: LimbRoleName) => ({
    id: z.string().regex(ITEM_ID).describe('Unique id; mirrored copies get .L and .R'),
    attach: limbAttach(role).prefault({}),
    length: range(0.05, role === 'tentacle' ? 4 : 3)
      .default(ROLE_DEFAULTS[role].length)
      .describe('Total limb length in torso lengths'),
    segments: z
      .number()
      .int()
      .min(2)
      .max(role === 'tentacle' ? 16 : 4)
      .default(ROLE_DEFAULTS[role].segments)
      .describe(
        role === 'tentacle' ? 'Bones along the tentacle' : 'Bones from hip or shoulder to ankle',
      ),
    radius: profile(0.005, 0.5, 'Radius from root to tip, in torso lengths').default([
      ...ROLE_DEFAULTS[role].radius,
    ]),
    muscle: range(0, 1)
      .optional()
      .describe('How muscled this limb is, 0 to 1; left out, it follows body.muscle'),
    remove: z.literal(true).optional().describe('Delete an inherited limb with this id'),
  });
  const splay = range(-30, 90)
    .default(0)
    .describe('Degrees the limb swings out from under the body; about 50 for sprawlers');
  const lift = range(0, 150)
    .default(0)
    .describe(
      'Arms only: degrees the arm is raised forward from hanging; 90 holds it straight out (pincers)',
    );
  const limb = z
    .discriminatedUnion('role', [
      z
        .strictObject({
          role: z.literal('leg').default('leg').describe('"leg" limbs carry the body'),
          ...limbCommon('leg'),
          splay,
          lift,
          stance: z
            .enum(STANCES)
            .optional()
            .describe(
              'How the foot meets the ground: plantigrade (whole sole), digitigrade (toes), unguligrade (hoof tips); left out, the foot suggests one',
            ),
          foot: footField('leg'),
        })
        .describe('A leg: carries the body; legs come in mirrored pairs'),
      z
        .strictObject({
          role: z.literal('arm').describe('"arm" limbs are free for actions'),
          ...limbCommon('arm'),
          splay,
          lift,
          foot: footField('arm'),
        })
        .describe('An arm: hangs free, for grabbing and striking'),
      z
        .strictObject({
          role: z.literal('wing').describe('"wing" limbs fold at rest and beat in the air'),
          ...limbCommon('wing'),
          membrane: membraneField('wing'),
          foot: footField('wing'),
        })
        .describe('A wing: an arm-like chain carrying a membrane, folded at rest'),
      z
        .strictObject({
          role: z.literal('fin').describe('"fin" limbs steer and beat in water'),
          ...limbCommon('fin'),
          membrane: membraneField('fin'),
          foot: footField('fin'),
        })
        .describe('A fin or flipper: a short flat limb; null membrane makes a flipper'),
      z
        .strictObject({
          role: z.literal('tentacle').describe('"tentacle" limbs curl and reach'),
          ...limbCommon('tentacle'),
          curl: range(-360, 360)
            .default(0)
            .describe(
              'Total degrees the tentacle curls at rest, toward the belly; negative curls toward the back',
            ),
          curlStart: range(0, 0.95)
            .default(0)
            .describe('Share of the tentacle that stays straight before the curl begins'),
          foot: footField('tentacle'),
        })
        .describe('A tentacle: a long tapering chain of up to 16 bones'),
    ])
    .describe('A limb; its role decides its fields and defaults');

  const partAttach = z.strictObject({
    on: z
      .string()
      .optional()
      .describe(
        'Body section (head, jaw, neck, torso, tail; spine runs neck to tail tip), limb id or part id',
      ),
    at: range(0, 1)
      .optional()
      .describe('Where along it: snout-to-tail on sections, root-to-tip on limbs and parts'),
    from: range(0, 1).optional().describe('Start of a row'),
    to: range(0, 1).optional().describe('End of a row'),
    angle: range(0, 180)
      .optional()
      .describe('Degrees around the section from the top: 0 dorsal, 90 side, 180 belly'),
    side: side.optional().describe('Defaults to "both", or "center" when angle is 0 or 180'),
    area: z
      .enum(AREAS)
      .optional()
      .describe('Area-slot parts only: which area of "on" they cover (back, belly, sides, all)'),
  });

  const part = z.strictObject({
    id: z.string().regex(ITEM_ID).describe('Unique id; mirrored copies get .L and .R'),
    type: idEnum(ids.part, 'Part module'),
    attach: partAttach.prefault({}),
    params: z.record(z.string(), z.unknown()).default({}).describe("The part module's parameters"),
    remove: z.literal(true).optional().describe('Delete an inherited part with this id'),
  });

  const layer = z
    .object({
      type: idEnum(ids.pattern, 'Pattern module'),
      id: z
        .string()
        .regex(ITEM_ID)
        .optional()
        .describe('Optional id; keys the layer random stream'),
      region: z.enum(REGIONS).default('all').describe('Where the layer shows'),
      strength: range(0, 1).default(1).describe('Layer opacity'),
    })
    .catchall(z.unknown())
    .describe('One pattern layer; its parameters sit beside `type`');

  const palette = z
    .object({
      base: colorSchema,
      belly: colorSchema,
      accent: colorSchema,
      harmony: z
        .enum(HARMONIES)
        .describe(
          'Generate the base, belly and accent you leave out from the seed: the accent analogous, complementary, triadic, split-complementary or the same hue (monochrome), around the base you give',
        ),
    })
    .catchall(colorSchema)
    .partial()
    .describe('Named colours. base, belly and accent always exist; add any others by name');

  const fur = z
    .strictObject({
      length: range(0.002, 0.3).default(0.03).describe('Hair length in torso lengths'),
      density: range(0, 1).default(0.8).describe('How thick the coat is'),
      region: z
        .union([z.enum(REGIONS), z.array(z.enum(REGIONS)).min(1).max(8)])
        .default('all')
        .describe(
          'Where it grows: a layer region or a list of them, e.g. ["torso", "limbs", "tail"]',
        ),
    })
    .describe('A coat of fur over the skin');

  const skin = z.strictObject({
    palette: palette.default({}),
    material: z.enum(SKIN_MATERIALS).default('skin').describe('Base surface under the patterns'),
    fur: z
      .union([fur, z.null()])
      .optional()
      .describe('Fur over the skin, where its region says; null removes an inherited coat'),
    layers: z
      .array(layer)
      .max(12)
      .prefault((defaults.layers ?? []).map((l) => ({ ...l })) as z.input<typeof layer>[])
      .describe('Pattern stack, bottom first'),
  });

  const moduleRef = (kind: readonly string[], what: string) =>
    z.union([idEnum(kind, what), z.object({ type: idEnum(kind, what) }).catchall(z.unknown())]);

  const motion = z.strictObject({
    temperament: z
      .enum(TEMPERAMENTS)
      .default('calm')
      .describe('Sets pace, posture and idle behaviour'),
    gaits: z
      .array(moduleRef(ids.gait, 'Gait'))
      .max(12)
      .optional()
      .describe(
        'Gaits it may use; by default every gait that suits its body; a list replaces the defaults only for the media its gaits serve',
      ),
    actions: z
      .array(moduleRef(ids.action, 'Action'))
      .max(12)
      .optional()
      .describe('Actions it can perform; by default every action its body allows'),
    media: z
      .strictObject({
        land: z.boolean().optional().describe('Walks (or slithers) on the ground'),
        water: z.boolean().optional().describe('Swims'),
        air: z.boolean().optional().describe('Flies; needs a wing'),
      })
      .optional()
      .describe(
        'Where it moves; each switch left out follows the body (land with legs, water with fins or tentacles on the torso and no legs, air with wings)',
      ),
  });

  return z.strictObject({
    format: z.literal(FORMAT).describe('Format id and version'),
    name: z.string().min(1).max(80).default('Unnamed creature'),
    seed: z
      .number()
      .int()
      .min(0)
      .max(4294967295)
      .default(1)
      .describe('Same blueprint and seed, same monster'),
    extends: idEnum(ids.bodyPlan, 'Body plan')
      .optional()
      .describe('Body-plan preset to start from'),
    scale: range(0.05, 20)
      .default(1)
      .describe('Torso length in metres; every other length is a multiple of it'),
    body: z
      .strictObject({
        torso: torso.prefault({}),
        neck: neck.prefault({}),
        head: head.prefault({}),
        tail: tail.prefault({}),
        muscle: range(0, 1)
          .default(0.5)
          .describe('How muscled the body is, 0 (smooth tubes) to 1 (heavily built)'),
      })
      .prefault({}),
    limbs: z
      .array(limb)
      .max(16)
      .default([])
      .describe('Legs and arms. Lists merge with the preset by id'),
    parts: z
      .array(part)
      .max(64)
      .default([])
      .describe('Hard parts. Lists merge with the preset by id'),
    skin: skin.prefault({}),
    motion: motion.prefault({}),
  });
}

export type BlueprintSchema = ReturnType<typeof buildBlueprintSchema>;
/** A blueprint after defaults, before module params and mirroring are resolved. */
export type BlueprintDoc = z.output<BlueprintSchema>;
export type LimbDoc = BlueprintDoc['limbs'][number];
export type PartDoc = BlueprintDoc['parts'][number];
export type LayerDoc = BlueprintDoc['skin']['layers'][number];
