use bevy::prelude::*;
use bevy::platform::collections::HashMap;
use crate::octree::*;
use crate::body::*;

#[derive(Component, Debug)]
pub struct Position(pub Vec3);

#[derive(Component, Debug)]
pub struct Velocity(pub Vec3);

#[derive(Component, Debug)]
pub struct Mass(pub f32);

#[derive(Component, Debug)]
pub struct Radius(pub f32);

#[derive(Resource, Default)]
pub struct PhysicsOctree(pub Option<Octree>);

pub fn update_physics(
    time: Res<Time<Virtual>>,
    mut global_octree: ResMut<PhysicsOctree>,
    mut local_bodies: Local<Vec<Body>>,
    mut query: Query<(
        &mut Position,
        &mut Velocity,
        &Mass,
        &Radius,
        Option<&mut Transform>,
    )>,
) {
    if time.is_paused() {
        return;
    }

    let dt = time.delta_secs().min(0.03);

    if global_octree.0.is_none() {
        global_octree.0 = Some(Octree::new(0.5, 2.0));
    }
    let octree = global_octree.0.as_mut().unwrap();

    let bodies = &mut *local_bodies;
    bodies.clear();
    for (pos, vel, mass, radius, _) in query.iter() {
        bodies.push(Body::new(pos.0, vel.0, mass.0, radius.0));
    }

    if bodies.is_empty() {
        return;
    }

    let bounds = Bounds3D::new_containing(bodies);
    octree.clear(bounds);

    for body in bodies.iter() {
        octree.insert(body.pos, body.mass);
    }
    octree.propagate();

    let octree_ref = &*octree;
    query
        .par_iter_mut()
        .for_each(|(mut pos, mut vel, _mass, _radius, mut transform)| {
            let acc = octree_ref.acc(pos.0);
            vel.0 += acc * dt;
            pos.0 += vel.0 * dt;
            if let Some(ref mut t) = transform {
                t.translation = pos.0;
            }
        });
}

const HASH_SIZE: usize = 131072;

#[inline]
fn hash_cell(cell: (i32, i32, i32)) -> usize {
    let x = cell.0 as u32;
    let y = cell.1 as u32;
    let z = cell.2 as u32;
    (x.wrapping_mul(73856093) ^ y.wrapping_mul(19349663) ^ z.wrapping_mul(83492791)) as usize
}

pub struct AccelerationCache {
    bodies: Vec<(Entity, Vec3, Vec3, f32, f32, (i32, i32, i32))>,
    parent: Vec<usize>,
    head: Vec<usize>,
    next: Vec<usize>,
    cluster_data: HashMap<usize, (f32, Vec3, Vec3)>,
}

impl Default for AccelerationCache {
    fn default() -> Self {
        Self {
            bodies: Vec::new(),
            parent: Vec::new(),
            head: vec![usize::MAX; HASH_SIZE],
            next: Vec::new(),
            cluster_data: HashMap::new(),
        }
    }
}

pub fn handle_acceleration(
    mut commands: Commands,
    mut query: Query<(
        Entity,
        &mut Position,
        &mut Velocity,
        &mut Mass,
        &mut Radius,
        Option<&mut Transform>,
    )>,
    mut cache: Local<AccelerationCache>,
    mut prev_max_radius: Local<f32>,
    time: Res<Time<Virtual>>,
) {
    if time.is_paused() {
        return;
    }

    let cache = &mut *cache;

    let cell_size = (*prev_max_radius * 2.2).max(10.0);

    let bodies = &mut cache.bodies;
    bodies.clear();
    let mut next_max_radius = 0.0f32;
    for (entity, pos, vel, mass, radius, _) in query.iter() {
        let r = radius.0;
        next_max_radius = next_max_radius.max(r);
        let cell = (
            (pos.0.x / cell_size).floor() as i32,
            (pos.0.y / cell_size).floor() as i32,
            (pos.0.z / cell_size).floor() as i32,
        );
        bodies.push((entity, pos.0, vel.0, mass.0, r, cell));
    }
    *prev_max_radius = next_max_radius;

    let n = bodies.len();
    if n < 2 {
        return;
    }

    let parent = &mut cache.parent;
    parent.clear();
    for i in 0..n {
        parent.push(i);
    }

    fn find(i: usize, parent: &mut [usize]) -> usize {
        if parent[i] == i {
            i
        } else {
            let root = find(parent[i], parent);
            parent[i] = root;
            root
        }
    }

    let head = &mut cache.head;
    head.fill(usize::MAX);

    let next = &mut cache.next;
    next.clear();

    for i in 0..n {
        let cell = bodies[i].5;
        let hash_idx = hash_cell(cell) & (HASH_SIZE - 1);
        next.push(head[hash_idx]);
        head[hash_idx] = i;
    }

    for i in 0..n {
        let p1 = bodies[i].1;
        let r1 = bodies[i].4;
        let cell = bodies[i].5;

        let cell_x = cell.0;
        let cell_y = cell.1;
        let cell_z = cell.2;

        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let neighbor_cell = (cell_x + dx, cell_y + dy, cell_z + dz);
                    let hash_idx = hash_cell(neighbor_cell) & (HASH_SIZE - 1);

                    let mut j = head[hash_idx];
                    while j != usize::MAX {
                        if i < j {
                            let cell_j = bodies[j].5;
                            if cell_j == neighbor_cell {
                                let p2 = bodies[j].1;
                                let r2 = bodies[j].4;

                                let d_sq = (p1 - p2).length_squared();
                                let r_sum = r1 + r2;

                                if d_sq < r_sum * r_sum {
                                    let root_i = find(i, parent);
                                    let root_j = find(j, parent);
                                    if root_i != root_j {
                                        if bodies[root_i].3 >= bodies[root_j].3 {
                                            parent[root_j] = root_i;
                                        } else {
                                            parent[root_i] = root_j;
                                        }
                                    }
                                }
                            }
                        }
                        j = next[j];
                    }
                }
            }
        }
    }

    let cluster_data = &mut cache.cluster_data;
    cluster_data.clear();

    for i in 0..n {
        let root = find(i, parent);
        let mass = bodies[i].3;
        let pos = bodies[i].1;
        let vel = bodies[i].2;

        let momentum = vel * mass;
        let weighted_pos = pos * mass;

        let entry = cluster_data
            .entry(root)
            .or_insert((0.0, Vec3::ZERO, Vec3::ZERO));
        entry.0 += mass;
        entry.1 += momentum;
        entry.2 += weighted_pos;
    }

    for i in 0..n {
        let root = find(i, parent);
        let entity = bodies[i].0;

        if i != root {
            commands.entity(entity).despawn();
        } else if let Some(&(total_mass, total_momentum, total_weighted_pos)) =
            cluster_data.get(&root)
        {
            if total_mass > bodies[i].3 {
                if let Ok((_, mut p, mut v, mut m, mut r, mut t)) = query.get_mut(entity) {
                    let new_vel = total_momentum / total_mass;
                    let new_pos = total_weighted_pos / total_mass;
                    let volume_factor = total_mass / 100.0;
                    let new_radius = volume_factor.cbrt().max(0.4);

                    p.0 = new_pos;
                    v.0 = new_vel;
                    m.0 = total_mass;
                    r.0 = new_radius;

                    if let Some(ref mut t) = t {
                        t.translation = new_pos;
                        t.scale = Vec3::splat(new_radius);
                    }
                }
            }
        }
    }
}
