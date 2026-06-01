use crate::config::*;
use bevy::{
    camera_controller::free_camera::{FreeCamera, FreeCameraPlugin},
    core_pipeline::tonemapping::Tonemapping,
    post_process::bloom::Bloom,
    prelude::*,
    window::WindowMode,
};
use rand::RngExt;
use bevy::platform::collections::HashMap;
use std::f32::consts::PI;

mod body;
mod config;
mod octree;
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(Component, Debug)]
pub struct Position(pub Vec3);

#[derive(Component, Debug)]
pub struct Velocity(pub Vec3);

#[derive(Component, Debug)]
pub struct Mass(pub f32);

#[derive(Component, Debug)]
pub struct Radius(pub f32);

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::BLACK))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                resizable: false,
                mode: WindowMode::BorderlessFullscreen(MonitorSelection::Primary),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(FreeCameraPlugin)
        .add_systems(Startup, (setup_camera, spawn_lights, spawn_bodies))
        .add_systems(Update, (update_physics, handle_acceleration).chain())
        .run();
}

fn setup_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 0.0, 150.0).looking_at(Vec3::ZERO, Vec3::Y),
        Tonemapping::TonyMcMapface,
        Bloom::NATURAL,
        FreeCamera {
            sensitivity: 0.2,
            friction: 25.0,
            walk_speed: 100.0,
            run_speed: 250.0,
            key_up: KeyCode::Space,
            key_down: KeyCode::ShiftLeft,
            key_run: KeyCode::ControlLeft,
            mouse_key_cursor_grab: MouseButton::Other(9999),
            keyboard_key_toggle_cursor_grab: KeyCode::Escape,
            ..default()
        },
    ));
}

fn spawn_lights(mut commands: Commands) {
    commands.spawn((
        AmbientLight {
            color: Color::srgb(1.0, 1.0, 1.0),
            brightness: 200.0,
            affects_lightmapped_meshes: false,
        },
        Transform::from_xyz(0.0, 2.0, 0.0),
    ));
}

fn spawn_bodies(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut time: ResMut<Time<Virtual>>,
) {
    let mut rng = rand::rng();
    time.set_relative_speed(SIMULATION_SPEED_FACTOR);

    let base_mesh = meshes.add(Sphere::new(1.0).mesh().ico(4).unwrap());

    // Pre-built material pool (palette)
    // Only these 256 materials will be sent to the GPU, thousands of objects will share them.
    let palette_size = 256;
    let mut material_palette = Vec::with_capacity(palette_size);
    for i in 0..palette_size {
        // Convert i (0..255) value to intensity (0.1..1.0) range
        let t = i as f32 / (palette_size - 1) as f32;
        let intensity = 0.1 + t * 0.9;

        let color = Color::hsl(30.0 + intensity * 40.0, 0.9, 0.4 + intensity * 0.3);
        let mut emissive_color: LinearRgba =
            LinearRgba::rgb(120.0 / intensity, 55.0 / intensity, intensity * 20.0);
        if intensity > 0.98 {
            emissive_color = LinearRgba::rgb(100.0, 100.0, 100.0);
        } else if intensity > 0.8 {
            emissive_color = LinearRgba::rgb(25.0 / intensity, 5.0 * intensity, intensity * 200.0);
        } else if intensity < 0.12 {
            emissive_color =
                LinearRgba::rgb(100.0 / intensity, 25.0 * intensity, intensity * 200.0);
        }

        material_palette.push(materials.add(StandardMaterial {
            base_color: color,
            emissive: emissive_color,
            metallic: 0.2,
            perceptual_roughness: 0.5,
            ..default()
        }));
    }

    // Spawn the Objects
    for _ in 0..BODY_COUNT {
        let theta = rng.random_range(0.0..2.0) * PI;
        let phi = rng.random_range(0.0..1.0) * PI;
        let dist = if BODY_POS_RANGE > 0.0 {
            BODY_POS_RANGE * rng.random_range(0.0..1.0_f32).cbrt()
        } else {
            0.0
        };
        let (sint, cost) = theta.sin_cos();
        let (sinp, cosp) = phi.sin_cos();
        let pos = Vec3::new(dist * sinp * cost, dist * sinp * sint, dist * cosp);

        // Orbital-like velocities or random expansion velocities
        let vel = if BODY_VEL_RANGE > 0.0 {
            Vec3::new(
                rng.random_range(-BODY_VEL_RANGE..BODY_VEL_RANGE),
                rng.random_range(-BODY_VEL_RANGE..BODY_VEL_RANGE),
                rng.random_range(-BODY_VEL_RANGE..BODY_VEL_RANGE),
            )
        } else {
            Vec3::ZERO
        };
        let mass: f32 = if BODY_MASS_RANGE[0] < BODY_MASS_RANGE[1] {
            rng.random_range(BODY_MASS_RANGE[0]..BODY_MASS_RANGE[1])
        } else {
            BODY_MASS_RANGE[0]
        };

        // harmonized color based on mass (heavier bodies are hotter/brighter)
        let intensity = (mass / 1000.0).clamp(0.1, 1.0);

        // Convert Intensity value to pool index in [0, 255] range
        let t = (intensity - 0.1) / 0.9;
        let palette_index =
            ((t * (palette_size - 1) as f32).round() as usize).min(palette_size - 1);
        let sphere_material = material_palette[palette_index].clone();

        let volume_factor = mass / 100.0;
        let radius = volume_factor.cbrt().max(BODY_MESH_RADIUS);

        commands.spawn((
            Mesh3d(base_mesh.clone()),
            MeshMaterial3d(sphere_material),
            Transform::from_translation(pos).with_scale(Vec3::splat(radius)),
            Position(pos),
            Velocity(vel),
            Mass(mass),
            Radius(radius),
        ));
    }
}

fn update_physics(
    time: Res<Time>,
    mut local_octree: Local<Option<octree::Octree>>,
    mut local_bodies: Local<Vec<body::Body>>,
    mut query: Query<(&mut Position, &mut Velocity, &Mass, &Radius, &mut Transform)>,
) {
    let dt = time.delta_secs().min(0.03);

    if local_octree.is_none() {
        *local_octree = Some(octree::Octree::new(0.5, 2.0));
    }
    let octree = local_octree.as_mut().unwrap();

    let bodies = &mut *local_bodies;
    bodies.clear();
    for (pos, vel, mass, radius, _) in query.iter() {
        bodies.push(body::Body::new(pos.0, vel.0, mass.0, radius.0));
    }

    if bodies.is_empty() {
        return;
    }

    let bounds = octree::Bounds3D::new_containing(bodies);
    octree.clear(bounds);

    // Single Core Build
    for body in bodies.iter() {
        octree.insert(body.pos, body.mass);
    }
    octree.propagate();

    // Rayon Parallel Gravity Calculation
    let octree_ref = &*octree;
    query
        .par_iter_mut()
        .for_each(|(mut pos, mut vel, _mass, _radius, mut transform)| {
            let acc = octree_ref.acc(pos.0);
            vel.0 += acc * dt;
            pos.0 += vel.0 * dt;
            transform.translation = pos.0;
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

struct AccelerationCache {
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

fn handle_acceleration(
    mut commands: Commands,
    mut query: Query<(
        Entity,
        &mut Position,
        &mut Velocity,
        &mut Mass,
        &mut Radius,
        &mut Transform,
    )>,
    mut cache: Local<AccelerationCache>,
    mut prev_max_radius: Local<f32>,
) {
    let cache = &mut *cache;

    // Dynamic Cell size calculation (TUNNELING PREVENTION) using previous frame's max radius
    let cell_size = (*prev_max_radius * 2.2).max(10.0);

    // Copy all entity data to a temporary vector for reading and precompute cells.
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

    // Union-Find series: Everyone is initially their own parent.
    let parent = &mut cache.parent;
    parent.clear();
    for i in 0..n {
        parent.push(i);
    }

    // Union-Find helper func
    fn find(i: usize, parent: &mut [usize]) -> usize {
        if parent[i] == i {
            i
        } else {
            let root = find(parent[i], parent);
            parent[i] = root; // Path compression
            root
        }
    }

    // --- SPATIAL HASHING (FLAT ARRAY / LINKED LIST GRID) ---

    // Reset head array
    let head = &mut cache.head;
    head.fill(usize::MAX);

    // Resize next array to fit current n elements
    let next = &mut cache.next;
    next.clear();
    next.resize(n, usize::MAX);

    // Place all bodies into the Flat Array Hash Grid O(N)
    for i in 0..n {
        let cell = bodies[i].5;
        let hash_idx = hash_cell(cell) & (HASH_SIZE - 1);
        next[i] = head[hash_idx];
        head[hash_idx] = i;
    }

    // Only check 27 neighboring cells for intersection O(N)
    for i in 0..n {
        let p1 = bodies[i].1;
        let r1 = bodies[i].4;
        let cell = bodies[i].5;

        let cell_x = cell.0;
        let cell_y = cell.1;
        let cell_z = cell.2;

        // Check your own cell and 26 neighboring cells
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let neighbor_cell = (cell_x + dx, cell_y + dy, cell_z + dz);
                    let hash_idx = hash_cell(neighbor_cell) & (HASH_SIZE - 1);

                    let mut j = head[hash_idx];
                    while j != usize::MAX {
                        // Prevent checking duplicate pairs or self-checking
                        if i < j {
                            let cell_j = bodies[j].5;
                            // Resolve hash collisions: make sure body j is in the exact neighbor cell we are querying
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
    // --- SPATIAL HASHING BİTİŞ ---

    // Calculation of the total mass and momentum of the clusters.
    // Key: Root Index, Value: (Total Mass, Total Momentum (Mass * Vel), Center of Mass (Mass * Pos))
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

                    t.translation = new_pos;
                    t.scale = Vec3::splat(new_radius);
                }
            }
        }
    }
}
