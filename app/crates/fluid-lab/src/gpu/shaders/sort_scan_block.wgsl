// Particle spatial sort — pass 1 of the exclusive prefix sum over the per-cell
// occupancy histogram. Each workgroup of 256 threads exclusive-scans one 256-cell
// block of `occ` into `cell_offset` (block-local exclusive prefix) and writes the
// block total into `scan_spine[workgroup_id.x]`. A later spine scan + add fixup
// turns these block-local prefixes into the global exclusive prefix.
//
// `occ` is the SAME occupancy buffer mark.wgsl filled and classify.wgsl reads, so
// the scan must NOT mutate it — output goes to the separate `cell_offset` buffer.
// Integer scan only (determinism: the sort is a pure permutation).

struct Params {
    dims: vec4<u32>,
    geom: vec4<f32>,
    phys: vec4<f32>,
    origin: vec4<f32>,
    grav: vec4<f32>,
    spc:  vec4<f32>,
    cls:  vec4<f32>,
    gdim: vec4<u32>,   // nx, ny, nz, _
};
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> occ: array<u32>;
@group(0) @binding(2) var<storage, read_write> cell_offset: array<u32>;
@group(0) @binding(3) var<storage, read_write> scan_spine: array<u32>;

const BLOCK: u32 = 256u;
var<workgroup> sdata: array<u32, 256>;

@compute @workgroup_size(256)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_index) li: u32,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let cells = params.gdim.x * params.gdim.y * params.gdim.z;
    let idx = gid.x;
    let v = select(0u, occ[idx], idx < cells);
    sdata[li] = v;
    workgroupBarrier();

    // Work-efficient (Blelloch) scan over the 256-element block.
    // Up-sweep (reduce).
    var offset = 1u;
    var d = BLOCK >> 1u;
    loop {
        if (d == 0u) { break; }
        if (li < d) {
            let ai = offset * (2u * li + 1u) - 1u;
            let bi = offset * (2u * li + 2u) - 1u;
            sdata[bi] = sdata[bi] + sdata[ai];
        }
        offset = offset * 2u;
        workgroupBarrier();
        d = d >> 1u;
    }

    // Clear the last element (the block total) and stash it for the spine.
    if (li == 0u) {
        scan_spine[wid.x] = sdata[BLOCK - 1u];
        sdata[BLOCK - 1u] = 0u;
    }
    workgroupBarrier();

    // Down-sweep.
    d = 1u;
    loop {
        if (d >= BLOCK) { break; }
        offset = offset >> 1u;
        if (li < d) {
            let ai = offset * (2u * li + 1u) - 1u;
            let bi = offset * (2u * li + 2u) - 1u;
            let t = sdata[ai];
            sdata[ai] = sdata[bi];
            sdata[bi] = sdata[bi] + t;
        }
        workgroupBarrier();
        d = d * 2u;
    }

    // sdata[li] now holds the block-local EXCLUSIVE prefix sum.
    if (idx < cells) {
        cell_offset[idx] = sdata[li];
    }
}
