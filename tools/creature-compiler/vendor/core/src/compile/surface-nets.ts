import { Vector3 } from 'three';
import { SDF_BIG, type Sdf, SdfEvaluator } from './sdf.ts';

/**
 * Surface nets over a grid fitted to the creature, sampling only blocks near the surface and
 * only against primitives that can reach them. Then light smoothing, snapping back onto the
 * surface and normals from the field gradient.
 */
export interface Grid {
  readonly min: Vector3;
  readonly cell: number;
  readonly nx: number;
  readonly ny: number;
  readonly nz: number;
}

export interface SurfaceMesh {
  readonly positions: Float32Array;
  readonly normals: Float32Array;
  readonly indices: Uint32Array;
  /** Primitive lists per block, reused for later evaluations near the surface. */
  readonly culling: PrimCulling;
  readonly grid: Grid;
  readonly stats: {
    readonly samples: number;
    readonly blocks: number;
    readonly activeBlocks: number;
  };
}

const BLOCK = 8;

/** Primitives near each block of the grid. */
export class PrimCulling {
  readonly grid: Grid;
  readonly bx: number;
  readonly by: number;
  readonly bz: number;
  /** Start offset into `lists` per block, and the count. */
  readonly start: Int32Array;
  readonly count: Int32Array;
  readonly lists: Int32Array;

  constructor(sdf: Sdf, grid: Grid, margin: number) {
    this.grid = grid;
    this.bx = Math.ceil(grid.nx / BLOCK);
    this.by = Math.ceil(grid.ny / BLOCK);
    this.bz = Math.ceil(grid.nz / BLOCK);
    const blocks = this.bx * this.by * this.bz;
    this.start = new Int32Array(blocks);
    this.count = new Int32Array(blocks);
    const lists: number[] = [];
    const half = (BLOCK * grid.cell) / 2;
    const rb = half * Math.sqrt(3);
    for (let k = 0; k < this.bz; k++) {
      for (let j = 0; j < this.by; j++) {
        for (let i = 0; i < this.bx; i++) {
          const b = i + this.bx * (j + this.by * k);
          const cx = grid.min.x + i * BLOCK * grid.cell + half;
          const cy = grid.min.y + j * BLOCK * grid.cell + half;
          const cz = grid.min.z + k * BLOCK * grid.cell + half;
          this.start[b] = lists.length;
          for (let p = 0; p < sdf.count; p++) {
            const o = p * 4;
            const dx = cx - (sdf.bounds[o] as number);
            const dy = cy - (sdf.bounds[o + 1] as number);
            const dz = cz - (sdf.bounds[o + 2] as number);
            const reach = rb + (sdf.bounds[o + 3] as number) + (sdf.reach[p] as number) + margin;
            if (dx * dx + dy * dy + dz * dz < reach * reach) lists.push(p);
          }
          this.count[b] = lists.length - (this.start[b] as number);
        }
      }
    }
    this.lists = new Int32Array(lists);
  }

  /** Block index containing a point (clamped to the grid). */
  blockAt(x: number, y: number, z: number): number {
    const g = this.grid;
    const clampi = (v: number, n: number) => (v < 0 ? 0 : v >= n ? n - 1 : v);
    const i = clampi(Math.floor((x - g.min.x) / g.cell / BLOCK), this.bx);
    const j = clampi(Math.floor((y - g.min.y) / g.cell / BLOCK), this.by);
    const k = clampi(Math.floor((z - g.min.z) / g.cell / BLOCK), this.bz);
    return i + this.bx * (j + this.by * k);
  }

  /** The primitive list for a block, as a view into the shared array. */
  primsOf(block: number): Int32Array {
    const s = this.start[block] as number;
    return this.lists.subarray(s, s + (this.count[block] as number));
  }
}

/**
 * A grid with `cells` cells along the longest axis of the field's bounds. With `lattice`, the
 * grid's cell size and lattice come from that field instead (the same creature without its
 * muscle), extended by whole cells to cover this one, so what anatomy leaves alone (a head, a
 * mouth, a tail tip) is sampled exactly as it was.
 */
export function fitGrid(sdf: Sdf, cells: number, lattice: Sdf = sdf): Grid {
  const bounds = (field: Sdf) => {
    const min = new Vector3(Infinity, Infinity, Infinity);
    const max = new Vector3(-Infinity, -Infinity, -Infinity);
    for (let p = 0; p < field.count; p++) {
      const o = p * 4;
      const r = field.bounds[o + 3] as number;
      min.x = Math.min(min.x, (field.bounds[o] as number) - r);
      min.y = Math.min(min.y, (field.bounds[o + 1] as number) - r);
      min.z = Math.min(min.z, (field.bounds[o + 2] as number) - r);
      max.x = Math.max(max.x, (field.bounds[o] as number) + r);
      max.y = Math.max(max.y, (field.bounds[o + 1] as number) + r);
      max.z = Math.max(max.z, (field.bounds[o + 2] as number) + r);
    }
    return { min, max };
  };
  const { min, max } = bounds(lattice);
  const size = new Vector3().subVectors(max, min);
  const longest = Math.max(size.x, size.y, size.z);
  const cell = longest / cells;
  const pad = cell * 2;
  min.subScalar(pad);
  max.addScalar(pad);
  if (lattice !== sdf) {
    // Grow by whole cells, so the lattice stays where it was.
    const own = bounds(sdf);
    for (const axis of ['x', 'y', 'z'] as const) {
      const below = own.min[axis] - pad;
      if (below < min[axis]) min[axis] -= Math.ceil((min[axis] - below) / cell) * cell;
      max[axis] = Math.max(max[axis], own.max[axis] + pad);
    }
  }
  size.subVectors(max, min);
  return {
    min,
    cell,
    nx: Math.ceil(size.x / cell),
    ny: Math.ceil(size.y / cell),
    nz: Math.ceil(size.z / cell),
  };
}

export function surfaceNets(sdf: Sdf, grid: Grid, options: { smooth?: number } = {}): SurfaceMesh {
  const { nx, ny, nz, cell } = grid;
  const vx = nx + 1;
  const vy = ny + 1;
  const vz = nz + 1;
  const values = new Float32Array(vx * vy * vz).fill(SDF_BIG);
  const done = new Uint8Array(vx * vy * vz);
  const culling = new PrimCulling(sdf, grid, sdf.maxBlend + cell * 2);
  const evaluator = new SdfEvaluator(sdf);
  // Head details are left to the head's refinement: under a cell, they would only alias here.
  evaluator.details = false;
  const idx = (i: number, j: number, k: number) => i + vx * (j + vy * k);
  const rb = ((BLOCK * cell) / 2) * Math.sqrt(3);
  const subPrims = new Int32Array(sdf.count);

  let samples = 0;
  let activeBlocks = 0;
  for (let bk = 0; bk < culling.bz; bk++) {
    for (let bj = 0; bj < culling.by; bj++) {
      for (let bi = 0; bi < culling.bx; bi++) {
        const block = bi + culling.bx * (bj + culling.by * bk);
        const prims = culling.primsOf(block);
        if (prims.length === 0) continue;
        const i0 = bi * BLOCK;
        const j0 = bj * BLOCK;
        const k0 = bk * BLOCK;
        const cx = grid.min.x + (i0 + BLOCK / 2) * cell;
        const cy = grid.min.y + (j0 + BLOCK / 2) * cell;
        const cz = grid.min.z + (k0 + BLOCK / 2) * cell;
        const centre = evaluator.eval(cx, cy, cz, prims);
        samples++;
        const i1 = Math.min(i0 + BLOCK, nx);
        const j1 = Math.min(j0 + BLOCK, ny);
        const k1 = Math.min(k0 + BLOCK, nz);
        if (Math.abs(centre) > rb + cell * 1.5) {
          // Entirely inside or outside: record the sign only.
          const fill = centre > 0 ? SDF_BIG : -SDF_BIG;
          for (let k = k0; k <= k1; k++)
            for (let j = j0; j <= j1; j++)
              for (let i = i0; i <= i1; i++) {
                const v = idx(i, j, k);
                if (!done[v]) values[v] = fill;
              }
          continue;
        }
        activeBlocks++;
        // Second level: 4-cell sub-blocks, again sampling only those near the surface, each with
        // the block's primitives narrowed to those within reach of the sub-block.
        const SUB = BLOCK / 2;
        const rs = ((SUB * cell) / 2) * Math.sqrt(3);
        const margin = sdf.maxBlend + cell * 2;
        for (let sk = k0; sk < k1; sk += SUB) {
          for (let sj = j0; sj < j1; sj += SUB) {
            for (let si = i0; si < i1; si += SUB) {
              const ei = Math.min(si + SUB, nx);
              const ej = Math.min(sj + SUB, ny);
              const ek = Math.min(sk + SUB, nz);
              const scx = grid.min.x + (si + SUB / 2) * cell;
              const scy = grid.min.y + (sj + SUB / 2) * cell;
              const scz = grid.min.z + (sk + SUB / 2) * cell;
              let near = 0;
              for (let q = 0; q < prims.length; q++) {
                const p = prims[q] as number;
                const o = p * 4;
                const dx = scx - (sdf.bounds[o] as number);
                const dy = scy - (sdf.bounds[o + 1] as number);
                const dz = scz - (sdf.bounds[o + 2] as number);
                const reach = rs + (sdf.bounds[o + 3] as number) + margin;
                if (dx * dx + dy * dy + dz * dz < reach * reach) subPrims[near++] = p;
              }
              const sc = evaluator.eval(scx, scy, scz, subPrims, near);
              samples++;
              const far = Math.abs(sc) > rs + cell * 1.5;
              for (let k = sk; k <= ek; k++) {
                const z = grid.min.z + k * cell;
                for (let j = sj; j <= ej; j++) {
                  const y = grid.min.y + j * cell;
                  for (let i = si; i <= ei; i++) {
                    const v = idx(i, j, k);
                    if (done[v]) continue;
                    if (far) {
                      values[v] = sc > 0 ? SDF_BIG : -SDF_BIG;
                      continue;
                    }
                    values[v] = evaluator.eval(grid.min.x + i * cell, y, z, subPrims, near);
                    done[v] = 1;
                    samples++;
                  }
                }
              }
            }
          }
        }
      }
    }
  }

  // One vertex per cell with a sign change, at the mean of its edge crossings.
  const cellVertex = new Int32Array(nx * ny * nz).fill(-1);
  const cidx = (i: number, j: number, k: number) => i + nx * (j + ny * k);
  const positions: number[] = [];
  const corner = new Float64Array(8);
  const EDGES = [
    [0, 1],
    [2, 3],
    [4, 5],
    [6, 7],
    [0, 2],
    [1, 3],
    [4, 6],
    [5, 7],
    [0, 4],
    [1, 5],
    [2, 6],
    [3, 7],
  ] as const;
  for (let k = 0; k < nz; k++) {
    for (let j = 0; j < ny; j++) {
      for (let i = 0; i < nx; i++) {
        let mask = 0;
        for (let c = 0; c < 8; c++) {
          const val = values[idx(i + (c & 1), j + ((c >> 1) & 1), k + ((c >> 2) & 1))] as number;
          corner[c] = val;
          if (val < 0) mask |= 1 << c;
        }
        if (mask === 0 || mask === 255) continue;
        let sx = 0;
        let sy = 0;
        let sz = 0;
        let n = 0;
        for (const [a, b] of EDGES) {
          const va = corner[a] as number;
          const vb = corner[b] as number;
          if (va < 0 === vb < 0) continue;
          const t = va / (va - vb);
          const ax = a & 1;
          const ay = (a >> 1) & 1;
          const az = (a >> 2) & 1;
          const bx = b & 1;
          const by = (b >> 1) & 1;
          const bz = (b >> 2) & 1;
          sx += ax + (bx - ax) * t;
          sy += ay + (by - ay) * t;
          sz += az + (bz - az) * t;
          n++;
        }
        cellVertex[cidx(i, j, k)] = positions.length / 3;
        positions.push(
          grid.min.x + (i + sx / n) * cell,
          grid.min.y + (j + sy / n) * cell,
          grid.min.z + (k + sz / n) * cell,
        );
      }
    }
  }

  // A quad for every grid edge with a sign change, joining the four cells around it.
  const indices: number[] = [];
  const quad = (a: number, b: number, c: number, d: number, flip: boolean) => {
    if (a < 0 || b < 0 || c < 0 || d < 0) return;
    // Split along the shorter diagonal.
    const pa = a * 3;
    const pb = b * 3;
    const pc = c * 3;
    const pd = d * 3;
    const d1 = dist2(positions, pa, pc);
    const d2 = dist2(positions, pb, pd);
    let tris: number[];
    if (d1 <= d2) tris = [a, b, c, a, c, d];
    else tris = [a, b, d, b, c, d];
    if (flip)
      for (let t = 0; t < tris.length; t += 3)
        [tris[t + 1], tris[t + 2]] = [tris[t + 2] as number, tris[t + 1] as number];
    indices.push(...tris);
  };
  for (let k = 1; k < nz; k++) {
    for (let j = 1; j < ny; j++) {
      for (let i = 0; i < nx; i++) {
        // Edge along x from (i, j, k) to (i+1, j, k), shared by cells (i, j-1..j, k-1..k).
        const a = (values[idx(i, j, k)] as number) < 0;
        const b = (values[idx(i + 1, j, k)] as number) < 0;
        if (a === b) continue;
        quad(
          cellVertex[cidx(i, j - 1, k - 1)] as number,
          cellVertex[cidx(i, j, k - 1)] as number,
          cellVertex[cidx(i, j, k)] as number,
          cellVertex[cidx(i, j - 1, k)] as number,
          !a,
        );
      }
    }
  }
  for (let k = 1; k < nz; k++) {
    for (let j = 0; j < ny; j++) {
      for (let i = 1; i < nx; i++) {
        const a = (values[idx(i, j, k)] as number) < 0;
        const b = (values[idx(i, j + 1, k)] as number) < 0;
        if (a === b) continue;
        quad(
          cellVertex[cidx(i - 1, j, k - 1)] as number,
          cellVertex[cidx(i - 1, j, k)] as number,
          cellVertex[cidx(i, j, k)] as number,
          cellVertex[cidx(i, j, k - 1)] as number,
          !a,
        );
      }
    }
  }
  for (let k = 0; k < nz; k++) {
    for (let j = 1; j < ny; j++) {
      for (let i = 1; i < nx; i++) {
        const a = (values[idx(i, j, k)] as number) < 0;
        const b = (values[idx(i, j, k + 1)] as number) < 0;
        if (a === b) continue;
        quad(
          cellVertex[cidx(i - 1, j - 1, k)] as number,
          cellVertex[cidx(i, j - 1, k)] as number,
          cellVertex[cidx(i, j, k)] as number,
          cellVertex[cidx(i - 1, j, k)] as number,
          !a,
        );
      }
    }
  }

  const pos = new Float32Array(positions);
  const tri = new Uint32Array(indices);
  relax(pos, tri, options.smooth ?? 1);
  const normals = new Float32Array(pos.length);
  const g = new Vector3();
  for (let v = 0; v < pos.length; v += 3) {
    let x = pos[v] as number;
    let y = pos[v + 1] as number;
    let z = pos[v + 2] as number;
    const prims = culling.primsOf(culling.blockAt(x, y, z));
    // One Newton step back onto the surface; the gradient that steers it is also the normal
    // (the step is a fraction of a cell, too short for the normal to change visibly).
    const d = evaluator.valueAndGradient(x, y, z, cell * 0.2, prims, g);
    x -= d * g.x;
    y -= d * g.y;
    z -= d * g.z;
    pos[v] = x;
    pos[v + 1] = y;
    pos[v + 2] = z;
    normals[v] = g.x;
    normals[v + 1] = g.y;
    normals[v + 2] = g.z;
  }
  return {
    positions: pos,
    normals,
    indices: tri,
    culling,
    grid,
    stats: { samples, blocks: culling.bx * culling.by * culling.bz, activeBlocks },
  };
}

function dist2(p: number[], a: number, b: number): number {
  const dx = (p[a] as number) - (p[b] as number);
  const dy = (p[a + 1] as number) - (p[b + 1] as number);
  const dz = (p[a + 2] as number) - (p[b + 2] as number);
  return dx * dx + dy * dy + dz * dz;
}

/** Laplacian smoothing toward the neighbour average, `iterations` times. */
function relax(pos: Float32Array, tri: Uint32Array, iterations: number): void {
  if (iterations <= 0) return;
  const n = pos.length / 3;
  const sum = new Float64Array(n * 3);
  const count = new Uint32Array(n);
  for (let it = 0; it < iterations; it++) {
    sum.fill(0);
    count.fill(0);
    for (let t = 0; t < tri.length; t += 3) {
      for (let e = 0; e < 3; e++) {
        const a = tri[t + e] as number;
        const b = tri[t + ((e + 1) % 3)] as number;
        for (let c = 0; c < 3; c++) {
          sum[a * 3 + c] = (sum[a * 3 + c] as number) + (pos[b * 3 + c] as number);
          sum[b * 3 + c] = (sum[b * 3 + c] as number) + (pos[a * 3 + c] as number);
        }
        count[a] = (count[a] as number) + 1;
        count[b] = (count[b] as number) + 1;
      }
    }
    for (let v = 0; v < n; v++) {
      const c = count[v] as number;
      if (c === 0) continue;
      for (let a = 0; a < 3; a++) {
        const i = v * 3 + a;
        pos[i] = (pos[i] as number) * 0.5 + ((sum[i] as number) / c) * 0.5;
      }
    }
  }
}
