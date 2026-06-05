// Divergence at liquid cell centers from staggered face velocities.
// div = (u[i+1]-u[i] + v[j+1]-v[j] + w[k+1]-w[k]) / h. Non-liquid cells -> 0.

struct Params {
    dims: vec4<u32>,
    geom: vec4<f32>,   // h, inv_h, dt, fixed_scale
    phys: vec4<f32>,
    origin: vec4<f32>,
    grav: vec4<f32>,
    spc:  vec4<f32>,   // rest_per_cell, volume_stiffness, drift_clamp, _
    cls:  vec4<f32>,   // liquid_threshold, surface_dilation, _, _
    gdim: vec4<u32>,   // nx, ny, nz, _
};
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> u_vel: array<f32>;
@group(0) @binding(2) var<storage, read> v_vel: array<f32>;
@group(0) @binding(3) var<storage, read> w_vel: array<f32>;
@group(0) @binding(4) var<storage, read> cell_type: array<u32>;
@group(0) @binding(5) var<storage, read_write> divergence: array<f32>;
@group(0) @binding(6) var<storage, read> occ: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let nx = params.gdim.x;
    let ny = params.gdim.y;
    let nz = params.gdim.z;
    let c = gid.x;
    if (c >= nx * ny * nz) { return; }
    if (cell_type[c] != 1u) { divergence[c] = 0.0; return; }

    let i = c % nx;
    let j = (c / nx) % ny;
    let k = c / (nx * ny);

    // u dims (nx+1,ny,nz); v dims (nx,ny+1,nz); w dims (nx,ny,nz+1)
    let u_hi = u_vel[(i + 1u) + (nx + 1u) * (j + ny * k)];
    let u_lo = u_vel[i + (nx + 1u) * (j + ny * k)];
    let v_hi = v_vel[i + nx * ((j + 1u) + (ny + 1u) * k)];
    let v_lo = v_vel[i + nx * (j + (ny + 1u) * k)];
    let w_hi = w_vel[i + nx * (j + ny * (k + 1u))];
    let w_lo = w_vel[i + nx * (j + ny * k)];

    var div = ((u_hi - u_lo) + (v_hi - v_lo) + (w_hi - w_lo)) * params.geom.y; // * inv_h

    // Volume-drift (anti-clump) source: cells holding more particles than the rest
    // target get a NEGATIVE divergence bias, so the projection produces net OUTWARD
    // flow there and the fluid spreads toward uniform packing. This is the physical
    // replacement for the old occupancy-repulsion hack. Off when stiffness == 0.
    let rest  = params.spc.x;
    let stiff = params.spc.y;
    let clamp_d = params.spc.z;
    if (stiff > 0.0 && rest > 0.0) {
        let over = max(0.0, f32(occ[c]) - rest) / rest; // fractional over-packing
        let drift = min(stiff * over, clamp_d);
        div = div - drift;
    }

    divergence[c] = div;
}
