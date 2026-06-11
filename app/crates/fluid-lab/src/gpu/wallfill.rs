//! Wall-fill system (v1.21): dense per-wall-cell flat water sheet against glass.
//!
//! Two sub-systems:
//!
//! `WallOccupancySystem` — a compute pass that writes dense per-wall-cell occupancy
//! into a dedicated storage buffer. Runs every frame (no decay), always reflecting
//! the current `cell_type` state.
//! Covers 5 faces: back (z=lo), left (x=lo), right (x=hi), front (z=hi), floor (y=lo).
//! Buffer layout: one `f32` occupancy value per supersampled face texel, 5 faces concatenated.
//!
//! `WallFillRenderer` — a render pass that draws a single full-screen triangle,
//! intersects each of the 5 tank planes per pixel, and writes into the SAME three
//! MRT targets as the particle thickness pass:
//!   target 0 thickness  (R16Float, Add)  — fill_slab
//!   target 1 nearest_z  (R16Float, Min)  — glass-plane eye distance
//!   target 2 whitewater (R16Float, Add)  — 0.0 (foam untouched)
//!   target 3 wallfill mask (R16Float, Replace) — per-pixel fill coverage
//! This runs AFTER the particle thickness pass (LoadOp::Load) and BEFORE the
//! bilateral smoothing passes, so the fill is smoothed with the rest of the surface
//! while the mask lets the composite tune fill-only color/reflection. Smoothing does
//! not need a separate path.
//!
//! Face storage lengths:
//!   back  / front (z walls): (nx*ss) * (ny*ss)  (index = j*nx_ss + i)
//!   left  / right (x walls): (nz*ss) * (ny*ss)  (index = j*nz_ss + k)
//!   floor (y=lo):             (nx*ss) * (nz*ss)  (index = k*nx_ss + i)

use crate::settings::HeroParams;
use glam::{Mat3, Mat4, Vec3};
use wgpu::util::DeviceExt;

const WG: u32 = 64;

/// Shared uniform for the compute occupancy pass.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct OccUniform {
    /// x=nx_ss, y=ny_ss, z=nz_ss, w=total occupancy entries
    dims: [u32; 4],
    /// x=nx, y=ny, z=nz, w=ss
    orig: [u32; 4],
    /// x=back(nx_ss*ny_ss), y=left(nz_ss*ny_ss), z=right(nz_ss*ny_ss), w=front(nx_ss*ny_ss)
    nc: [u32; 4],
    /// x=nc_floor(nx_ss*nz_ss), yzw=unused
    nc_floor: [u32; 4],
    /// x=fill_enabled(0/1), y=fill_strength, z=fill_slab, w=waterline_softness
    fill: [f32; 4],
    /// tank world-space lower corner, w=unused
    tank_lo: [f32; 4],
    /// tank world-space upper corner, w=unused
    tank_hi: [f32; 4],
}

/// Per-frame camera uniform for the fill render pass.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FillUniform {
    /// x=fill_enabled, y=fill_strength, z=fill_slab, w=waterline_softness
    fill: [f32; 4],
    /// x=nx_ss, y=ny_ss, z=nz_ss, w=total occupancy entries
    dims: [u32; 4],
    /// x=back(nx_ss*ny_ss), y=left(nz_ss*ny_ss), z=right(nz_ss*ny_ss), w=front(nx_ss*ny_ss)
    nc: [u32; 4],
    /// x=nc_floor, yzw=unused
    nc_floor: [u32; 4],
    /// x=tan(fov_y/2), y=width, z=height, w=unused
    cam_params: [f32; 4],
    /// tank lo (xyz, w=unused)
    tank_lo: [f32; 4],
    /// tank hi (xyz, w=unused)
    tank_hi: [f32; 4],
    /// camera eye in box-local space (xyz, w=unused)
    box_eye_local: [f32; 4],
    /// box-local→world rotation col0 (padded)
    box_rot_col0: [f32; 4],
    /// box-local→world rotation col1
    box_rot_col1: [f32; 4],
    /// box-local→world rotation col2
    box_rot_col2: [f32; 4],
    /// eye→world rotation (mat4x4, upper-left 3x3 used)
    eye_to_world: [[f32; 4]; 4],
    /// x=flat_water epsilon, yzw=unused
    flat_epsilon: [f32; 4],
}

pub struct WallOccupancySystem {
    /// Storage buffer: 1 f32 occupancy value per wall texel/cell.
    pub occ_buf: wgpu::Buffer,
    /// Uniform buffer (OccUniform).
    uniform_buf: wgpu::Buffer,
    pipeline: wgpu::ComputePipeline,
    bind_group: wgpu::BindGroup,
    total_columns: u32,
    ss: u32,
    orig_nx: u32,
    orig_ny: u32,
    orig_nz: u32,
    nx: u32,
    ny: u32,
    nz: u32,
    nc_back: u32,
    nc_left: u32,
    nc_right: u32,
    nc_front: u32,
    nc_floor: u32,
    tank_lo: [f32; 3],
    tank_hi: [f32; 3],
}

impl WallOccupancySystem {
    pub fn new(
        device: &wgpu::Device,
        cell_type_buf: &wgpu::Buffer,
        nx: u32,
        ny: u32,
        nz: u32,
        tank_lo: [f32; 3],
        tank_hi: [f32; 3],
        hero: &HeroParams,
    ) -> Self {
        let ss = hero.flat_water_fill_supersample.max(1).min(32);
        let nx_ss = nx.saturating_mul(ss);
        let ny_ss = ny.saturating_mul(ss);
        let nz_ss = nz.saturating_mul(ss);
        let nc_back = nx_ss * ny_ss;
        let nc_left = nz_ss * ny_ss;
        let nc_right = nz_ss * ny_ss;
        let nc_front = nx_ss * ny_ss;
        let nc_floor = nx_ss * nz_ss;
        let total_columns = nc_back + nc_left + nc_right + nc_front + nc_floor;

        let init_u = OccUniform {
            dims: [nx_ss, ny_ss, nz_ss, total_columns],
            orig: [nx, ny, nz, ss],
            nc: [nc_back, nc_left, nc_right, nc_front],
            nc_floor: [nc_floor, 0, 0, 0],
            fill: [
                if hero.flat_water_fill_enabled {
                    1.0
                } else {
                    0.0
                },
                hero.flat_water_fill_strength.clamp(0.0, 1.0),
                hero.flat_water_fill_slab.max(0.0),
                hero.flat_water_waterline_softness.max(0.001),
            ],
            tank_lo: [tank_lo[0], tank_lo[1], tank_lo[2], 0.0],
            tank_hi: [tank_hi[0], tank_hi[1], tank_hi[2], 0.0],
        };

        let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("wallfill occ uniform"),
            contents: bytemuck::bytes_of(&init_u),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // 1 f32 per wall texel/cell.
        let occ_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("wallfill occ buffer"),
            size: (total_columns as u64) * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("wallfill compute shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/wallfill.wgsl").into()),
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("wallfill occ bgl"),
            entries: &[
                // binding 0: OccUniform
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // binding 1: cell_type (read-only)
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // binding 2: occ_buf (read_write)
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("wallfill occ bg"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: cell_type_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: occ_buf.as_entire_binding(),
                },
            ],
        });

        let pl_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("wallfill occ layout"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("wallfill occupancy"),
            layout: Some(&pl_layout),
            module: &shader,
            entry_point: Some("cs_occupancy"),
            compilation_options: Default::default(),
            cache: None,
        });

        Self {
            occ_buf,
            uniform_buf,
            pipeline,
            bind_group,
            total_columns,
            ss,
            orig_nx: nx,
            orig_ny: ny,
            orig_nz: nz,
            nx: nx_ss,
            ny: ny_ss,
            nz: nz_ss,
            nc_back,
            nc_left,
            nc_right,
            nc_front,
            nc_floor,
            tank_lo,
            tank_hi,
        }
    }

    /// Dispatch the occupancy compute pass.
    pub fn record_step(&self, device: &wgpu::Device, queue: &wgpu::Queue, hero: &HeroParams) {
        // Update uniform with current fill params.
        let u = OccUniform {
            dims: [self.nx, self.ny, self.nz, self.total_columns],
            orig: [self.orig_nx, self.orig_ny, self.orig_nz, self.ss],
            nc: [self.nc_back, self.nc_left, self.nc_right, self.nc_front],
            nc_floor: [self.nc_floor, 0, 0, 0],
            fill: [
                if hero.flat_water_fill_enabled {
                    1.0
                } else {
                    0.0
                },
                hero.flat_water_fill_strength.clamp(0.0, 1.0),
                hero.flat_water_fill_slab.max(0.0),
                hero.flat_water_waterline_softness.max(0.001),
            ],
            tank_lo: [self.tank_lo[0], self.tank_lo[1], self.tank_lo[2], 0.0],
            tank_hi: [self.tank_hi[0], self.tank_hi[1], self.tank_hi[2], 0.0],
        };
        queue.write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&u));

        if !hero.flat_water_fill_enabled || self.total_columns == 0 {
            return;
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("wallfill occupancy"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("wallfill occupancy"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            let groups = (self.total_columns + WG - 1) / WG;
            pass.dispatch_workgroups(groups, 1, 1);
        }
        queue.submit(std::iter::once(encoder.finish()));
    }

    pub fn dims(&self) -> (u32, u32, u32) {
        (self.nx, self.ny, self.nz)
    }

    pub fn column_counts(&self) -> (u32, u32, u32, u32, u32) {
        (
            self.nc_back,
            self.nc_left,
            self.nc_right,
            self.nc_front,
            self.nc_floor,
        )
    }

    pub fn total_columns(&self) -> u32 {
        self.total_columns
    }
}

// ============================================================
// Render pass: wall-fill injection
// ============================================================

pub struct WallFillRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    fill_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    nx: u32,
    ny: u32,
    nz: u32,
    nc_back: u32,
    nc_left: u32,
    nc_right: u32,
    nc_front: u32,
    nc_floor: u32,
    total_columns: u32,
    tank_lo: [f32; 3],
    tank_hi: [f32; 3],
}

impl WallFillRenderer {
    pub fn new(device: &wgpu::Device, occ: &WallOccupancySystem, hero: &HeroParams) -> Self {
        let (nx, ny, nz) = occ.dims();
        let (nc_back, nc_left, nc_right, nc_front, nc_floor) = occ.column_counts();
        let total_columns = occ.total_columns();

        let init_fill = FillUniform {
            fill: [
                if hero.flat_water_fill_enabled {
                    1.0
                } else {
                    0.0
                },
                hero.flat_water_fill_strength.clamp(0.0, 1.0),
                hero.flat_water_fill_slab.max(0.0),
                hero.flat_water_waterline_softness.max(0.001),
            ],
            dims: [nx, ny, nz, total_columns],
            nc: [nc_back, nc_left, nc_right, nc_front],
            nc_floor: [nc_floor, 0, 0, 0],
            cam_params: [(50.0_f32.to_radians() * 0.5).tan(), 1.0, 1.0, 0.0],
            tank_lo: [occ.tank_lo[0], occ.tank_lo[1], occ.tank_lo[2], 0.0],
            tank_hi: [occ.tank_hi[0], occ.tank_hi[1], occ.tank_hi[2], 0.0],
            box_eye_local: [0.0; 4],
            box_rot_col0: [1.0, 0.0, 0.0, 0.0],
            box_rot_col1: [0.0, 1.0, 0.0, 0.0],
            box_rot_col2: [0.0, 0.0, 1.0, 0.0],
            eye_to_world: Mat4::IDENTITY.to_cols_array_2d(),
            flat_epsilon: [hero.flat_water_epsilon.max(0.0), 0.0, 0.0, 0.0],
        };

        let fill_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("wallfill fill uniform"),
            contents: bytemuck::bytes_of(&init_fill),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("wallfill render shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/wallfill.wgsl").into()),
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("wallfill fill bgl"),
            entries: &[
                // binding 3: FillUniform
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // binding 4: occ_buf (read-only for render)
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("wallfill fill bg"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: fill_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: occ.occ_buf.as_entire_binding(),
                },
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("wallfill fill layout"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });

        // Pipeline matching the SAME blend modes as the particle thickness pipeline,
        // plus a wall-fill-only coverage mask for composite-time color/reflection tuning:
        //   target 0 (thickness): Add/Add
        //   target 1 (nearest_z): Min/Min
        //   target 2 (whitewater): Add/Add
        //   target 3 (wallfill_mask): Replace
        let add_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let min_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Min,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Min,
            },
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("wallfill fill pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_fill"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_fill"),
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::R16Float,
                        blend: Some(add_blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::R16Float,
                        blend: Some(min_blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::R16Float,
                        blend: Some(add_blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::R16Float,
                        blend: Some(wgpu::BlendState::REPLACE),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Self {
            pipeline,
            bind_group_layout: bgl,
            fill_buf,
            bind_group,
            nx,
            ny,
            nz,
            nc_back,
            nc_left,
            nc_right,
            nc_front,
            nc_floor,
            total_columns,
            tank_lo: occ.tank_lo,
            tank_hi: occ.tank_hi,
        }
    }

    /// Rebind the occ_buf reference (called on recreate_fluid when new occ is built).
    pub fn rebind_occ(&mut self, device: &wgpu::Device, occ: &WallOccupancySystem) {
        self.bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("wallfill fill bg"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.fill_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: occ.occ_buf.as_entire_binding(),
                },
            ],
        });
        // Update stored dims from new occ.
        let (nx, ny, nz) = occ.dims();
        let (nc_back, nc_left, nc_right, nc_front, nc_floor) = occ.column_counts();
        self.nx = nx;
        self.ny = ny;
        self.nz = nz;
        self.nc_back = nc_back;
        self.nc_left = nc_left;
        self.nc_right = nc_right;
        self.nc_front = nc_front;
        self.nc_floor = nc_floor;
        self.total_columns = occ.total_columns();
        self.tank_lo = occ.tank_lo;
        self.tank_hi = occ.tank_hi;
    }

    /// Push per-frame camera + fill parameters to the uniform buffer.
    pub fn set_camera(
        &self,
        queue: &wgpu::Queue,
        eye_to_world: &Mat4,
        eye_world_local: Vec3,
        box_rot: Mat3,
        width: u32,
        height: u32,
        hero: &HeroParams,
    ) {
        let fu = FillUniform {
            fill: [
                if hero.flat_water_fill_enabled {
                    1.0
                } else {
                    0.0
                },
                hero.flat_water_fill_strength.clamp(0.0, 1.0),
                hero.flat_water_fill_slab.max(0.0),
                hero.flat_water_waterline_softness.max(0.001),
            ],
            dims: [self.nx, self.ny, self.nz, self.total_columns],
            nc: [self.nc_back, self.nc_left, self.nc_right, self.nc_front],
            nc_floor: [self.nc_floor, 0, 0, 0],
            cam_params: [
                (50.0_f32.to_radians() * 0.5).tan(),
                width.max(1) as f32,
                height.max(1) as f32,
                0.0,
            ],
            tank_lo: [self.tank_lo[0], self.tank_lo[1], self.tank_lo[2], 0.0],
            tank_hi: [self.tank_hi[0], self.tank_hi[1], self.tank_hi[2], 0.0],
            box_eye_local: [eye_world_local.x, eye_world_local.y, eye_world_local.z, 0.0],
            box_rot_col0: [box_rot.x_axis.x, box_rot.x_axis.y, box_rot.x_axis.z, 0.0],
            box_rot_col1: [box_rot.y_axis.x, box_rot.y_axis.y, box_rot.y_axis.z, 0.0],
            box_rot_col2: [box_rot.z_axis.x, box_rot.z_axis.y, box_rot.z_axis.z, 0.0],
            eye_to_world: eye_to_world.to_cols_array_2d(),
            flat_epsilon: [hero.flat_water_epsilon.max(0.0), 0.0, 0.0, 0.0],
        };
        queue.write_buffer(&self.fill_buf, 0, bytemuck::bytes_of(&fu));
    }

    /// Record the fill pass into the existing MRT render pass.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}
