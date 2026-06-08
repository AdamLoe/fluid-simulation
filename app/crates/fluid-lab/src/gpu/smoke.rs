//! Platform smoke test (Phase 0.1, work item 2).
//!
//! Validates the three platform capabilities the whole project depends on, before
//! any architecture is built on top of them:
//!   1. compute dispatch works,
//!   2. integer `atomicAdd` scatter into a `u32` storage buffer works (the forced
//!      basis for GPU P2G — WebGPU has no float atomics),
//!   3. a one-time readback returns the expected deterministic result.
//!
//! This is an explicit, one-shot readback — allowed by the no-normal-frame-readback
//! rule. It runs once at boot and is not on any per-frame path.

const WORKGROUPS: u32 = 64;
const WORKGROUP_SIZE: u32 = 64;
const EXPECTED: u32 = WORKGROUPS * WORKGROUP_SIZE; // every invocation adds 1

const SHADER: &str = r#"
@group(0) @binding(0) var<storage, read_write> counter: atomic<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    atomicAdd(&counter, 1u);
}
"#;

pub async fn run_atomic_smoke_test(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<String, String> {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("smoke shader"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });

    let storage = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("smoke counter"),
        size: 4,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    // Initialize to zero deterministically.
    queue.write_buffer(&storage, 0, &0u32.to_le_bytes());

    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("smoke readback"),
        size: 4,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("smoke pipeline"),
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("smoke bind group"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: storage.as_entire_binding(),
        }],
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("smoke"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("smoke pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(WORKGROUPS, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&storage, 0, &readback, 0, 4);
    queue.submit(std::iter::once(encoder.finish()));

    // One-time map + readback.
    let slice = readback.slice(..);
    let (tx, rx) = futures_channel::oneshot::channel();
    slice.map_async(wgpu::MapMode::Read, move |res| {
        let _ = tx.send(res);
    });
    // On the WebGPU backend poll does not block; the browser resolves the map
    // promise and the awaited oneshot completes on the microtask queue.
    let _ = device.poll(wgpu::PollType::Poll);
    rx.await
        .map_err(|_| "map_async sender dropped".to_string())?
        .map_err(|e| format!("map_async failed: {e:?}"))?;

    let value = {
        let data = slice.get_mapped_range();
        u32::from_le_bytes([data[0], data[1], data[2], data[3]])
    };
    readback.unmap();

    let _ = WORKGROUP_SIZE; // documented constant; workgroup size is fixed in WGSL
    if value == EXPECTED {
        Ok(format!(
            "atomicAdd result {value} == expected {EXPECTED} ({WORKGROUPS} workgroups x {WORKGROUP_SIZE})"
        ))
    } else {
        Err(format!(
            "atomicAdd result {value} != expected {EXPECTED} — integer atomics or compute dispatch is broken"
        ))
    }
}
