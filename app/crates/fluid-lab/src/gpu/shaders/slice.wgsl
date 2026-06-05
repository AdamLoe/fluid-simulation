// Grid-slice debug renderer — multi-mode. Draws one flat quad per cell in the XY
// cross-section at k = nz/2. Air cells (and non-liquid cells in modes 1/2) are made
// degenerate (scale 0) so they produce no visible pixels.
//
// Binding layout:
//   @group(0) @binding(0)  uniform U            (vertex)
//   @group(0) @binding(1)  cell_type  array<u32> (vertex, read-only storage)
//   @group(0) @binding(2)  pressure   array<f32> (vertex, read-only storage)
//   @group(0) @binding(3)  u_vel      array<f32> (vertex, read-only storage)
//   @group(0) @binding(4)  v_vel      array<f32> (vertex, read-only storage)
//   @group(0) @binding(5)  w_vel      array<f32> (vertex, read-only storage)
//
// Modes (stored as f32 in u.grid.w, rounded to u32 at runtime):
//   0 = cell-type  (solid gray / liquid blue / air hidden)
//   1 = pressure   (liquid only; blue→white→red diverging colormap, scale /2000)
//   2 = speed      (liquid only; blue→cyan→yellow→red colormap, scale /4)

struct U {
    view_proj: mat4x4<f32>,
    // xyz = nx, ny, nz (per-axis cell counts), w=unused
    dims: vec4<u32>,
    // x=slice_k, y=h (cell size), z=mode (0/1/2 as f32), w=unused
    grid: vec4<f32>,
    // xyz=origin (world-space corner of the grid), w=unused
    origin: vec4<f32>,
};

@group(0) @binding(0) var<uniform>          u:         U;
@group(0) @binding(1) var<storage, read>    cell_type: array<u32>;
@group(0) @binding(2) var<storage, read>    pressure:  array<f32>;
@group(0) @binding(3) var<storage, read>    u_vel:     array<f32>;
@group(0) @binding(4) var<storage, read>    v_vel:     array<f32>;
@group(0) @binding(5) var<storage, read>    w_vel:     array<f32>;

struct VsOut {
    @builtin(position) clip:  vec4<f32>,
    @location(0)       color: vec3<f32>,
};

// Diverging blue→white→red colormap for pressure (t in [0,1], 0.5 = white).
fn colormap_diverging(t: f32) -> vec3<f32> {
    let tc = clamp(t, 0.0, 1.0);
    if tc < 0.5 {
        // blue → white
        let s = tc * 2.0;
        return mix(vec3<f32>(0.0, 0.2, 0.9), vec3<f32>(1.0, 1.0, 1.0), s);
    } else {
        // white → red
        let s = (tc - 0.5) * 2.0;
        return mix(vec3<f32>(1.0, 1.0, 1.0), vec3<f32>(0.9, 0.05, 0.05), s);
    }
}

// Sequential blue→cyan→yellow→red colormap for speed (t in [0,1]).
fn colormap_speed(t: f32) -> vec3<f32> {
    let tc = clamp(t, 0.0, 1.0);
    if tc < 0.333 {
        let s = tc / 0.333;
        return mix(vec3<f32>(0.0, 0.0, 0.8), vec3<f32>(0.0, 0.9, 0.9), s);
    } else if tc < 0.667 {
        let s = (tc - 0.333) / 0.333;
        return mix(vec3<f32>(0.0, 0.9, 0.9), vec3<f32>(1.0, 1.0, 0.0), s);
    } else {
        let s = (tc - 0.667) / 0.333;
        return mix(vec3<f32>(1.0, 1.0, 0.0), vec3<f32>(0.9, 0.0, 0.0), s);
    }
}

@vertex
fn vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VsOut {
    let nx      = u.dims.x;
    let ny      = u.dims.y;
    let nz      = u.dims.z;
    let slice_k = u32(u.grid.x);
    let h       = u.grid.y;
    let mode    = u32(u.grid.z);   // 0, 1, or 2

    // Decompose instance index into (i, j) across the XY plane.
    let i = ii % nx;
    let j = ii / nx;
    let k = slice_k;

    let idx = i + nx * (j + ny * k);
    let ct  = cell_type[idx]; // 0=Solid, 1=Liquid, 2=Air

    // Cell center in world space.
    let cx = u.origin.x + (f32(i) + 0.5) * h;
    let cy = u.origin.y + (f32(j) + 0.5) * h;
    let cz = u.origin.z + (f32(k) + 0.5) * h;

    var color: vec3<f32>;
    var half: f32;

    if mode == 0u {
        // ── Mode 0: cell-type ──
        // Air hidden, Solid gray, Liquid blue.
        half = select(h * 0.45, 0.0, ct == 2u);
        if ct == 0u {
            color = vec3<f32>(0.40, 0.40, 0.46); // Solid
        } else {
            color = vec3<f32>(0.20, 0.55, 1.00); // Liquid (air is degenerate anyway)
        }
    } else if mode == 1u {
        // ── Mode 1: pressure ──
        // Only liquid cells are visible.
        if ct != 1u {
            half = 0.0;
            color = vec3<f32>(0.0, 0.0, 0.0);
        } else {
            half = h * 0.45;
            let p = pressure[idx];
            // Map pressure into [0,1]: 0.5 = zero pressure.
            let t = clamp(p / 2000.0 * 0.5 + 0.5, 0.0, 1.0);
            color = colormap_diverging(t);
        }
    } else {
        // ── Mode 2: speed ──
        // Only liquid cells are visible.
        if ct != 1u {
            half = 0.0;
            color = vec3<f32>(0.0, 0.0, 0.0);
        } else {
            half = h * 0.45;
            // u faces: indices (i,j,k) and (i+1,j,k) in (nx+1,ny,nz) layout.
            // v faces: indices (i,j,k) and (i,j+1,k) in (nx,ny+1,nz) layout.
            // w faces: indices (i,j,k) and (i,j,k+1) in (nx,ny,nz+1) layout.
            let u0 = u_vel[i       + (nx + 1u) * (j + ny * k)];
            let u1 = u_vel[(i + 1u) + (nx + 1u) * (j + ny * k)];
            let v0 = v_vel[i + nx * (j       + (ny + 1u) * k)];
            let v1 = v_vel[i + nx * ((j + 1u) + (ny + 1u) * k)];
            let w0 = w_vel[i + nx * (j + ny * k)];
            let w1 = w_vel[i + nx * (j + ny * (k + 1u))];
            let avg_u = (u0 + u1) * 0.5;
            let avg_v = (v0 + v1) * 0.5;
            let avg_w = (w0 + w1) * 0.5;
            let spd = length(vec3<f32>(avg_u, avg_v, avg_w));
            // Normalize by 4 m/s and clamp to [0,1].
            let t = clamp(spd / 4.0, 0.0, 1.0);
            color = colormap_speed(t);
        }
    }

    // Two triangles forming a square in the XY plane:
    // Tri 0: (-1,-1), (1,-1), (1,1)
    // Tri 1: (-1,-1), (1,1), (-1,1)
    var offsets = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0,  1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0,  1.0), vec2<f32>(-1.0, 1.0),
    );
    let off = offsets[vi] * half;

    let world = vec4<f32>(cx + off.x, cy + off.y, cz, 1.0);

    var out: VsOut;
    out.clip  = u.view_proj * world;
    out.color = color;
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color, 1.0);
}
