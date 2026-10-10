export {
  type Analysis,
  type AnalyzeOptions,
  analyzeCreature,
  describeCreature,
  type LimbHit,
  type MotionCheck,
} from './analysis/analyze.ts';
export {
  computeStats,
  type StatsHooks,
  type StatsInput,
  statsInput,
} from './analysis/stats.ts';
export * from './blueprint/colors.ts';
export type * from './blueprint/creature.ts';
export { type BlueprintDiff, diffBlueprints } from './blueprint/diff.ts';
export { headSpacing, instanceNames, instanceSuffixes } from './blueprint/instances.ts';
export { formatIssue, formatPath, type Issue } from './blueprint/issues.ts';
export { cloneJson, ID_LISTS, isRecord, mergeBlueprint } from './blueprint/merge.ts';
export { KNOWN_FORMATS, type Migration, migrate, toCurrentFormat } from './blueprint/migrate.ts';
export { normalizeBlueprint } from './blueprint/normalize.ts';
export {
  applyPatch,
  diffJson,
  formatDiff,
  type PatchChange,
  type PatchOp,
  type PatchResult,
} from './blueprint/patch.ts';
export { buildable, notBuilt } from './blueprint/planned.ts';
export { randomBlueprint, sampleSchema } from './blueprint/random.ts';
export {
  AREAS,
  type BlueprintDoc,
  buildBlueprintSchema,
  CROSS_SECTIONS,
  colorRef,
  DEFAULT_PALETTE,
  HEAD_SHAPES,
  ITEM_ID,
  LIMB_ROLES,
  MEDIA,
  REGIONS,
  ROLE_DEFAULTS,
  ROLE_FIELDS,
  SECTIONS,
  SIDES,
  SKIN_MATERIALS,
  STANCES,
  speedProfile,
  TEMPERAMENTS,
  TONGUES,
} from './blueprint/schema.ts';
export { didYouMean, editDistance } from './blueprint/suggest.ts';
export {
  blueprintSchemaFor,
  expandCreature,
  minimalBlueprint,
  type ResolvedDoc,
  resolveBlueprint,
  resolveDocument,
  type ValidateOptions,
  type ValidationResult,
  validateBlueprint,
} from './blueprint/validate.ts';
export {
  type Anchor,
  type AnchorResult,
  anchorAt,
  placeOnSkin,
} from './compile/anchor.ts';
export * from './compile/compile.ts';
export { type LimbIkSetup, solveLimb } from './compile/ik.ts';
export type { MembraneLook, Spar } from './compile/membranes.ts';
export { type MouthLine, mouthLine } from './compile/mouth.ts';
export {
  AREA_ANGLES,
  type AreaBand,
  type EmitOptions,
  type EyeOptions,
  type PanelLook,
  type PartBuildContext,
  type PartChain,
  type PartHooks,
  type ScatterPoint,
  type Socket,
  type WingContext,
} from './compile/parts.ts';
export { buildSdf, type Sdf, SdfEvaluator } from './compile/sdf.ts';
export { buildSkeleton, type SkeletonBuild } from './compile/skeleton.ts';
export type * from './compile/types.ts';
export { allEyes, mainHead } from './compile/types.ts';
export {
  BAT_STYLE,
  type DigitChain,
  type DigitContext,
  FIN_STYLE,
  type WingHooks,
  type WingStyle,
} from './compile/wings.ts';
export {
  type BakedColors,
  bakeEyeColors,
  bakePartColors,
  bakeSkinColors,
  bakeVertexColors,
  EYE_ROUGHNESS,
  eyeColor,
  limbAndWings,
  membraneSurfaceAt,
  srgbToLinear,
  surfaceAt,
} from './export/bake.ts';
export {
  LOD_RATIOS,
  type LodChain,
  type LodLevel,
  type LodView,
  pickLevel,
  projectedError,
  screenCoverage,
} from './export/lods.ts';
export type { BakedMap, BakedTextures, TexturedMesh } from './export/textures.ts';
export { FORMAT } from './format.ts';
export * from './geometry/kit.ts';
export {
  type ActionContext,
  type ActionGoals,
  type ActionHooks,
  envelope,
  type LeapPlan,
  type LeapTiming,
  ramp,
} from './motion/actions.ts';
export { type BakedClip, type BakeOptions, bakeClips, clipNames } from './motion/clips.ts';
export {
  type GaitInfo,
  type Ground,
  type GroundSample,
  MotionController,
  type MotionData,
  type MotionEvent,
  type MotionOptions,
  type Water,
  type WaterSample,
} from './motion/controller.ts';
export { applyFace, applyRest, JAW_OPEN } from './motion/face.ts';
export { motionData } from './motion/gaits.ts';
export { Pose } from './motion/pose.ts';
export {
  checkScenario,
  parseScenario,
  type Scenario,
  type ScenarioCall,
  type ScenarioResult,
  ScenarioRun,
  ScenarioSchema,
} from './motion/scenario.ts';
export { type Lake, openSea, slope, testCourse, withLake } from './motion/terrain.ts';
export { applyStations, applyWings } from './motion/wings.ts';
export * from './registry.ts';
export { createRng, deriveSeed, type Rng } from './rng.ts';
export * from './shading/compose.ts';
export { cpuKit, hash3u } from './shading/cpu.ts';
export { COAT, type FurEye, type FurInputs, furEyes, furReach } from './shading/fur.ts';
export type { Kit, LayerOutput, PatternHooks, Surface } from './shading/kit.ts';
export { cells, fbm, valueNoise } from './shading/noise.ts';
export {
  type CrossbreedOptions,
  type CrossbreedResult,
  crossbreed,
} from './variation/crossbreed.ts';
export {
  type ColorRange,
  type GenerateConstraints,
  type GenerateOptions,
  type GenerateResult,
  generate,
  measureBody,
  type Range,
  type ThemeBias,
} from './variation/generate.ts';
export { expand, type Gene, genesOf, type VariationResult } from './variation/genes.ts';
export { type MutateOptions, mutate } from './variation/mutate.ts';
export {
  instantiate,
  isRange,
  isSpecies,
  resolveSpecies,
  type Species,
  validateSpecies,
} from './variation/species.ts';
