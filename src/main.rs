use crate::config::*;
use bevy::{
    camera_controller::free_camera::{FreeCamera, FreeCameraPlugin},
    core_pipeline::tonemapping::Tonemapping,
    post_process::bloom::Bloom,
    prelude::*,
    window::WindowMode,
    time::TimeUpdateStrategy,
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

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    #[default]
    Init,
    Simulate,
    Evaluate,
    Mutate,
}

#[derive(Clone, Copy, Debug)]
pub struct Genome {
    pub pos_range: f32,
    pub vel_range: f32,
    pub mass_max: f32,
}

#[derive(Resource)]
pub struct GeneticEngine {
    pub generation: usize,
    pub current_tick: usize,
    pub max_ticks: usize,
    pub current_genome: Genome,
    pub best_genome: Genome,
    pub best_fitness: f32,
    pub initial_body_count: usize,
    pub initial_rms_radius: f32,
}

impl Default for GeneticEngine {
    fn default() -> Self {
        let initial_genome = Genome {
            pos_range: BODY_POS_RANGE,
            vel_range: BODY_VEL_RANGE,
            mass_max: BODY_MASS_RANGE[1],
        };
        Self {
            generation: 1,
            current_tick: 0,
            max_ticks: 1500, // 1500 ticks per epoch
            current_genome: initial_genome,
            best_genome: initial_genome,
            best_fitness: -1.0,
            initial_body_count: BODY_COUNT as usize,
            initial_rms_radius: 1.0,
        }
    }
}

fn main() {
    let is_headless = std::env::args().any(|arg| arg == "--train");

    let mut app = App::new();

    if is_headless {
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::state::app::StatesPlugin)
            .add_plugins(bevy::log::LogPlugin::default())
            .insert_resource(TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f32(0.016)));
    } else {
        app.insert_resource(ClearColor(Color::BLACK))
            .add_plugins(DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    resizable: false,
                    mode: WindowMode::BorderlessFullscreen(MonitorSelection::Primary),
                    ..default()
                }),
                ..default()
            }))
            .add_plugins(FreeCameraPlugin)
            .add_systems(Startup, (setup_camera, spawn_lights));
    }

    app.init_resource::<GeneticEngine>()
        .init_state::<AppState>()
        // App State systems
        .add_systems(OnEnter(AppState::Init), init_population)
        .add_systems(Update, track_simulation.run_if(in_state(AppState::Simulate)))
        .add_systems(OnEnter(AppState::Evaluate), evaluate_generation)
        .add_systems(OnEnter(AppState::Mutate), mutate_generation)
        // Physics updates run in Simulate state
        .add_systems(
            Update,
            (update_physics, handle_acceleration)
                .chain()
                .run_if(in_state(AppState::Simulate)),
        )
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

fn init_population(
    mut commands: Commands,
    query: Query<Entity, With<Position>>,
    mut engine: ResMut<GeneticEngine>,
    mut next_state: ResMut<NextState<AppState>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    materials: Option<ResMut<Assets<StandardMaterial>>>,
    mut time: ResMut<Time<Virtual>>,
) {
    // 1. Despawn all existing bodies
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }

    // 2. Reset tick count
    engine.current_tick = 0;

    let genome = engine.current_genome;
    let mut rng = rand::rng();
    time.set_relative_speed(SIMULATION_SPEED_FACTOR);

    // CRITICAL: Request assets as Option to prevent crash on startup under MinimalPlugins
    let is_visual = meshes.is_some() && materials.is_some();

    let mut base_mesh = None;
    let mut material_palette = Vec::new();
    let palette_size = 256;

    if is_visual {
        let mut meshes = meshes.unwrap();
        let mut materials = materials.unwrap();
        base_mesh = Some(meshes.add(Sphere::new(1.0).mesh().ico(4).unwrap()));

        for i in 0..palette_size {
            let t = i as f32 / (palette_size - 1) as f32;
            let intensity = 0.1 + t * 0.9;
            let color = Color::hsl(30.0 + intensity * 40.0, 0.9, 0.4 + intensity * 0.3);
            let mut emissive_color = LinearRgba::rgb(120.0 / intensity, 55.0 / intensity, intensity * 20.0);
            if intensity > 0.98 {
                emissive_color = LinearRgba::rgb(100.0, 100.0, 100.0);
            } else if intensity > 0.8 {
                emissive_color = LinearRgba::rgb(25.0 / intensity, 5.0 * intensity, intensity * 200.0);
            } else if intensity < 0.12 {
                emissive_color = LinearRgba::rgb(100.0 / intensity, 25.0 * intensity, intensity * 200.0);
            }

            material_palette.push(materials.add(StandardMaterial {
                base_color: color,
                emissive: emissive_color,
                metallic: 0.2,
                perceptual_roughness: 0.5,
                ..default()
            }));
        }
    }

    let mut sum_sq_dist = 0.0;

    for _ in 0..BODY_COUNT {
        let theta = rng.random_range(0.0..2.0) * PI;
        let phi = rng.random_range(0.0..1.0) * PI;
        let dist = if genome.pos_range > 0.0 {
            genome.pos_range * rng.random_range(0.0..1.0_f32).cbrt()
        } else {
            0.0
        };
        let (sint, cost) = theta.sin_cos();
        let (sinp, cosp) = phi.sin_cos();
        let pos = Vec3::new(dist * sinp * cost, dist * sinp * sint, dist * cosp);

        sum_sq_dist += pos.length_squared();

        let vel = if genome.vel_range > 0.0 {
            Vec3::new(
                rng.random_range(-genome.vel_range..genome.vel_range),
                rng.random_range(-genome.vel_range..genome.vel_range),
                rng.random_range(-genome.vel_range..genome.vel_range),
            )
        } else {
            Vec3::ZERO
        };

        let mass_min = BODY_MASS_RANGE[0];
        let mass = if mass_min < genome.mass_max {
            rng.random_range(mass_min..genome.mass_max)
        } else {
            mass_min
        };

        let volume_factor = mass / 100.0;
        let radius = volume_factor.cbrt().max(BODY_MESH_RADIUS);

        if is_visual {
            let intensity = (mass / 1000.0).clamp(0.1, 1.0);
            let t = (intensity - 0.1) / 0.9;
            let palette_index = ((t * (palette_size - 1) as f32).round() as usize).min(palette_size - 1);
            let sphere_material = material_palette[palette_index].clone();

            commands.spawn((
                Mesh3d(base_mesh.as_ref().unwrap().clone()),
                MeshMaterial3d(sphere_material),
                Transform::from_translation(pos).with_scale(Vec3::splat(radius)),
                Position(pos),
                Velocity(vel),
                Mass(mass),
                Radius(radius),
            ));
        } else {
            commands.spawn((
                Position(pos),
                Velocity(vel),
                Mass(mass),
                Radius(radius),
            ));
        }
    }

    engine.initial_body_count = BODY_COUNT as usize;
    engine.initial_rms_radius = if BODY_COUNT > 0 {
        (sum_sq_dist / BODY_COUNT as f32).sqrt().max(1.0)
    } else {
        1.0
    };

    println!("--- [Generation {} Initialized] ---", engine.generation);
    println!("  [Params] pos_range: {:.2}, vel_range: {:.2}, mass_max: {:.2}", genome.pos_range, genome.vel_range, genome.mass_max);
    println!("  [Universe] bodies: {}, RMS Radius: {:.4}", engine.initial_body_count, engine.initial_rms_radius);

    next_state.set(AppState::Simulate);
}

fn track_simulation(
    mut engine: ResMut<GeneticEngine>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    engine.current_tick += 1;
    if engine.current_tick >= engine.max_ticks {
        next_state.set(AppState::Evaluate);
    }
}

fn evaluate_generation(
    query: Query<(&Position, &Velocity, &Mass)>,
    mut engine: ResMut<GeneticEngine>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let mut total_mass = 0.0f32;
    let mut max_mass = 0.0f32;
    let mut sum_sq_dist = 0.0f32;
    let mut sum_orbit_terms = 0.0f32;
    let current_body_count = query.iter().count();

    for (pos, vel, mass) in query.iter() {
        let m = mass.0;
        let p = pos.0;
        let v = vel.0;

        total_mass += m;
        if m > max_mass {
            max_mass = m;
        }

        sum_sq_dist += p.length_squared();

        let pos_len = p.length();
        let vel_len = v.length();
        // Handle zero lengths safely to completely prevent NaN
        if pos_len > 1e-6 && vel_len > 1e-6 {
            let norm_p = p / pos_len;
            let norm_v = v / vel_len;
            let dot_prod = norm_p.dot(norm_v).abs();
            sum_orbit_terms += m * dot_prod;
        }
    }

    // 1. S_mass
    let s_mass = if total_mass > 0.0 {
        let ratio = max_mass / total_mass;
        (1.0 - ratio.powi(2)).max(0.0)
    } else {
        0.0
    };

    // 2. S_orbit
    let s_orbit = if total_mass > 0.0 {
        (1.0 - (sum_orbit_terms / total_mass)).max(0.0)
    } else {
        0.0
    };

    // 3. S_contain
    let current_rms_radius = if current_body_count > 0 {
        (sum_sq_dist / current_body_count as f32).sqrt()
    } else {
        0.0
    };
    let initial_rms = if engine.initial_rms_radius > 0.0 {
        engine.initial_rms_radius
    } else {
        1.0
    };
    let s_contain = 1.0 / (1.0 + (current_rms_radius / initial_rms));

    // 4. S_survival
    let s_survival = if engine.initial_body_count > 0 {
        current_body_count as f32 / engine.initial_body_count as f32
    } else {
        0.0
    };

    // Multiplicative Dimensionless Fitness
    let fitness = s_mass * s_orbit * s_contain * s_survival;

    println!("=== [Evaluation of Gen {}] ===", engine.generation);
    println!("  Fitness: {:.6} (S_mass: {:.4}, S_orbit: {:.4}, S_contain: {:.4}, S_survival: {:.4})", fitness, s_mass, s_orbit, s_contain, s_survival);
    println!("  Active bodies: {} / {}", current_body_count, engine.initial_body_count);
    println!("  RMS Radius: {:.2} (Initial: {:.2})", current_rms_radius, engine.initial_rms_radius);

    if fitness > engine.best_fitness {
        engine.best_fitness = fitness;
        engine.best_genome = engine.current_genome;
        println!("  *** NEW BEST GENOME SET! ***");
    }
    println!("  [Best Fitness So Far] {:.6}", engine.best_fitness.max(fitness));

    next_state.set(AppState::Mutate);
}

fn mutate_generation(
    mut engine: ResMut<GeneticEngine>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let mut rng = rand::rng();

    // Uniform percentage mutations in range [-0.15, +0.15]
    let pos_mutation = 1.0 + rng.random_range(-0.15..0.15);
    let vel_mutation = 1.0 + rng.random_range(-0.15..0.15);
    let mass_mutation = 1.0 + rng.random_range(-0.15..0.15);

    let mut new_genome = engine.best_genome;
    new_genome.pos_range = (new_genome.pos_range * pos_mutation).clamp(10.0, 5000.0);
    new_genome.vel_range = (new_genome.vel_range * vel_mutation).clamp(0.0, 500.0);
    new_genome.mass_max = (new_genome.mass_max * mass_mutation).clamp(100.0, 10000.0);

    engine.current_genome = new_genome;
    engine.generation += 1;

    println!("--- [Applying Mutation to Best Genome for Gen {}] ---", engine.generation);
    println!("  Mutated pos_range: {:.2} -> {:.2}", engine.best_genome.pos_range, new_genome.pos_range);
    println!("  Mutated vel_range: {:.2} -> {:.2}", engine.best_genome.vel_range, new_genome.vel_range);
    println!("  Mutated mass_max:  {:.2} -> {:.2}", engine.best_genome.mass_max, new_genome.mass_max);

    next_state.set(AppState::Init);
}

fn update_physics(
    time: Res<Time>,
    mut local_octree: Local<Option<octree::Octree>>,
    mut local_bodies: Local<Vec<body::Body>>,
    mut query: Query<(&mut Position, &mut Velocity, &Mass, &Radius, Option<&mut Transform>)>,
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
        Option<&mut Transform>,
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

    // Reset next array
    let next = &mut cache.next;
    next.clear();

    // Place all bodies into the Flat Array Hash Grid O(N)
    for i in 0..n {
        let cell = bodies[i].5;
        let hash_idx = hash_cell(cell) & (HASH_SIZE - 1);
        next.push(head[hash_idx]);
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

                    if let Some(ref mut t) = t {
                        t.translation = new_pos;
                        t.scale = Vec3::splat(new_radius);
                    }
                }
            }
        }
    }
}
