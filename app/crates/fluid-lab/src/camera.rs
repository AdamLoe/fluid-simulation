//! Orbit camera. Yaw/pitch/distance around a fixed target (tank center).
//!
//! The view is built from an orientation quaternion (yaw about world-Y, then pitch
//! about local-X) rather than `look_at`, so pitch is UNCLAMPED — the camera can spin
//! fully over the top / underneath without the up-vector degeneracy a fixed-up
//! `look_at` hits at the poles.

use glam::{Mat4, Quat, Vec3};

pub struct OrbitCamera {
    target: Vec3,
    distance: f32,
    yaw: f32,   // radians, around +Y (unbounded)
    pitch: f32, // radians, around local X (unbounded — full freedom)
    roll: f32,  // radians, around local Z
    fov_y: f32,
}

impl OrbitCamera {
    pub fn new() -> Self {
        Self {
            target: Vec3::ZERO,
            distance: 6.0,
            yaw: 0.6,
            pitch: 0.4,
            roll: 0.0,
            fov_y: 50f32.to_radians(),
        }
    }

    pub fn set_yaw(&mut self, yaw: f32) {
        self.yaw = yaw;
    }

    pub fn set_pitch(&mut self, pitch: f32) {
        self.pitch = pitch;
    }

    pub fn set_roll(&mut self, roll: f32) {
        self.roll = roll;
    }

    pub fn set_distance(&mut self, d: f32) {
        self.distance = d.clamp(2.0, 40.0);
    }

    /// Drag deltas in pixels → orbit. No pitch clamp: the camera turns as far as the
    /// user keeps dragging, in either axis.
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        const SENS: f32 = 0.005;
        self.yaw -= dx * SENS;
        self.pitch -= dy * SENS;
    }

    /// Alternate drag: twist around the view direction, with vertical drag still
    /// pitching so right-drag has two-axis feedback.
    pub fn twist(&mut self, dx: f32, dy: f32) {
        const SENS: f32 = 0.005;
        self.roll += dx * SENS;
        self.pitch -= dy * SENS;
    }

    /// Move the orbit target in the camera screen plane.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        let (right, up) = self.billboard_basis();
        let scale = (self.distance * 0.0015).clamp(0.003, 0.06);
        self.target -= right * (dx * scale);
        self.target += up * (dy * scale);
    }

    /// Positive delta zooms out, negative zooms in (matches wheel deltaY).
    pub fn zoom(&mut self, delta: f32) {
        let factor = (1.0 + delta * 0.001).clamp(0.5, 2.0);
        self.distance = (self.distance * factor).clamp(2.0, 40.0);
    }

    /// Camera orientation: yaw about world-Y, then pitch about local-X, then roll about local-Z.
    fn orientation(&self) -> Quat {
        Quat::from_rotation_y(self.yaw)
            * Quat::from_rotation_x(self.pitch)
            * Quat::from_rotation_z(self.roll)
    }

    fn forward(&self) -> Vec3 {
        self.orientation() * Vec3::NEG_Z
    }

    /// World-space camera eye position (orbit target minus forward·distance).
    pub fn eye(&self) -> Vec3 {
        self.target - self.forward() * self.distance
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let o = self.orientation();
        // up rotates with the camera, so look_to_rh never degenerates at the poles.
        let view = Mat4::look_to_rh(self.eye(), o * Vec3::NEG_Z, o * Vec3::Y);
        // perspective_rh produces 0..1 clip depth, which is what WebGPU expects.
        let proj = Mat4::perspective_rh(self.fov_y, aspect.max(0.01), 0.1, 100.0);
        proj * view
    }

    /// Camera-facing right/up unit vectors, used for world-space particle billboards
    /// and for camera-relative box rotation/translation.
    pub fn billboard_basis(&self) -> (Vec3, Vec3) {
        let o = self.orientation();
        (o * Vec3::X, o * Vec3::Y)
    }

    /// Camera view direction (unit), used as the roll axis for box rotation.
    pub fn view_dir(&self) -> Vec3 {
        self.forward()
    }
}
