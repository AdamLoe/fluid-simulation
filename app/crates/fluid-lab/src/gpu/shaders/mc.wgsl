// Gate 3 + 4: Marching cubes surface extraction + per-vertex normals.
//
// One invocation per cube. Cube (i,j,k) for i in 0..nx-1, j in 0..ny-1,
// k in 0..nz-1, so there are (nx-1)(ny-1)(nz-1) cubes total.
//
// Corner ordering (standard Lorensen-Cline / Bourke):
//   0:(0,0,0) 1:(1,0,0) 2:(1,1,0) 3:(0,1,0)
//   4:(0,0,1) 5:(1,0,1) 6:(1,1,1) 7:(0,1,1)
// Edges:
//   0:0-1 1:1-2 2:2-3 3:3-0 4:4-5 5:5-6 6:6-7 7:7-4 8:0-4 9:1-5 10:2-6 11:3-7
//
// INSIDE CONVENTION: fluid is HIGH density. Corner c is INSIDE when scalar >= isolevel.
// (Opposite of an SDF; same tables, different bit-set comparison.)
//
// Output: interleaved vertices in `verts` buffer:
//   stride = 8 floats = vec4(pos.xyz, 0) ++ vec4(normal.xyz, 0)
// Counter in `counter[0]` is incremented atomically (3 per triangle).
// MAX_VERTS cap: if base+3 > MAX_VERTS we skip the triangle entirely.

struct MeshParams {
    isolevel: f32,
    h: f32,
    foam_scale: f32,   // speed (u/s) mapped to full foam
    _pad1: f32,
    dims: vec4<u32>,   // nx, ny, nz, _
    origin: vec4<f32>,
};

struct Vertex {
    pos: vec4<f32>,
    nrm: vec4<f32>,    // xyz = normal, w = foam factor (0..1)
};

@group(0) @binding(0) var<uniform> mesh_params: MeshParams;
@group(0) @binding(1) var<storage, read> scalar: array<f32>;
@group(0) @binding(2) var<storage, read_write> verts: array<Vertex>;
@group(0) @binding(3) var<storage, read_write> counter: array<atomic<u32>>;
@group(0) @binding(4) var<storage, read> speed: array<f32>;

// Foam factor at a world position: cell-centered speed normalized by foam_scale.
// Matches the particle speed→white cue so the mesh whitens where the water moves.
fn foam_at(nx: u32, ny: u32, nz: u32, p: vec3<f32>) -> f32 {
    let h = mesh_params.h;
    let origin = mesh_params.origin.xyz;
    let g = (p - origin) / h;
    let ci = u32(clamp(round(g.x - 0.5), 0.0, f32(nx - 1u)));
    let cj = u32(clamp(round(g.y - 0.5), 0.0, f32(ny - 1u)));
    let ck = u32(clamp(round(g.z - 0.5), 0.0, f32(nz - 1u)));
    let s = speed[ci + nx * (cj + ny * ck)];
    let scale = max(mesh_params.foam_scale, 1e-4);
    return clamp(s / scale, 0.0, 1.0);
}

// MAX_VERTS = MAX_TRIS * 3 = 800_000 * 3 = 2_400_000
const MAX_VERTS: u32 = 2400000u;

// ── EDGE_TABLE (256 × u16, stored as u32) ─────────────────────────────────────
// Bitmask of which of the 12 edges are crossed for each of the 256 corner-sign cases.
fn edge_table(ci: u32) -> u32 {
    var t = array<u32, 256>(
        0x0u, 0x109u, 0x203u, 0x30au, 0x406u, 0x50fu, 0x605u, 0x70cu,
        0x80cu, 0x905u, 0xa0fu, 0xb06u, 0xc0au, 0xd03u, 0xe09u, 0xf00u,
        0x190u, 0x99u, 0x393u, 0x29au, 0x596u, 0x49fu, 0x795u, 0x69cu,
        0x99cu, 0x895u, 0xb9fu, 0xa96u, 0xd9au, 0xc93u, 0xf99u, 0xe90u,
        0x230u, 0x339u, 0x33u, 0x13au, 0x636u, 0x73fu, 0x435u, 0x53cu,
        0xa3cu, 0xb35u, 0x83fu, 0x936u, 0xe3au, 0xf33u, 0xc39u, 0xd30u,
        0x3a0u, 0x2a9u, 0x1a3u, 0xaau, 0x7a6u, 0x6afu, 0x5a5u, 0x4acu,
        0xbacu, 0xaa5u, 0x9afu, 0x8a6u, 0xfaau, 0xea3u, 0xda9u, 0xca0u,
        0x460u, 0x569u, 0x663u, 0x76au, 0x66u, 0x16fu, 0x265u, 0x36cu,
        0xc6cu, 0xd65u, 0xe6fu, 0xf66u, 0x86au, 0x963u, 0xa69u, 0xb60u,
        0x5f0u, 0x4f9u, 0x7f3u, 0x6fau, 0x1f6u, 0xffu, 0x3f5u, 0x2fcu,
        0xdfcu, 0xcf5u, 0xfffu, 0xef6u, 0x9fau, 0x8f3u, 0xbf9u, 0xaf0u,
        0x650u, 0x759u, 0x453u, 0x55au, 0x256u, 0x35fu, 0x55u, 0x15cu,
        0xe5cu, 0xf55u, 0xc5fu, 0xd56u, 0xa5au, 0xb53u, 0x859u, 0x950u,
        0x7c0u, 0x6c9u, 0x5c3u, 0x4cau, 0x3c6u, 0x2cfu, 0x1c5u, 0xccu,
        0xfccu, 0xec5u, 0xdcfu, 0xcc6u, 0xbcau, 0xac3u, 0x9c9u, 0x8c0u,
        0x8c0u, 0x9c9u, 0xac3u, 0xbcau, 0xcc6u, 0xdcfu, 0xec5u, 0xfccu,
        0xccu, 0x1c5u, 0x2cfu, 0x3c6u, 0x4cau, 0x5c3u, 0x6c9u, 0x7c0u,
        0x950u, 0x859u, 0xb53u, 0xa5au, 0xd56u, 0xc5fu, 0xf55u, 0xe5cu,
        0x15cu, 0x55u, 0x35fu, 0x256u, 0x55au, 0x453u, 0x759u, 0x650u,
        0xaf0u, 0xbf9u, 0x8f3u, 0x9fau, 0xef6u, 0xfffu, 0xcf5u, 0xdfcu,
        0x2fcu, 0x3f5u, 0xffu, 0x1f6u, 0x6fau, 0x7f3u, 0x4f9u, 0x5f0u,
        0xb60u, 0xa69u, 0x963u, 0x86au, 0xf66u, 0xe6fu, 0xd65u, 0xc6cu,
        0x36cu, 0x265u, 0x16fu, 0x66u, 0x76au, 0x663u, 0x569u, 0x460u,
        0xca0u, 0xda9u, 0xea3u, 0xfaau, 0x8a6u, 0x9afu, 0xaa5u, 0xbacu,
        0x4acu, 0x5a5u, 0x6afu, 0x7a6u, 0xaau, 0x1a3u, 0x2a9u, 0x3a0u,
        0xd30u, 0xc39u, 0xf33u, 0xe3au, 0x936u, 0x83fu, 0xb35u, 0xa3cu,
        0x53cu, 0x435u, 0x73fu, 0x636u, 0x13au, 0x33u, 0x339u, 0x230u,
        0xe90u, 0xf99u, 0xc93u, 0xd9au, 0xa96u, 0xb9fu, 0x895u, 0x99cu,
        0x69cu, 0x795u, 0x49fu, 0x596u, 0x29au, 0x393u, 0x99u, 0x190u,
        0xf00u, 0xe09u, 0xd03u, 0xc0au, 0xb06u, 0xa0fu, 0x905u, 0x80cu,
        0x70cu, 0x605u, 0x50fu, 0x406u, 0x30au, 0x203u, 0x109u, 0x0u
    );
    return t[ci];
}

// ── TRI_TABLE (256×16 i8, stored as i32) ───────────────────────────────────────
// Up to 5 triangles (15 edge indices) per case, -1 terminated, padded to 16.
// Returns the j-th entry (0..16) for a given case index.
fn tri_table(ci: u32, j: u32) -> i32 {
    // Encode as flat array of 256*16 = 4096 entries.
    // We pack each row of 16 into a let statement and index dynamically.
    // WGSL doesn't allow truly dynamic array indexing from a const array of arrays,
    // so we use a flat array approach.
    var row = array<i32, 16>(-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1);
    switch (ci) {
        case 0u: { row = array<i32,16>(-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 1u: { row = array<i32,16>(0,8,3,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 2u: { row = array<i32,16>(0,1,9,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 3u: { row = array<i32,16>(1,8,3,9,8,1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 4u: { row = array<i32,16>(1,2,10,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 5u: { row = array<i32,16>(0,8,3,1,2,10,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 6u: { row = array<i32,16>(9,2,10,0,2,9,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 7u: { row = array<i32,16>(2,8,3,2,10,8,10,9,8,-1,-1,-1,-1,-1,-1,-1); }
        case 8u: { row = array<i32,16>(3,11,2,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 9u: { row = array<i32,16>(0,11,2,8,11,0,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 10u: { row = array<i32,16>(1,9,0,2,3,11,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 11u: { row = array<i32,16>(1,11,2,1,9,11,9,8,11,-1,-1,-1,-1,-1,-1,-1); }
        case 12u: { row = array<i32,16>(3,10,1,11,10,3,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 13u: { row = array<i32,16>(0,10,1,0,8,10,8,11,10,-1,-1,-1,-1,-1,-1,-1); }
        case 14u: { row = array<i32,16>(3,9,0,3,11,9,11,10,9,-1,-1,-1,-1,-1,-1,-1); }
        case 15u: { row = array<i32,16>(9,8,10,10,8,11,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 16u: { row = array<i32,16>(4,7,8,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 17u: { row = array<i32,16>(4,3,0,7,3,4,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 18u: { row = array<i32,16>(0,1,9,8,4,7,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 19u: { row = array<i32,16>(4,1,9,4,7,1,7,3,1,-1,-1,-1,-1,-1,-1,-1); }
        case 20u: { row = array<i32,16>(1,2,10,8,4,7,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 21u: { row = array<i32,16>(3,4,7,3,0,4,1,2,10,-1,-1,-1,-1,-1,-1,-1); }
        case 22u: { row = array<i32,16>(9,2,10,9,0,2,8,4,7,-1,-1,-1,-1,-1,-1,-1); }
        case 23u: { row = array<i32,16>(2,10,9,2,9,7,2,7,3,7,9,4,-1,-1,-1,-1); }
        case 24u: { row = array<i32,16>(8,4,7,3,11,2,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 25u: { row = array<i32,16>(11,4,7,11,2,4,2,0,4,-1,-1,-1,-1,-1,-1,-1); }
        case 26u: { row = array<i32,16>(9,0,1,8,4,7,2,3,11,-1,-1,-1,-1,-1,-1,-1); }
        case 27u: { row = array<i32,16>(4,7,11,9,4,11,9,11,2,9,2,1,-1,-1,-1,-1); }
        case 28u: { row = array<i32,16>(3,10,1,3,11,10,7,8,4,-1,-1,-1,-1,-1,-1,-1); }
        case 29u: { row = array<i32,16>(1,11,10,1,4,11,1,0,4,7,11,4,-1,-1,-1,-1); }
        case 30u: { row = array<i32,16>(4,7,8,9,0,11,9,11,10,11,0,3,-1,-1,-1,-1); }
        case 31u: { row = array<i32,16>(4,7,11,4,11,9,9,11,10,-1,-1,-1,-1,-1,-1,-1); }
        case 32u: { row = array<i32,16>(9,5,4,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 33u: { row = array<i32,16>(9,5,4,0,8,3,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 34u: { row = array<i32,16>(0,5,4,1,5,0,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 35u: { row = array<i32,16>(8,5,4,8,3,5,3,1,5,-1,-1,-1,-1,-1,-1,-1); }
        case 36u: { row = array<i32,16>(1,2,10,9,5,4,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 37u: { row = array<i32,16>(3,0,8,1,2,10,4,9,5,-1,-1,-1,-1,-1,-1,-1); }
        case 38u: { row = array<i32,16>(5,2,10,5,4,2,4,0,2,-1,-1,-1,-1,-1,-1,-1); }
        case 39u: { row = array<i32,16>(2,10,5,3,2,5,3,5,4,3,4,8,-1,-1,-1,-1); }
        case 40u: { row = array<i32,16>(9,5,4,2,3,11,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 41u: { row = array<i32,16>(0,11,2,0,8,11,4,9,5,-1,-1,-1,-1,-1,-1,-1); }
        case 42u: { row = array<i32,16>(0,5,4,0,1,5,2,3,11,-1,-1,-1,-1,-1,-1,-1); }
        case 43u: { row = array<i32,16>(2,1,5,2,5,8,2,8,11,4,8,5,-1,-1,-1,-1); }
        case 44u: { row = array<i32,16>(10,3,11,10,1,3,9,5,4,-1,-1,-1,-1,-1,-1,-1); }
        case 45u: { row = array<i32,16>(4,9,5,0,8,1,8,10,1,8,11,10,-1,-1,-1,-1); }
        case 46u: { row = array<i32,16>(5,4,0,5,0,11,5,11,10,11,0,3,-1,-1,-1,-1); }
        case 47u: { row = array<i32,16>(5,4,8,5,8,10,10,8,11,-1,-1,-1,-1,-1,-1,-1); }
        case 48u: { row = array<i32,16>(9,7,8,5,7,9,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 49u: { row = array<i32,16>(9,3,0,9,5,3,5,7,3,-1,-1,-1,-1,-1,-1,-1); }
        case 50u: { row = array<i32,16>(0,7,8,0,1,7,1,5,7,-1,-1,-1,-1,-1,-1,-1); }
        case 51u: { row = array<i32,16>(1,5,3,3,5,7,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 52u: { row = array<i32,16>(9,7,8,9,5,7,10,1,2,-1,-1,-1,-1,-1,-1,-1); }
        case 53u: { row = array<i32,16>(10,1,2,9,5,0,5,3,0,5,7,3,-1,-1,-1,-1); }
        case 54u: { row = array<i32,16>(8,0,2,8,2,5,8,5,7,10,5,2,-1,-1,-1,-1); }
        case 55u: { row = array<i32,16>(2,10,5,2,5,3,3,5,7,-1,-1,-1,-1,-1,-1,-1); }
        case 56u: { row = array<i32,16>(7,9,5,7,8,9,3,11,2,-1,-1,-1,-1,-1,-1,-1); }
        case 57u: { row = array<i32,16>(9,5,7,9,7,2,9,2,0,2,7,11,-1,-1,-1,-1); }
        case 58u: { row = array<i32,16>(2,3,11,0,1,8,1,7,8,1,5,7,-1,-1,-1,-1); }
        case 59u: { row = array<i32,16>(11,2,1,11,1,7,7,1,5,-1,-1,-1,-1,-1,-1,-1); }
        case 60u: { row = array<i32,16>(9,5,8,8,5,7,10,1,3,10,3,11,-1,-1,-1,-1); }
        case 61u: { row = array<i32,16>(5,7,0,5,0,9,7,11,0,1,0,10,11,10,0,-1); }
        case 62u: { row = array<i32,16>(11,10,0,11,0,3,10,5,0,8,0,7,5,7,0,-1); }
        case 63u: { row = array<i32,16>(11,10,5,7,11,5,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 64u: { row = array<i32,16>(10,6,5,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 65u: { row = array<i32,16>(0,8,3,5,10,6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 66u: { row = array<i32,16>(9,0,1,5,10,6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 67u: { row = array<i32,16>(1,8,3,1,9,8,5,10,6,-1,-1,-1,-1,-1,-1,-1); }
        case 68u: { row = array<i32,16>(1,6,5,2,6,1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 69u: { row = array<i32,16>(1,6,5,1,2,6,3,0,8,-1,-1,-1,-1,-1,-1,-1); }
        case 70u: { row = array<i32,16>(9,6,5,9,0,6,0,2,6,-1,-1,-1,-1,-1,-1,-1); }
        case 71u: { row = array<i32,16>(5,9,8,5,8,2,5,2,6,3,2,8,-1,-1,-1,-1); }
        case 72u: { row = array<i32,16>(2,3,11,10,6,5,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 73u: { row = array<i32,16>(11,0,8,11,2,0,10,6,5,-1,-1,-1,-1,-1,-1,-1); }
        case 74u: { row = array<i32,16>(0,1,9,2,3,11,5,10,6,-1,-1,-1,-1,-1,-1,-1); }
        case 75u: { row = array<i32,16>(5,10,6,1,9,2,9,11,2,9,8,11,-1,-1,-1,-1); }
        case 76u: { row = array<i32,16>(6,3,11,6,5,3,5,1,3,-1,-1,-1,-1,-1,-1,-1); }
        case 77u: { row = array<i32,16>(0,8,11,0,11,5,0,5,1,5,11,6,-1,-1,-1,-1); }
        case 78u: { row = array<i32,16>(3,11,6,0,3,6,0,6,5,0,5,9,-1,-1,-1,-1); }
        case 79u: { row = array<i32,16>(6,5,9,6,9,11,11,9,8,-1,-1,-1,-1,-1,-1,-1); }
        case 80u: { row = array<i32,16>(5,10,6,4,7,8,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 81u: { row = array<i32,16>(4,3,0,4,7,3,6,5,10,-1,-1,-1,-1,-1,-1,-1); }
        case 82u: { row = array<i32,16>(1,9,0,5,10,6,8,4,7,-1,-1,-1,-1,-1,-1,-1); }
        case 83u: { row = array<i32,16>(10,6,5,1,9,7,1,7,3,7,9,4,-1,-1,-1,-1); }
        case 84u: { row = array<i32,16>(6,1,2,6,5,1,4,7,8,-1,-1,-1,-1,-1,-1,-1); }
        case 85u: { row = array<i32,16>(1,2,5,5,2,6,3,0,4,3,4,7,-1,-1,-1,-1); }
        case 86u: { row = array<i32,16>(8,4,7,9,0,5,0,6,5,0,2,6,-1,-1,-1,-1); }
        case 87u: { row = array<i32,16>(7,3,9,7,9,4,3,2,9,5,9,6,2,6,9,-1); }
        case 88u: { row = array<i32,16>(3,11,2,7,8,4,10,6,5,-1,-1,-1,-1,-1,-1,-1); }
        case 89u: { row = array<i32,16>(5,10,6,4,7,2,4,2,0,2,7,11,-1,-1,-1,-1); }
        case 90u: { row = array<i32,16>(0,1,9,4,7,8,2,3,11,5,10,6,-1,-1,-1,-1); }
        case 91u: { row = array<i32,16>(9,2,1,9,11,2,9,4,11,7,11,4,5,10,6,-1); }
        case 92u: { row = array<i32,16>(8,4,7,3,11,5,3,5,1,5,11,6,-1,-1,-1,-1); }
        case 93u: { row = array<i32,16>(5,1,11,5,11,6,1,0,11,7,11,4,0,4,11,-1); }
        case 94u: { row = array<i32,16>(0,5,9,0,6,5,0,3,6,11,6,3,8,4,7,-1); }
        case 95u: { row = array<i32,16>(6,5,9,6,9,11,4,7,9,7,11,9,-1,-1,-1,-1); }
        case 96u: { row = array<i32,16>(10,4,9,6,4,10,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 97u: { row = array<i32,16>(4,10,6,4,9,10,0,8,3,-1,-1,-1,-1,-1,-1,-1); }
        case 98u: { row = array<i32,16>(10,0,1,10,6,0,6,4,0,-1,-1,-1,-1,-1,-1,-1); }
        case 99u: { row = array<i32,16>(8,3,1,8,1,6,8,6,4,6,1,10,-1,-1,-1,-1); }
        case 100u: { row = array<i32,16>(1,4,9,1,2,4,2,6,4,-1,-1,-1,-1,-1,-1,-1); }
        case 101u: { row = array<i32,16>(3,0,8,1,2,9,2,4,9,2,6,4,-1,-1,-1,-1); }
        case 102u: { row = array<i32,16>(0,2,4,4,2,6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 103u: { row = array<i32,16>(8,3,2,8,2,4,4,2,6,-1,-1,-1,-1,-1,-1,-1); }
        case 104u: { row = array<i32,16>(10,4,9,10,6,4,11,2,3,-1,-1,-1,-1,-1,-1,-1); }
        case 105u: { row = array<i32,16>(0,8,2,2,8,11,4,9,10,4,10,6,-1,-1,-1,-1); }
        case 106u: { row = array<i32,16>(3,11,2,0,1,6,0,6,4,6,1,10,-1,-1,-1,-1); }
        case 107u: { row = array<i32,16>(6,4,1,6,1,10,4,8,1,2,1,11,8,11,1,-1); }
        case 108u: { row = array<i32,16>(9,6,4,9,3,6,9,1,3,11,6,3,-1,-1,-1,-1); }
        case 109u: { row = array<i32,16>(8,11,1,8,1,0,11,6,1,9,1,4,6,4,1,-1); }
        case 110u: { row = array<i32,16>(3,11,6,3,6,0,0,6,4,-1,-1,-1,-1,-1,-1,-1); }
        case 111u: { row = array<i32,16>(6,4,8,11,6,8,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 112u: { row = array<i32,16>(7,10,6,7,8,10,8,9,10,-1,-1,-1,-1,-1,-1,-1); }
        case 113u: { row = array<i32,16>(0,7,3,0,10,7,0,9,10,6,7,10,-1,-1,-1,-1); }
        case 114u: { row = array<i32,16>(10,6,7,1,10,7,1,7,8,1,8,0,-1,-1,-1,-1); }
        case 115u: { row = array<i32,16>(10,6,7,10,7,1,1,7,3,-1,-1,-1,-1,-1,-1,-1); }
        case 116u: { row = array<i32,16>(1,2,6,1,6,8,1,8,9,8,6,7,-1,-1,-1,-1); }
        case 117u: { row = array<i32,16>(2,6,9,2,9,1,6,7,9,0,9,3,7,3,9,-1); }
        case 118u: { row = array<i32,16>(7,8,0,7,0,6,6,0,2,-1,-1,-1,-1,-1,-1,-1); }
        case 119u: { row = array<i32,16>(7,3,2,6,7,2,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 120u: { row = array<i32,16>(2,3,11,10,6,8,10,8,9,8,6,7,-1,-1,-1,-1); }
        case 121u: { row = array<i32,16>(2,0,7,2,7,11,0,9,7,6,7,10,9,10,7,-1); }
        case 122u: { row = array<i32,16>(1,8,0,1,7,8,1,10,7,6,7,10,2,3,11,-1); }
        case 123u: { row = array<i32,16>(11,2,1,11,1,7,10,6,1,6,7,1,-1,-1,-1,-1); }
        case 124u: { row = array<i32,16>(8,9,6,8,6,7,9,1,6,11,6,3,1,3,6,-1); }
        case 125u: { row = array<i32,16>(0,9,1,11,6,7,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 126u: { row = array<i32,16>(7,8,0,7,0,6,3,11,0,11,6,0,-1,-1,-1,-1); }
        case 127u: { row = array<i32,16>(7,11,6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 128u: { row = array<i32,16>(7,6,11,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 129u: { row = array<i32,16>(3,0,8,11,7,6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 130u: { row = array<i32,16>(0,1,9,11,7,6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 131u: { row = array<i32,16>(8,1,9,8,3,1,11,7,6,-1,-1,-1,-1,-1,-1,-1); }
        case 132u: { row = array<i32,16>(10,1,2,6,11,7,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 133u: { row = array<i32,16>(1,2,10,3,0,8,6,11,7,-1,-1,-1,-1,-1,-1,-1); }
        case 134u: { row = array<i32,16>(2,9,0,2,10,9,6,11,7,-1,-1,-1,-1,-1,-1,-1); }
        case 135u: { row = array<i32,16>(6,11,7,2,10,3,10,8,3,10,9,8,-1,-1,-1,-1); }
        case 136u: { row = array<i32,16>(7,2,3,6,2,7,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 137u: { row = array<i32,16>(7,0,8,7,6,0,6,2,0,-1,-1,-1,-1,-1,-1,-1); }
        case 138u: { row = array<i32,16>(2,7,6,2,3,7,0,1,9,-1,-1,-1,-1,-1,-1,-1); }
        case 139u: { row = array<i32,16>(1,6,2,1,8,6,1,9,8,8,7,6,-1,-1,-1,-1); }
        case 140u: { row = array<i32,16>(10,7,6,10,1,7,1,3,7,-1,-1,-1,-1,-1,-1,-1); }
        case 141u: { row = array<i32,16>(10,7,6,1,7,10,1,8,7,1,0,8,-1,-1,-1,-1); }
        case 142u: { row = array<i32,16>(0,3,7,0,7,10,0,10,9,6,10,7,-1,-1,-1,-1); }
        case 143u: { row = array<i32,16>(7,6,10,7,10,8,8,10,9,-1,-1,-1,-1,-1,-1,-1); }
        case 144u: { row = array<i32,16>(6,8,4,11,8,6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 145u: { row = array<i32,16>(3,6,11,3,0,6,0,4,6,-1,-1,-1,-1,-1,-1,-1); }
        case 146u: { row = array<i32,16>(8,6,11,8,4,6,9,0,1,-1,-1,-1,-1,-1,-1,-1); }
        case 147u: { row = array<i32,16>(9,4,6,9,6,3,9,3,1,11,3,6,-1,-1,-1,-1); }
        case 148u: { row = array<i32,16>(6,8,4,6,11,8,2,10,1,-1,-1,-1,-1,-1,-1,-1); }
        case 149u: { row = array<i32,16>(1,2,10,3,0,11,0,6,11,0,4,6,-1,-1,-1,-1); }
        case 150u: { row = array<i32,16>(4,11,8,4,6,11,0,2,9,2,10,9,-1,-1,-1,-1); }
        case 151u: { row = array<i32,16>(10,9,3,10,3,2,9,4,3,11,3,6,4,6,3,-1); }
        case 152u: { row = array<i32,16>(8,2,3,8,4,2,4,6,2,-1,-1,-1,-1,-1,-1,-1); }
        case 153u: { row = array<i32,16>(0,4,2,4,6,2,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 154u: { row = array<i32,16>(1,9,0,2,3,4,2,4,6,4,3,8,-1,-1,-1,-1); }
        case 155u: { row = array<i32,16>(1,9,4,1,4,2,2,4,6,-1,-1,-1,-1,-1,-1,-1); }
        case 156u: { row = array<i32,16>(8,1,3,8,6,1,8,4,6,6,10,1,-1,-1,-1,-1); }
        case 157u: { row = array<i32,16>(10,1,0,10,0,6,6,0,4,-1,-1,-1,-1,-1,-1,-1); }
        case 158u: { row = array<i32,16>(4,6,3,4,3,8,6,10,3,0,3,9,10,9,3,-1); }
        case 159u: { row = array<i32,16>(10,9,4,6,10,4,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 160u: { row = array<i32,16>(4,9,5,7,6,11,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 161u: { row = array<i32,16>(0,8,3,4,9,5,11,7,6,-1,-1,-1,-1,-1,-1,-1); }
        case 162u: { row = array<i32,16>(5,0,1,5,4,0,7,6,11,-1,-1,-1,-1,-1,-1,-1); }
        case 163u: { row = array<i32,16>(11,7,6,8,3,4,3,5,4,3,1,5,-1,-1,-1,-1); }
        case 164u: { row = array<i32,16>(9,5,4,10,1,2,7,6,11,-1,-1,-1,-1,-1,-1,-1); }
        case 165u: { row = array<i32,16>(6,11,7,1,2,10,0,8,3,4,9,5,-1,-1,-1,-1); }
        case 166u: { row = array<i32,16>(7,6,11,5,4,10,4,2,10,4,0,2,-1,-1,-1,-1); }
        case 167u: { row = array<i32,16>(3,4,8,3,5,4,3,2,5,10,5,2,11,7,6,-1); }
        case 168u: { row = array<i32,16>(7,2,3,7,6,2,5,4,9,-1,-1,-1,-1,-1,-1,-1); }
        case 169u: { row = array<i32,16>(9,5,4,0,8,6,0,6,2,6,8,7,-1,-1,-1,-1); }
        case 170u: { row = array<i32,16>(3,6,2,3,7,6,1,5,0,5,4,0,-1,-1,-1,-1); }
        case 171u: { row = array<i32,16>(6,2,8,6,8,7,2,1,8,4,8,5,1,5,8,-1); }
        case 172u: { row = array<i32,16>(9,5,4,10,1,6,1,7,6,1,3,7,-1,-1,-1,-1); }
        case 173u: { row = array<i32,16>(1,6,10,1,7,6,1,0,7,8,7,0,9,5,4,-1); }
        case 174u: { row = array<i32,16>(4,0,10,4,10,5,0,3,10,6,10,7,3,7,10,-1); }
        case 175u: { row = array<i32,16>(7,6,10,7,10,8,5,4,10,4,8,10,-1,-1,-1,-1); }
        case 176u: { row = array<i32,16>(6,9,5,6,11,9,11,8,9,-1,-1,-1,-1,-1,-1,-1); }
        case 177u: { row = array<i32,16>(3,6,11,0,6,3,0,5,6,0,9,5,-1,-1,-1,-1); }
        case 178u: { row = array<i32,16>(0,11,8,0,5,11,0,1,5,5,6,11,-1,-1,-1,-1); }
        case 179u: { row = array<i32,16>(6,11,3,6,3,5,5,3,1,-1,-1,-1,-1,-1,-1,-1); }
        case 180u: { row = array<i32,16>(1,2,10,9,5,11,9,11,8,11,5,6,-1,-1,-1,-1); }
        case 181u: { row = array<i32,16>(0,11,3,0,6,11,0,9,6,5,6,9,1,2,10,-1); }
        case 182u: { row = array<i32,16>(11,8,5,11,5,6,8,0,5,10,5,2,0,2,5,-1); }
        case 183u: { row = array<i32,16>(6,11,3,6,3,5,2,10,3,10,5,3,-1,-1,-1,-1); }
        case 184u: { row = array<i32,16>(5,8,9,5,2,8,5,6,2,3,8,2,-1,-1,-1,-1); }
        case 185u: { row = array<i32,16>(9,5,6,9,6,0,0,6,2,-1,-1,-1,-1,-1,-1,-1); }
        case 186u: { row = array<i32,16>(1,5,8,1,8,0,5,6,8,3,8,2,6,2,8,-1); }
        case 187u: { row = array<i32,16>(1,5,6,2,1,6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 188u: { row = array<i32,16>(1,3,6,1,6,10,3,8,6,5,6,9,8,9,6,-1); }
        case 189u: { row = array<i32,16>(10,1,0,10,0,6,9,5,0,5,6,0,-1,-1,-1,-1); }
        case 190u: { row = array<i32,16>(0,3,8,5,6,10,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 191u: { row = array<i32,16>(10,5,6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 192u: { row = array<i32,16>(11,5,10,7,5,11,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 193u: { row = array<i32,16>(11,5,10,11,7,5,8,3,0,-1,-1,-1,-1,-1,-1,-1); }
        case 194u: { row = array<i32,16>(5,11,7,5,10,11,1,9,0,-1,-1,-1,-1,-1,-1,-1); }
        case 195u: { row = array<i32,16>(10,7,5,10,11,7,9,8,1,8,3,1,-1,-1,-1,-1); }
        case 196u: { row = array<i32,16>(11,1,2,11,7,1,7,5,1,-1,-1,-1,-1,-1,-1,-1); }
        case 197u: { row = array<i32,16>(0,8,3,1,2,7,1,7,5,7,2,11,-1,-1,-1,-1); }
        case 198u: { row = array<i32,16>(9,7,5,9,2,7,9,0,2,2,11,7,-1,-1,-1,-1); }
        case 199u: { row = array<i32,16>(7,5,2,7,2,11,5,9,2,3,2,8,9,8,2,-1); }
        case 200u: { row = array<i32,16>(2,5,10,2,3,5,3,7,5,-1,-1,-1,-1,-1,-1,-1); }
        case 201u: { row = array<i32,16>(8,2,0,8,5,2,8,7,5,10,2,5,-1,-1,-1,-1); }
        case 202u: { row = array<i32,16>(9,0,1,5,10,3,5,3,7,3,10,2,-1,-1,-1,-1); }
        case 203u: { row = array<i32,16>(9,8,2,9,2,1,8,7,2,10,2,5,7,5,2,-1); }
        case 204u: { row = array<i32,16>(1,3,5,3,7,5,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 205u: { row = array<i32,16>(0,8,7,0,7,1,1,7,5,-1,-1,-1,-1,-1,-1,-1); }
        case 206u: { row = array<i32,16>(9,0,3,9,3,5,5,3,7,-1,-1,-1,-1,-1,-1,-1); }
        case 207u: { row = array<i32,16>(9,8,7,5,9,7,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 208u: { row = array<i32,16>(5,8,4,5,10,8,10,11,8,-1,-1,-1,-1,-1,-1,-1); }
        case 209u: { row = array<i32,16>(5,0,4,5,11,0,5,10,11,11,3,0,-1,-1,-1,-1); }
        case 210u: { row = array<i32,16>(0,1,9,8,4,10,8,10,11,10,4,5,-1,-1,-1,-1); }
        case 211u: { row = array<i32,16>(10,11,4,10,4,5,11,3,4,9,4,1,3,1,4,-1); }
        case 212u: { row = array<i32,16>(2,5,1,2,8,5,2,11,8,4,5,8,-1,-1,-1,-1); }
        case 213u: { row = array<i32,16>(0,4,11,0,11,3,4,5,11,2,11,1,5,1,11,-1); }
        case 214u: { row = array<i32,16>(0,2,5,0,5,9,2,11,5,4,5,8,11,8,5,-1); }
        case 215u: { row = array<i32,16>(9,4,5,2,11,3,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 216u: { row = array<i32,16>(2,5,10,3,5,2,3,4,5,3,8,4,-1,-1,-1,-1); }
        case 217u: { row = array<i32,16>(5,10,2,5,2,4,4,2,0,-1,-1,-1,-1,-1,-1,-1); }
        case 218u: { row = array<i32,16>(3,10,2,3,5,10,3,8,5,4,5,8,0,1,9,-1); }
        case 219u: { row = array<i32,16>(5,10,2,5,2,4,1,9,2,9,4,2,-1,-1,-1,-1); }
        case 220u: { row = array<i32,16>(8,4,5,8,5,3,3,5,1,-1,-1,-1,-1,-1,-1,-1); }
        case 221u: { row = array<i32,16>(0,4,5,1,0,5,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 222u: { row = array<i32,16>(8,4,5,8,5,3,9,0,5,0,3,5,-1,-1,-1,-1); }
        case 223u: { row = array<i32,16>(9,4,5,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 224u: { row = array<i32,16>(4,11,7,4,9,11,9,10,11,-1,-1,-1,-1,-1,-1,-1); }
        case 225u: { row = array<i32,16>(0,8,3,4,9,7,9,11,7,9,10,11,-1,-1,-1,-1); }
        case 226u: { row = array<i32,16>(1,10,11,1,11,4,1,4,0,7,4,11,-1,-1,-1,-1); }
        case 227u: { row = array<i32,16>(3,1,4,3,4,8,1,10,4,7,4,11,10,11,4,-1); }
        case 228u: { row = array<i32,16>(4,11,7,9,11,4,9,2,11,9,1,2,-1,-1,-1,-1); }
        case 229u: { row = array<i32,16>(9,7,4,9,11,7,9,1,11,2,11,1,0,8,3,-1); }
        case 230u: { row = array<i32,16>(11,7,4,11,4,2,2,4,0,-1,-1,-1,-1,-1,-1,-1); }
        case 231u: { row = array<i32,16>(11,7,4,11,4,2,8,3,4,3,2,4,-1,-1,-1,-1); }
        case 232u: { row = array<i32,16>(2,9,10,2,7,9,2,3,7,7,4,9,-1,-1,-1,-1); }
        case 233u: { row = array<i32,16>(9,10,7,9,7,4,10,2,7,8,7,0,2,0,7,-1); }
        case 234u: { row = array<i32,16>(3,7,10,3,10,2,7,4,10,1,10,0,4,0,10,-1); }
        case 235u: { row = array<i32,16>(1,10,2,8,7,4,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 236u: { row = array<i32,16>(4,9,1,4,1,7,7,1,3,-1,-1,-1,-1,-1,-1,-1); }
        case 237u: { row = array<i32,16>(4,9,1,4,1,7,0,8,1,8,7,1,-1,-1,-1,-1); }
        case 238u: { row = array<i32,16>(4,0,3,7,4,3,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 239u: { row = array<i32,16>(4,8,7,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 240u: { row = array<i32,16>(9,10,8,10,11,8,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 241u: { row = array<i32,16>(3,0,9,3,9,11,11,9,10,-1,-1,-1,-1,-1,-1,-1); }
        case 242u: { row = array<i32,16>(0,1,10,0,10,8,8,10,11,-1,-1,-1,-1,-1,-1,-1); }
        case 243u: { row = array<i32,16>(3,1,10,11,3,10,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 244u: { row = array<i32,16>(1,2,11,1,11,9,9,11,8,-1,-1,-1,-1,-1,-1,-1); }
        case 245u: { row = array<i32,16>(3,0,9,3,9,11,1,2,9,2,11,9,-1,-1,-1,-1); }
        case 246u: { row = array<i32,16>(0,2,11,8,0,11,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 247u: { row = array<i32,16>(3,2,11,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 248u: { row = array<i32,16>(2,3,8,2,8,10,10,8,9,-1,-1,-1,-1,-1,-1,-1); }
        case 249u: { row = array<i32,16>(9,10,2,0,9,2,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 250u: { row = array<i32,16>(2,3,8,2,8,10,0,1,8,1,10,8,-1,-1,-1,-1); }
        case 251u: { row = array<i32,16>(1,10,2,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 252u: { row = array<i32,16>(1,3,8,9,1,8,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 253u: { row = array<i32,16>(0,9,1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        case 254u: { row = array<i32,16>(0,3,8,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
        default:   { row = array<i32,16>(-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1); }
    }
    return row[j];
}

// ── EDGE_VERTS: which 2 corners each of the 12 edges connects ─────────────────
fn edge_vert_a(e: u32) -> u32 {
    var t = array<u32,12>(0u,1u,2u,3u,4u,5u,6u,7u,0u,1u,2u,3u);
    return t[e];
}
fn edge_vert_b(e: u32) -> u32 {
    var t = array<u32,12>(1u,2u,3u,0u,5u,6u,7u,4u,4u,5u,6u,7u);
    return t[e];
}

// ── CORNER_OFFSETS (dx,dy,dz for corners 0..7) ───────────────────────────────
fn corner_offset(c: u32) -> vec3<f32> {
    switch (c) {
        case 0u: { return vec3<f32>(0.0, 0.0, 0.0); }
        case 1u: { return vec3<f32>(1.0, 0.0, 0.0); }
        case 2u: { return vec3<f32>(1.0, 1.0, 0.0); }
        case 3u: { return vec3<f32>(0.0, 1.0, 0.0); }
        case 4u: { return vec3<f32>(0.0, 0.0, 1.0); }
        case 5u: { return vec3<f32>(1.0, 0.0, 1.0); }
        case 6u: { return vec3<f32>(1.0, 1.0, 1.0); }
        case 7u: { return vec3<f32>(0.0, 1.0, 1.0); }
        default: { return vec3<f32>(0.0, 0.0, 0.0); }
    }
}

// ── Linear interpolation on an edge ──────────────────────────────────────────
fn vertex_interp(iso: f32, pA: vec3<f32>, pB: vec3<f32>, vA: f32, vB: f32) -> vec3<f32> {
    let denom = vB - vA;
    var t: f32 = 0.5;
    if (abs(denom) >= 1e-10) {
        t = clamp((iso - vA) / denom, 0.0, 1.0);
    }
    return pA + t * (pB - pA);
}

// ── Scalar field gradient via central differences ─────────────────────────────
// Returns the gradient at world position p. We find the nearest cell, then
// sample ±1 cell in each axis (clamped to interior). Normal = -normalize(grad)
// so it points outward (from high to low density).
fn scalar_gradient(nx: u32, ny: u32, nz: u32, p: vec3<f32>) -> vec3<f32> {
    let h = mesh_params.h;
    let origin = mesh_params.origin.xyz;
    // Map world position to cell coordinates.
    let g = (p - origin) / h;
    let ci = u32(clamp(round(g.x - 0.5), 0.0, f32(nx - 1u)));
    let cj = u32(clamp(round(g.y - 0.5), 0.0, f32(ny - 1u)));
    let ck = u32(clamp(round(g.z - 0.5), 0.0, f32(nz - 1u)));

    // Clamp neighbors to interior so we never read boundary garbage.
    let xi0 = select(ci - 1u, 0u,    ci == 0u);
    let xi1 = select(ci + 1u, nx-1u, ci == nx-1u);
    let yj0 = select(cj - 1u, 0u,    cj == 0u);
    let yj1 = select(cj + 1u, ny-1u, cj == ny-1u);
    let zk0 = select(ck - 1u, 0u,    ck == 0u);
    let zk1 = select(ck + 1u, nz-1u, ck == nz-1u);

    let sx0 = scalar[xi0 + nx * (cj  + ny * ck)];
    let sx1 = scalar[xi1 + nx * (cj  + ny * ck)];
    let sy0 = scalar[ci  + nx * (yj0 + ny * ck)];
    let sy1 = scalar[ci  + nx * (yj1 + ny * ck)];
    let sz0 = scalar[ci  + nx * (cj  + ny * zk0)];
    let sz1 = scalar[ci  + nx * (cj  + ny * zk1)];

    let dx = f32(xi1 - xi0);
    let dy = f32(yj1 - yj0);
    let dz = f32(zk1 - zk0);
    let scale = 1.0 / h;
    let gx = (sx1 - sx0) / (dx * scale);
    let gy = (sy1 - sy0) / (dy * scale);
    let gz = (sz1 - sz0) / (dz * scale);
    return vec3<f32>(gx, gy, gz);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let nx = mesh_params.dims.x;
    let ny = mesh_params.dims.y;
    let nz = mesh_params.dims.z;
    let nxm1 = nx - 1u;
    let nym1 = ny - 1u;
    let nzm1 = nz - 1u;
    let cube_count = nxm1 * nym1 * nzm1;
    let cube_id = gid.x;
    if (cube_id >= cube_count) { return; }

    // Cube (i, j, k) from flat id.
    let i = cube_id % nxm1;
    let j = (cube_id / nxm1) % nym1;
    let k = cube_id / (nxm1 * nym1);

    let h = mesh_params.h;
    let iso = mesh_params.isolevel;
    let origin = mesh_params.origin.xyz;

    // The 8 corners. Corner world position = origin + (cell_i+0.5, cell_j+0.5, cell_k+0.5)*h
    // Corner c adds dx/dy/dz offset in {0,1} to (i,j,k).
    var corner_val: array<f32, 8>;
    var corner_pos: array<vec3<f32>, 8>;
    for (var c = 0u; c < 8u; c++) {
        let off = corner_offset(c);
        let ci = i + u32(off.x);
        let cj = j + u32(off.y);
        let ck = k + u32(off.z);
        let cell_idx = ci + nx * (cj + ny * ck);
        corner_val[c] = scalar[cell_idx];
        // Cell center world position
        corner_pos[c] = origin + (vec3<f32>(f32(ci), f32(cj), f32(ck)) + vec3<f32>(0.5)) * h;
    }

    // Build cube case index: INSIDE when scalar >= isolevel (fluid = high density).
    var cube_case = 0u;
    for (var c = 0u; c < 8u; c++) {
        if (corner_val[c] >= iso) {
            cube_case |= (1u << c);
        }
    }

    let edges = edge_table(cube_case);
    if (edges == 0u) { return; }

    // Interpolate vertex positions on each of the 12 edges.
    var vlist: array<vec3<f32>, 12>;
    for (var e = 0u; e < 12u; e++) {
        if ((edges & (1u << e)) != 0u) {
            let a = edge_vert_a(e);
            let b = edge_vert_b(e);
            vlist[e] = vertex_interp(iso, corner_pos[a], corner_pos[b], corner_val[a], corner_val[b]);
        }
    }

    // Emit triangles.
    var ti = 0u;
    loop {
        if (ti >= 15u) { break; }
        let e0 = tri_table(cube_case, ti);
        if (e0 < 0) { break; }
        let e1 = tri_table(cube_case, ti + 1u);
        let e2 = tri_table(cube_case, ti + 2u);

        // Reserve 3 consecutive slots atomically.
        let base = atomicAdd(&counter[0], 3u);
        if (base + 3u > MAX_VERTS) {
            // Overflow: undo and stop (stable partial mesh).
            atomicSub(&counter[0], 3u);
            break;
        }

        let p0 = vlist[u32(e0)];
        let p1 = vlist[u32(e1)];
        let p2 = vlist[u32(e2)];

        // Per-vertex gradient normals. Normal = normalize(-gradient) = outward from fluid.
        let g0 = scalar_gradient(nx, ny, nz, p0);
        let g1 = scalar_gradient(nx, ny, nz, p1);
        let g2 = scalar_gradient(nx, ny, nz, p2);

        var n0 = vec3<f32>(0.0, 1.0, 0.0);
        var n1 = vec3<f32>(0.0, 1.0, 0.0);
        var n2 = vec3<f32>(0.0, 1.0, 0.0);
        if (dot(g0, g0) > 1e-12) { n0 = normalize(-g0); }
        if (dot(g1, g1) > 1e-12) { n1 = normalize(-g1); }
        if (dot(g2, g2) > 1e-12) { n2 = normalize(-g2); }

        // Per-vertex foam factor (nrm.w) from the surface speed field.
        let f0 = foam_at(nx, ny, nz, p0);
        let f1 = foam_at(nx, ny, nz, p1);
        let f2 = foam_at(nx, ny, nz, p2);

        verts[base]     = Vertex(vec4<f32>(p0, 0.0), vec4<f32>(n0, f0));
        verts[base + 1u] = Vertex(vec4<f32>(p1, 0.0), vec4<f32>(n1, f1));
        verts[base + 2u] = Vertex(vec4<f32>(p2, 0.0), vec4<f32>(n2, f2));

        ti += 3u;
    }
}
