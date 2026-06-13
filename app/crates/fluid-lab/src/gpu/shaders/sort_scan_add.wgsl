// Particle spatial sort — pass 3 of the exclusive prefix sum: add each block's
// scanned spine offset back into the block-local exclusive prefixes so cell_offset
// holds the GLOBAL exclusive prefix sum of the occupancy histogram. After this,
// cell_offset[c] = number of particles in all cells with index < c, i.e. the start
// of cell c's bucket in the sorted particle array.

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
@group(0) @binding(1) var<storage, read_write> cell_offset: array<u32>;
@group(0) @binding(2) var<storage, read> scan_spine: array<u32>;

const BLOCK: u32 = 256u;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let cells = params.gdim.x * params.gdim.y * params.gdim.z;
    let c = gid.x;
    if (c >= cells) { return; }
    let block = c / BLOCK;
    cell_offset[c] = cell_offset[c] + scan_spine[block];
}
