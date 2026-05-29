use glam::Vec3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    pub pos: Vec3,
    pub vel: Vec3,
    pub acc: Vec3,
    pub mass: f32,
    pub radius: f32,
}

impl Body {
    pub fn new(pos: Vec3, vel: Vec3, mass: f32, radius: f32) -> Self {
        Self {
            pos,
            vel,
            acc: Vec3::ZERO,
            mass,
            radius,
        }
    }

    #[allow(dead_code)]
    pub fn update(&mut self, dt: f32) {
        self.vel += self.acc * dt;
        self.pos += self.vel * dt;
    }
}
