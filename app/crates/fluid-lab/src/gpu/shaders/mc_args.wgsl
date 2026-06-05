// Write indirect draw args after MC: vertex_count = min(counter, MAX_VERTS), 1, 0, 0.
// One invocation.

const MAX_VERTS: u32 = 2400000u;

@group(0) @binding(0) var<storage, read> counter: array<u32>;
@group(0) @binding(1) var<storage, read_write> indirect_args: array<u32>;

@compute @workgroup_size(1)
fn main() {
    indirect_args[0] = min(counter[0], MAX_VERTS); // vertex_count
    indirect_args[1] = 1u;  // instance_count
    indirect_args[2] = 0u;  // first_vertex
    indirect_args[3] = 0u;  // first_instance
}
