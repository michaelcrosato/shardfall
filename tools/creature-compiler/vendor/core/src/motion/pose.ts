import { Quaternion, Vector3 } from 'three';
import type { BonesData } from '../compile/compile.ts';

const Y = new Vector3(0, 1, 0);

/**
 * A skeleton pose: a local rotation and position per bone, with world transforms worked out by
 * forward kinematics. Bones are ordered parents first, as the compiler emits them.
 */
export class Pose {
  readonly count: number;
  readonly parents: Int16Array;
  /** Whether each bone has children. */
  private readonly parent: Uint8Array;
  /**
   * Rest local transforms: the pose a creature stands in, which `reset` returns to. It equals
   * the bind pose except where `BonesData.rest` says otherwise (folded wings,
   * docs/design/9.3-wings-fins.md).
   */
  readonly restRot: Quaternion[];
  readonly restPos: Vector3[];
  /** Rest world transforms (model space). */
  readonly restWorldRot: Quaternion[];
  readonly restWorldPos: Vector3[];
  /** Bind local rotations (the pose the meshes were built in) and their world transforms. */
  readonly bindRot: Quaternion[];
  readonly bindWorldRot: Quaternion[];
  readonly bindWorldPos: Vector3[];
  /** Current local transforms. */
  readonly rot: Quaternion[];
  readonly pos: Vector3[];
  /** Current world transforms, valid after `solve()`. */
  readonly worldRot: Quaternion[];
  readonly worldPos: Vector3[];
  readonly lengths: Float32Array;
  /** Chest expansion, 0 to 1, for the renderer (breathing). */
  breath = 0;
  /** Eyelids, 0 open to 1 shut, for the renderer. */
  blink = 0;
  /** Seconds on the creature's clock, for pulsing patterns (0 in stills). */
  time = 0;

  constructor(bones: BonesData) {
    const n = bones.names.length;
    this.count = n;
    this.parents = bones.parents;
    this.parent = new Uint8Array(bones.parents.length);
    for (const p of bones.parents) if (p >= 0) this.parent[p] = 1;
    this.lengths = bones.lengths;
    this.bindWorldPos = Array.from({ length: n }, (_, i) =>
      new Vector3().fromArray(bones.positions, i * 3),
    );
    this.bindWorldRot = Array.from({ length: n }, (_, i) =>
      new Quaternion().fromArray(bones.rotations, i * 4),
    );
    this.bindRot = [];
    this.restPos = [];
    for (let i = 0; i < n; i++) {
      const p = bones.parents[i] as number;
      if (p >= 0) {
        const inv = (this.bindWorldRot[p] as Quaternion).clone().invert();
        this.restPos.push(
          (this.bindWorldPos[i] as Vector3)
            .clone()
            .sub(this.bindWorldPos[p] as Vector3)
            .applyQuaternion(inv),
        );
        this.bindRot.push(inv.multiply(this.bindWorldRot[i] as Quaternion));
      } else {
        this.restPos.push((this.bindWorldPos[i] as Vector3).clone());
        this.bindRot.push((this.bindWorldRot[i] as Quaternion).clone());
      }
    }
    const rest = bones.rest;
    this.restRot = this.bindRot.map((q, i) =>
      rest ? new Quaternion().fromArray(rest, i * 4) : q.clone(),
    );
    this.rot = this.restRot.map((q) => q.clone());
    this.pos = this.restPos.map((v) => v.clone());
    this.worldRot = this.bindWorldRot.map((q) => q.clone());
    this.worldPos = this.bindWorldPos.map((v) => v.clone());
    if (rest) {
      this.solve();
      this.restWorldRot = this.worldRot.map((q) => q.clone());
      this.restWorldPos = this.worldPos.map((v) => v.clone());
    } else {
      this.restWorldRot = this.bindWorldRot;
      this.restWorldPos = this.bindWorldPos;
    }
  }

  /** Back to the rest pose. */
  reset(): void {
    for (let i = 0; i < this.count; i++) {
      (this.rot[i] as Quaternion).copy(this.restRot[i] as Quaternion);
      (this.pos[i] as Vector3).copy(this.restPos[i] as Vector3);
    }
  }

  /** Forward kinematics from bone `from` onward (all bones by default). */
  solve(from = 0): void {
    for (let i = from; i < this.count; i++) this.solveBone(i);
  }

  /** World transform of one bone from its parent's (which must be current). */
  solveBone(i: number): void {
    const p = this.parents[i] as number;
    const wr = this.worldRot[i] as Quaternion;
    const wp = this.worldPos[i] as Vector3;
    if (p < 0) {
      wr.copy(this.rot[i] as Quaternion);
      wp.copy(this.pos[i] as Vector3);
      return;
    }
    const pr = this.worldRot[p] as Quaternion;
    wr.multiplyQuaternions(pr, this.rot[i] as Quaternion);
    wp.copy(this.pos[i] as Vector3)
      .applyQuaternion(pr)
      .add(this.worldPos[p] as Vector3);
  }

  /** Solves a bone and all its descendants (bones are ordered parents first). */
  solveSubtree(i: number): void {
    this.solveBone(i);
    // Leaves (most part bones: quills, a frill's spines) have nothing below them.
    if (!this.parent[i]) return;
    for (const j of this.descendants(i)) this.solveBone(j);
  }

  /** A bone's descendants in order, found once. */
  private descendants(i: number): Int32Array {
    let list = this.subtrees[i];
    if (!list) {
      const out: number[] = [];
      for (let j = i + 1; j < this.count; j++) {
        let p = this.parents[j] as number;
        while (p > i) p = this.parents[p] as number;
        if (p === i) out.push(j);
      }
      list = Int32Array.from(out);
      this.subtrees[i] = list;
    }
    return list;
  }
  private readonly subtrees: (Int32Array | undefined)[] = [];

  /** Bone direction in world space (its +Y axis). */
  direction(i: number, out = new Vector3()): Vector3 {
    return out.copy(Y).applyQuaternion(this.worldRot[i] as Quaternion);
  }

  /** World position of a bone's tail. */
  tail(i: number, out = new Vector3()): Vector3 {
    return this.direction(i, out)
      .multiplyScalar(this.lengths[i] as number)
      .add(this.worldPos[i] as Vector3);
  }

  /**
   * Turns bone `i` so it points along world direction `dir`, keeping its rest twist (swing
   * only). Its parent's world transform must be current; updates the bone's world transform.
   */
  aim(i: number, dir: Vector3): void {
    const p = this.parents[i] as number;
    const parentRot = p >= 0 ? (this.worldRot[p] as Quaternion) : IDENTITY;
    const restLocal = this.restRot[i] as Quaternion;
    // Rest direction and wanted direction, both in the parent's frame.
    const d0 = scratchA.copy(Y).applyQuaternion(restLocal);
    const d1 = scratchB.copy(dir).normalize().applyQuaternion(scratchQ.copy(parentRot).invert());
    swing.setFromUnitVectors(d0, d1);
    (this.rot[i] as Quaternion).multiplyQuaternions(swing, restLocal);
    this.solveBone(i);
  }
}

const IDENTITY = new Quaternion();
const scratchA = new Vector3();
const scratchB = new Vector3();
const scratchQ = new Quaternion();
const swing = new Quaternion();
