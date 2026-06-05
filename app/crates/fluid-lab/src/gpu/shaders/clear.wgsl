// Generic buffer clear: zero every element of a storage buffer.
// Reused for i32/u32/f32 buffers (zero bits == 0 in all three).
// Length comes from arrayLength(), so one pipeline clears any size.

@group(0) @binding(0) var<storage, read_write> buf: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i < arrayLength(&buf)) {
        buf[i] = 0u;
    }
}
