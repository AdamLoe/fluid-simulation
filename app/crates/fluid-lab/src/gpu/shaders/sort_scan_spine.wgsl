// Particle spatial sort — pass 2 of the exclusive prefix sum: scan the spine of
// per-block totals (written by sort_scan_block) IN PLACE into their exclusive
// prefix. Dispatched as ONE workgroup of 256 threads; chunks of 256 are scanned
// in shared memory with a running carry so any spine length is handled.
//
// num_blocks = ceil(cell_count / 256). cell_offset still holds the block-local
// exclusive prefixes; sort_scan_add adds scan_spine[block] back to finish.

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
@group(0) @binding(1) var<storage, read_write> scan_spine: array<u32>;

const BLOCK: u32 = 256u;
var<workgroup> sdata: array<u32, 256>;
var<workgroup> carry: u32;

@compute @workgroup_size(256)
fn main(@builtin(local_invocation_index) li: u32) {
    let cells = params.gdim.x * params.gdim.y * params.gdim.z;
    let num_blocks = (cells + BLOCK - 1u) / BLOCK;

    if (li == 0u) { carry = 0u; }
    workgroupBarrier();

    var chunk_start = 0u;
    loop {
        if (chunk_start >= num_blocks) { break; }
        let idx = chunk_start + li;
        let v = select(0u, scan_spine[idx], idx < num_blocks);
        sdata[li] = v;
        workgroupBarrier();

        // Blelloch exclusive scan over this 256-element chunk.
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
        var total = 0u;
        if (li == 0u) {
            total = sdata[BLOCK - 1u];
            sdata[BLOCK - 1u] = 0u;
        }
        workgroupBarrier();
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

        // Write exclusive prefix + running carry back into the spine.
        if (idx < num_blocks) {
            scan_spine[idx] = sdata[li] + carry;
        }
        workgroupBarrier();
        if (li == 0u) {
            carry = carry + total;
        }
        workgroupBarrier();
        chunk_start = chunk_start + BLOCK;
    }
}
