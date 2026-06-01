use crate::body::Body;
use glam::Vec3;

#[derive(Clone, Copy, Debug)]
pub struct Bounds3D {
    pub center: Vec3,
    pub size: f32,
}

impl Bounds3D {
    pub fn new_containing(bodies: &[Body]) -> Self {
        if bodies.is_empty() {
            return Self {
                center: Vec3::ZERO,
                size: 0.0,
            };
        }

        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut min_z = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;
        let mut max_z = f32::MIN;

        for body in bodies {
            min_x = min_x.min(body.pos.x);
            min_y = min_y.min(body.pos.y);
            min_z = min_z.min(body.pos.z);
            max_x = max_x.max(body.pos.x);
            max_y = max_y.max(body.pos.y);
            max_z = max_z.max(body.pos.z);
        }

        let center = Vec3::new(min_x + max_x, min_y + max_y, min_z + max_z) * 0.5;
        let size = (max_x - min_x).max(max_y - min_y).max(max_z - min_z);

        Self { center, size }
    }

    pub fn find_octant(&self, pos: Vec3) -> usize {
        ((pos.z > self.center.z) as usize) << 2
            | ((pos.y > self.center.y) as usize) << 1
            | ((pos.x > self.center.x) as usize)
    }

    pub fn into_octant(mut self, octant: usize) -> Self {
        self.size *= 0.5;
        self.center.x += ((octant & 1) as f32 - 0.5) * self.size;
        self.center.y += (((octant >> 1) & 1) as f32 - 0.5) * self.size;
        self.center.z += (((octant >> 2) & 1) as f32 - 0.5) * self.size;
        self
    }

    pub fn subdivide(&self) -> [Bounds3D; 8] {
        [0, 1, 2, 3, 4, 5, 6, 7].map(|i| self.into_octant(i))
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    pub children: usize,
    pub next: usize,
    pub pos: Vec3,
    pub mass: f32,
    pub bounds: Bounds3D,
}

impl Node {
    pub fn new(next: usize, bounds: Bounds3D) -> Self {
        Self {
            children: 0,
            next,
            pos: Vec3::ZERO,
            mass: 0.0,
            bounds,
        }
    }

    pub fn is_leaf(&self) -> bool {
        self.children == 0
    }

    pub fn is_branch(&self) -> bool {
        self.children != 0
    }

    pub fn is_empty(&self) -> bool {
        self.mass == 0.0
    }
}

pub struct Octree {
    pub t_sq: f32,
    pub e_sq: f32,
    pub nodes: Vec<Node>,
    pub parents: Vec<usize>,
}

impl Octree {
    pub const ROOT: usize = 0;

    pub fn new(theta: f32, epsilon: f32) -> Self {
        Self {
            t_sq: theta * theta,
            e_sq: epsilon * epsilon,
            nodes: Vec::new(),
            parents: Vec::new(),
        }
    }

    pub fn clear(&mut self, bounds: Bounds3D) {
        self.nodes.clear();
        self.parents.clear();
        self.nodes.push(Node::new(0, bounds));
    }

    fn subdivide(&mut self, node: usize) -> usize {
        self.parents.push(node);
        let children = self.nodes.len();
        self.nodes[node].children = children;

        let nexts = [
            children + 1,
            children + 2,
            children + 3,
            children + 4,
            children + 5,
            children + 6,
            children + 7,
            self.nodes[node].next,
        ];
        let bounds = self.nodes[node].bounds.subdivide();
        for i in 0..8 {
            self.nodes.push(Node::new(nexts[i], bounds[i]));
        }

        children
    }

    pub fn insert(&mut self, pos: Vec3, mass: f32) {
        let mut node = Self::ROOT;

        while self.nodes[node].is_branch() {
            let octant = self.nodes[node].bounds.find_octant(pos);
            node = self.nodes[node].children + octant;
        }

        if self.nodes[node].is_empty() {
            self.nodes[node].pos = pos;
            self.nodes[node].mass = mass;
            return;
        }

        let (p, m) = (self.nodes[node].pos, self.nodes[node].mass);

        // Float Drift Koruması: İki obje mikroskobik olarak aynı yerdeyse,
        // ağacı sonsuza kadar bölmek yerine kütlelerini aynı düğümde birleştir.
        if pos.distance_squared(p) < 1e-6 {
            self.nodes[node].mass += mass;
            return;
        }

        loop {
            let children = self.subdivide(node);

            let o1 = self.nodes[node].bounds.find_octant(p);
            let o2 = self.nodes[node].bounds.find_octant(pos);

            if o1 == o2 {
                node = children + o1;
            } else {
                let n1 = children + o1;
                let n2 = children + o2;

                self.nodes[n1].pos = p;
                self.nodes[n1].mass = m;
                self.nodes[n2].pos = pos;
                self.nodes[n2].mass = mass;
                return;
            }
        }
    }

    pub fn propagate(&mut self) {
        for &node in self.parents.iter().rev() {
            let i = self.nodes[node].children;

            let mut total_pos = Vec3::ZERO;
            let mut total_mass = 0.0;
            for offset in 0..8 {
                let child_pos = self.nodes[i + offset].pos;
                let child_mass = self.nodes[i + offset].mass;
                total_pos += child_pos * child_mass;
                total_mass += child_mass;
            }

            self.nodes[node].pos = if total_mass > 0.0 {
                total_pos / total_mass
            } else {
                Vec3::ZERO
            };
            self.nodes[node].mass = total_mass;
        }
    }

    pub fn acc(&self, pos: Vec3) -> Vec3 {
        let mut acc = Vec3::ZERO;
        if self.nodes.is_empty() {
            return acc;
        }

        let mut node = Self::ROOT;
        loop {
            let n = &self.nodes[node];

            // ALTIN VURUŞ: Eğer düğüm tamamen boşsa, vektör matematiğine
            // hiç girmeden doğrudan bir sonraki düğüme (next) atla.
            if n.mass == 0.0 {
                if n.next == 0 {
                    break;
                }
                node = n.next;
                continue;
            }

            let d = n.pos - pos;
            let d_sq = d.length_squared();

            if n.is_leaf() || n.bounds.size * n.bounds.size < d_sq * self.t_sq {
                if d_sq > 0.0 {
                    let inv_d = d_sq.sqrt().recip();
                    let inv_denom = (d_sq + self.e_sq).recip() * inv_d;
                    acc += d * (n.mass * inv_denom).min(f32::MAX);
                }

                if n.next == 0 {
                    break;
                }
                node = n.next;
            } else {
                node = n.children;
            }
        }

        acc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bounds_3d() {
        let bodies = vec![
            Body::new(Vec3::new(1.0, 2.0, 3.0), Vec3::ZERO, 1.0, 1.0),
            Body::new(Vec3::new(5.0, -2.0, 10.0), Vec3::ZERO, 1.0, 1.0),
        ];

        let bounds = Bounds3D::new_containing(&bodies);
        assert!((bounds.center.x - 3.0).abs() < 1e-5);
        assert!((bounds.center.y - 0.0).abs() < 1e-5);
        assert!((bounds.center.z - 6.5).abs() < 1e-5);
        assert!((bounds.size - 7.0).abs() < 1e-5);
    }

    #[test]
    fn test_center_of_mass() {
        let bodies = vec![
            Body::new(Vec3::new(2.0, 0.0, 0.0), Vec3::ZERO, 10.0, 1.0),
            Body::new(Vec3::new(-2.0, 0.0, 0.0), Vec3::ZERO, 30.0, 1.0),
        ];

        let bounds = Bounds3D::new_containing(&bodies);
        let mut octree = Octree::new(0.5, 0.1);
        octree.clear(bounds);
        for body in &bodies {
            octree.insert(body.pos, body.mass);
        }
        octree.propagate();

        let root = &octree.nodes[Octree::ROOT];
        // Center of mass should be (-1, 0, 0)
        assert!((root.pos.x - -1.0).abs() < 1e-5);
        assert!((root.pos.y - 0.0).abs() < 1e-5);
        assert!((root.pos.z - 0.0).abs() < 1e-5);
        assert_eq!(root.mass, 40.0);
    }
}
