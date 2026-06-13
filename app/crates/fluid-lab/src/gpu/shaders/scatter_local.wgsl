// Fused P2G scatter with WORKGROUP-LOCAL pre-accumulation (SORTED path only).
//
// Identical math/result to scatter.wgsl: each particle scatters all three MAC
// face components (u, v, w) with trilinear (tent) weights into i32 fixed-point
// num/den buffers. The ONLY difference is HOW the integer contributions reach
// the global buffers: instead of one global atomicAdd per stencil tap (which
// serializes once particles are cell-sorted and hammer the same faces at once),
// each workgroup first accumulates its taps into a SHARED-MEMORY open-addressed
// hash table keyed by global face slot, then flushes each occupied slot to the
// global buffer with ONE global atomicAdd. With sorted input a workgroup's 64
// particles touch a small set of faces, so the table absorbs nearly all the
// contention and the global-atomic count drops ~20-30x.
//
// DETERMINISM: still pure i32 fixed-point through BOTH the shared accumulate and
// the global flush. Integer add is associative/commutative, so any grouping or
// flush order yields bit-identical num/den vs the per-tap global-atomic path.
// The shared table also uses atomicAdd; collisions just merge into the same slot.
// On the rare table-full case a tap falls back to a direct global atomicAdd —
// still the same integer add, same result.

struct Params {
    dims: vec4<u32>,   // nx, particle_count, pressure_iters, _
    geom: vec4<f32>,   // h, inv_h, dt, fixed_scale
    phys: vec4<f32>,
    origin: vec4<f32>,
    grav: vec4<f32>,
    spc:  vec4<f32>,
    cls:  vec4<f32>,
    gdim: vec4<u32>,   // nx, ny, nz, _
};
struct Particle { pos: vec4<f32>, vel: vec4<f32> };

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> particles: array<Particle>;
@group(0) @binding(2) var<storage, read_write> u_num: array<atomic<i32>>;
@group(0) @binding(3) var<storage, read_write> u_den: array<atomic<i32>>;
@group(0) @binding(4) var<storage, read_write> v_num: array<atomic<i32>>;
@group(0) @binding(5) var<storage, read_write> v_den: array<atomic<i32>>;
@group(0) @binding(6) var<storage, read_write> w_num: array<atomic<i32>>;
@group(0) @binding(7) var<storage, read_write> w_den: array<atomic<i32>>;

const PARTICLE_WG: u32 = 64u;

// Shared open-addressed hash table. CAP=1024 entries: 4 KB keys + 4 KB vals =
// 8 KB of workgroup shared memory, well under the 16 KB WebGPU floor. Key space
// packs the global slot as buffer_id * SLOT_STRIDE + face_idx. SLOT_STRIDE=2^22
// (4,194,304) covers the largest face buffer at grid 128 ((nx+1)*ny*nz =
// 129*128*128 = 2,113,536). 6 buffers * 2^22 = 25.2M < 2^31, so key+1 fits i32.
const CAP: u32 = 1024u;
const SLOT_BITS: u32 = 22u;
const SLOT_STRIDE: i32 = 1 << 22u;   // 4,194,304
const SLOT_MASK: i32 = (1 << 22u) - 1;

var<workgroup> sh_key: array<atomic<i32>, CAP>;   // stored as key+1; 0 = empty
var<workgroup> sh_val: array<atomic<i32>, CAP>;

fn particle_index(wid: vec3<u32>, lid: u32, nwg: vec3<u32>) -> u32 {
    return ((wid.y * nwg.x + wid.x) * PARTICLE_WG) + lid;
}

// Direct global atomicAdd into buffer `buf` (0=u_num,1=u_den,2=v_num,3=v_den,
// 4=w_num,5=w_den) at face index `idx`. Used as the table-full fallback.
fn global_add(buf: i32, idx: i32, add: i32) {
    switch (buf) {
        case 0: { atomicAdd(&u_num[idx], add); }
        case 1: { atomicAdd(&u_den[idx], add); }
        case 2: { atomicAdd(&v_num[idx], add); }
        case 3: { atomicAdd(&v_den[idx], add); }
        case 4: { atomicAdd(&w_num[idx], add); }
        default: { atomicAdd(&w_den[idx], add); }
    }
}

fn hash_key(key: i32) -> u32 {
    // Mix the packed slot key into a table index. Multiply-xorshift.
    var h = u32(key) * 2654435761u;
    h = h ^ (h >> 15u);
    return h & (CAP - 1u);   // CAP is a power of two
}

// Accumulate `add` into the shared table slot for (buf, idx); fall back to a
// global atomic if the table is full (every probe occupied by other keys).
fn local_add(buf: i32, idx: i32, add: i32) {
    let key = buf * SLOT_STRIDE + idx;   // packed global slot id
    let k1 = key + 1;                    // sentinel-shifted (0 = empty)
    var h = hash_key(key);
    // Linear probing, bounded to CAP steps.
    for (var step: u32 = 0u; step < CAP; step = step + 1u) {
        let cur = atomicLoad(&sh_key[h]);
        if (cur == k1) {
            atomicAdd(&sh_val[h], add);
            return;
        }
        if (cur == 0) {
            let res = atomicCompareExchangeWeak(&sh_key[h], 0, k1);
            if (res.exchanged || res.old_value == k1) {
                // We claimed the empty slot, or it was concurrently claimed by
                // our OWN key — either way accumulate here.
                atomicAdd(&sh_val[h], add);
                return;
            }
            // A weak-CAS spurious failure (res.old_value still 0) or another key
            // won the slot. Retry this same slot for a spurious fail; otherwise
            // probe onward. Cheapest correct handling: re-loop on this index when
            // it is still empty, else advance.
            if (res.old_value == 0) { continue; }
        }
        h = (h + 1u) & (CAP - 1u);
    }
    // Table full: direct global atomic (still the same integer add).
    global_add(buf, idx, add);
}

// Scatter one velocity component into one face buffer PAIR (num at `num_buf`,
// den at `num_buf+1`). `off` is the per-axis staggering, `dim` the face dims.
fn scatter_component(
    g: vec3<f32>,
    dim: vec3<i32>,
    vcomp: f32,
    scale: f32,
    num_buf: i32,
) {
    let base = vec3<i32>(floor(g));
    let t = g - vec3<f32>(base);

    for (var dk = 0; dk < 2; dk = dk + 1) {
        let kk = base.z + dk;
        if (kk < 0 || kk >= dim.z) { continue; }
        let wz = select(1.0 - t.z, t.z, dk == 1);
        for (var dj = 0; dj < 2; dj = dj + 1) {
            let jj = base.y + dj;
            if (jj < 0 || jj >= dim.y) { continue; }
            let wy = select(1.0 - t.y, t.y, dj == 1);
            for (var di = 0; di < 2; di = di + 1) {
                let ii = base.x + di;
                if (ii < 0 || ii >= dim.x) { continue; }
                let wx = select(1.0 - t.x, t.x, di == 1);

                let w = wx * wy * wz;
                if (w <= 0.0) { continue; }
                let idx = ii + dim.x * (jj + dim.y * kk);
                local_add(num_buf,     idx, i32(round(w * vcomp * scale)));
                local_add(num_buf + 1, idx, i32(round(w * scale)));
            }
        }
    }
}

@compute @workgroup_size(64, 1, 1)
fn main(
    @builtin(workgroup_id) wid: vec3<u32>,
    @builtin(local_invocation_index) lid: u32,
    @builtin(num_workgroups) nwg: vec3<u32>,
) {
    // Clear the shared table (CAP entries / PARTICLE_WG lanes each).
    for (var s = lid; s < CAP; s = s + PARTICLE_WG) {
        atomicStore(&sh_key[s], 0);
        atomicStore(&sh_val[s], 0);
    }
    workgroupBarrier();

    let p = particle_index(wid, lid, nwg);
    if (p < params.dims.y) {
        let nx = i32(params.gdim.x);
        let ny = i32(params.gdim.y);
        let nz = i32(params.gdim.z);
        let inv_h = params.geom.y;
        let scale = params.geom.w;

        let pos = (particles[p].pos.xyz - params.origin.xyz) * inv_h;
        let pv = particles[p].vel.xyz;

        // u: x on faces -> off=( 0.0,-0.5,-0.5), dim=(nx+1,ny,nz),   bufs 0/1
        scatter_component(pos + vec3<f32>(0.0, -0.5, -0.5),
            vec3<i32>(nx + 1, ny, nz), pv.x, scale, 0);
        // v: y on faces -> off=(-0.5, 0.0,-0.5), dim=(nx,ny+1,nz),   bufs 2/3
        scatter_component(pos + vec3<f32>(-0.5, 0.0, -0.5),
            vec3<i32>(nx, ny + 1, nz), pv.y, scale, 2);
        // w: z on faces -> off=(-0.5,-0.5, 0.0), dim=(nx,ny,nz+1),   bufs 4/5
        scatter_component(pos + vec3<f32>(-0.5, -0.5, 0.0),
            vec3<i32>(nx, ny, nz + 1), pv.z, scale, 4);
    }

    workgroupBarrier();

    // Flush: each lane drains a strided share of the table. One global atomic
    // per occupied slot. Decode the packed key back to (buffer, face_idx).
    for (var s = lid; s < CAP; s = s + PARTICLE_WG) {
        let k1 = atomicLoad(&sh_key[s]);
        if (k1 == 0) { continue; }
        let val = atomicLoad(&sh_val[s]);
        if (val == 0) { continue; }
        let key = k1 - 1;
        let buf = key >> SLOT_BITS;
        let idx = key & SLOT_MASK;
        global_add(buf, idx, val);
    }
}
