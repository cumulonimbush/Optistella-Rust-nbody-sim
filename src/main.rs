use crate::config::*;
use bevy::platform::collections::HashMap;
use bevy::{
    camera_controller::free_camera::{FreeCamera, FreeCameraPlugin},
    core_pipeline::tonemapping::Tonemapping,
    post_process::bloom::Bloom,
    prelude::*,
    time::TimeUpdateStrategy,
    window::WindowMode,
};
use rand::RngExt;

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

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Genome {
    pub pos_range: f32,
    pub vel_variance: f32,
    pub orbital_spin: f32,
    pub mass_max: f32,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct SharedGenome {
    pub fitness: f32,
    pub genome: Genome,
}

fn load_best_genome() -> Option<(Genome, f32)> {
    if std::path::Path::new("best_genome.json").exists() {
        if let Ok(content) = std::fs::read_to_string("best_genome.json") {
            if let Ok(shared) = serde_json::from_str::<SharedGenome>(&content) {
                return Some((shared.genome, shared.fitness));
            }
            if let Ok(genome) = serde_json::from_str::<Genome>(&content) {
                return Some((genome, -1.0));
            }
        }
    }
    None
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
            vel_variance: BODY_VEL_RANGE,
            orbital_spin: 50.0,
            mass_max: BODY_MASS_RANGE[1],
        };
        Self {
            generation: 1,
            current_tick: 0,
            max_ticks: 4000,
            current_genome: initial_genome,
            best_genome: initial_genome,
            best_fitness: -1.0,
            initial_body_count: BODY_COUNT as usize,
            initial_rms_radius: 1.0,
        }
    }
}

#[derive(Resource, Default)]
pub struct PhysicsOctree(pub Option<octree::Octree>);

#[derive(Resource)]
pub struct SimState {
    pub speed: f32,
    pub is_paused: bool,
    pub tick_counter: u32,
    pub show_octree: bool,
}

impl Default for SimState {
    fn default() -> Self {
        Self {
            speed: 1.0,
            is_paused: false,
            tick_counter: 0,
            show_octree: false,
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
            .insert_resource(TimeUpdateStrategy::ManualDuration(
                std::time::Duration::from_secs_f32(0.016),
            ));
    } else {
        app.insert_resource(ClearColor(Color::BLACK))
            .init_resource::<SimState>()
            .add_plugins(DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    resizable: false,
                    mode: WindowMode::BorderlessFullscreen(MonitorSelection::Primary),
                    ..default()
                }),
                ..default()
            }))
            .add_plugins(FreeCameraPlugin)
            .add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin::default())
            .add_systems(Startup, (setup_camera, spawn_lights))
            .add_systems(
                Update,
                (handle_keyboard_controls, print_metrics, draw_octree_gizmos),
            );
    }

    app.init_resource::<GeneticEngine>()
        .init_resource::<PhysicsOctree>()
        .init_state::<AppState>()
        .add_systems(OnEnter(AppState::Init), init_population)
        .add_systems(
            Update,
            (update_physics, handle_acceleration)
                .chain()
                .run_if(in_state(AppState::Simulate)),
        );

    if is_headless {
        app.add_systems(
            Update,
            track_simulation.run_if(in_state(AppState::Simulate)),
        )
        .add_systems(OnEnter(AppState::Evaluate), evaluate_generation)
        .add_systems(OnEnter(AppState::Mutate), mutate_generation);
    }

    app.run();
}

fn handle_keyboard_controls(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut sim_state: ResMut<SimState>,
    mut time: ResMut<Time<Virtual>>,
) {
    if keyboard_input.just_pressed(KeyCode::KeyO) {
        sim_state.show_octree = !sim_state.show_octree;
        println!(
            "[SİSTEM] Octree Debug Görünümü: {}",
            if sim_state.show_octree {
                "AÇIK"
            } else {
                "KAPALI"
            }
        );
    }

    if keyboard_input.just_pressed(KeyCode::KeyP) {
        sim_state.is_paused = !sim_state.is_paused;
        if sim_state.is_paused {
            time.pause();
            println!("[SİSTEM] Simülasyon DURAKLATILDI.");
        } else {
            time.unpause();
            println!("[SİSTEM] Simülasyon DEVAM EDİYOR.");
        }
    }

    if keyboard_input.just_pressed(KeyCode::ArrowRight) {
        sim_state.speed = (sim_state.speed + 0.5).clamp(0.1, 10.0);
        time.set_relative_speed(sim_state.speed);
        println!("[SİSTEM] Hız artırıldı: {:.1}x", sim_state.speed);
    }

    if keyboard_input.just_pressed(KeyCode::ArrowLeft) {
        sim_state.speed = (sim_state.speed - 0.5).clamp(0.1, 10.0);
        time.set_relative_speed(sim_state.speed);
        println!("[SİSTEM] Hız düşürüldü: {:.1}x", sim_state.speed);
    }
}

fn print_metrics(
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
    query: Query<(), With<Mass>>,
    mut sim_state: ResMut<SimState>,
) {
    sim_state.tick_counter += 1;
    if sim_state.tick_counter % 60 == 0 {
        let mut fps = 0.0;
        if let Some(fps_diagnostic) =
            diagnostics.get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FPS)
        {
            if let Some(fps_value) = fps_diagnostic.smoothed() {
                fps = fps_value;
            }
        }
        let body_count = query.iter().count();
        println!(
            "[METRİKLER] FPS: {:.1} | Aktif Obje: {} | Hız: {:.1}x | Duraklatıldı: {}",
            fps, body_count, sim_state.speed, sim_state.is_paused
        );
    }
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
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }

    engine.current_tick = 0;
    let mut genome = engine.current_genome;
    if engine.generation == 1 {
        if let Some((disk_genome, disk_fitness)) = load_best_genome() {
            println!(
                "Loaded optimized genome from best_genome.json with fitness: {:.6}",
                disk_fitness
            );
            genome = disk_genome;
            engine.current_genome = disk_genome;
            engine.best_genome = disk_genome;
            engine.best_fitness = disk_fitness;
        } else {
            println!("best_genome.json not found. Using default parameters.");
            let fallback = Genome {
                pos_range: 800.0,
                vel_variance: 0.0,
                orbital_spin: 50.0,
                mass_max: BODY_MASS_RANGE[1],
            };
            genome = fallback;
            engine.current_genome = fallback;
            engine.best_genome = fallback;
            engine.best_fitness = -1.0;
        }
    } else {
        // Island Model Migration: check if there is a better genome on disk
        if let Some((disk_genome, disk_fitness)) = load_best_genome() {
            if disk_fitness > engine.best_fitness {
                println!(
                    "[MİGRASYON] Diskten daha iyi bir genom tespit edildi! Fitness: {:.6} (Lokal En İyi: {:.6})",
                    disk_fitness, engine.best_fitness
                );
                engine.best_fitness = disk_fitness;
                engine.best_genome = disk_genome;

                // Mutate from the migrated genome instead of old local best
                let mut rng = rand::rng();
                let pos_mutation = 1.0 + rng.random_range(-0.15..0.15);
                let vel_mutation = 1.0 + rng.random_range(-0.15..0.15);
                let spin_mutation = 1.0 + rng.random_range(-0.15..0.15);
                let mass_mutation = 1.0 + rng.random_range(-0.15..0.15);
                let vel_abs = rng.random_range(-0.5..=0.5);
                let pos_abs = rng.random_range(-10.0..=10.0);

                let mut mutated = disk_genome;
                mutated.pos_range = (mutated.pos_range * pos_mutation + pos_abs)
                    .max(200.0)
                    .clamp(200.0, 3000.0);
                mutated.vel_variance = (mutated.vel_variance * vel_mutation + vel_abs)
                    .max(0.0)
                    .clamp(0.0, 500.0);
                mutated.orbital_spin = (mutated.orbital_spin * spin_mutation).clamp(-500.0, 500.0);
                mutated.mass_max = (mutated.mass_max * mass_mutation).clamp(100.0, 10000.0);

                engine.current_genome = mutated;
                genome = mutated;
            }
        }
    }

    let mut rng = rand::rng();
    time.set_relative_speed(SIMULATION_SPEED_FACTOR);

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
            let mut emissive_color =
                LinearRgba::rgb(120.0 / intensity, 55.0 / intensity, intensity * 20.0);
            if intensity > 0.98 {
                emissive_color = LinearRgba::rgb(100.0, 100.0, 100.0);
            } else if intensity > 0.8 {
                emissive_color =
                    LinearRgba::rgb(25.0 / intensity, 5.0 * intensity, intensity * 200.0);
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
    }

    let mut sum_sq_dist = 0.0;

    for _ in 0..BODY_COUNT {
        let radius_dist = if genome.pos_range > 0.0 {
            genome.pos_range * rng.random_range(0.0..1.0_f32).powi(2)
        } else {
            0.0
        };

        let theta = rng.random_range(0.0..std::f32::consts::TAU);
        let y_thickness = genome.pos_range * 0.025;
        let y_pos = rng.random_range(-y_thickness..=y_thickness);

        let pos = Vec3::new(radius_dist * theta.cos(), y_pos, radius_dist * theta.sin());
        sum_sq_dist += pos.length_squared();

        let tangent = Vec3::Y.cross(pos).normalize_or_zero();
        let core_radius = genome.pos_range * 0.10;
        let distance = pos.length();

        let distance_sq = distance * distance;
        let core_sq = core_radius * core_radius;
        let orbit_speed =
            genome.orbital_spin * 300.0 * (distance / (distance_sq + core_sq).powf(0.75));

        let random_noise = Vec3::new(
            rng.random_range(-genome.vel_variance..=genome.vel_variance),
            rng.random_range(-genome.vel_variance..=genome.vel_variance),
            rng.random_range(-genome.vel_variance..=genome.vel_variance),
        );

        let vel = (tangent * orbit_speed) + random_noise;

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
            let palette_index =
                ((t * (palette_size - 1) as f32).round() as usize).min(palette_size - 1);
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
            commands.spawn((Position(pos), Velocity(vel), Mass(mass), Radius(radius)));
        }
    }

    engine.initial_body_count = BODY_COUNT as usize;
    engine.initial_rms_radius = if BODY_COUNT > 0 {
        (sum_sq_dist / BODY_COUNT as f32).sqrt().max(1.0)
    } else {
        1.0
    };

    println!("--- [Generation {} Initialized] ---", engine.generation);
    println!(
        "  [Params] pos_range: {:.2}, vel_variance: {:.2}, orbital_spin: {:.2}, mass_max: {:.2}",
        genome.pos_range, genome.vel_variance, genome.orbital_spin, genome.mass_max
    );
    println!(
        "  [Universe] bodies: {}, RMS Radius: {:.4}",
        engine.initial_body_count, engine.initial_rms_radius
    );

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
        if pos_len > 1e-6 && vel_len > 1e-6 {
            let norm_p = p / pos_len;
            let norm_v = v / vel_len;
            let dot_prod = norm_p.dot(norm_v).abs();
            sum_orbit_terms += m * dot_prod;
        }
    }

    let s_mass = if total_mass > 0.0 {
        let ratio = max_mass / total_mass;
        (1.0 - ratio.powi(2)).max(0.0)
    } else {
        0.0
    };

    let s_orbit = if total_mass > 0.0 {
        (1.0 - (sum_orbit_terms / total_mass)).max(0.0)
    } else {
        0.0
    };

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

    let s_survival = if engine.initial_body_count > 0 {
        current_body_count as f32 / engine.initial_body_count as f32
    } else {
        0.0
    };

    let fitness = s_mass * s_orbit * s_contain * s_survival;

    println!("=== [Evaluation of Gen {}] ===", engine.generation);
    println!(
        "  Fitness: {:.6} (S_mass: {:.4}, S_orbit: {:.4}, S_contain: {:.4}, S_survival: {:.4})",
        fitness, s_mass, s_orbit, s_contain, s_survival
    );
    println!(
        "  Active bodies: {} / {}",
        current_body_count, engine.initial_body_count
    );
    println!(
        "  RMS Radius: {:.2} (Initial: {:.2})",
        current_rms_radius, engine.initial_rms_radius
    );

    if fitness > engine.best_fitness {
        engine.best_fitness = fitness;
        engine.best_genome = engine.current_genome;
        println!("  *** NEW BEST GENOME SET! ***");

        let shared = SharedGenome {
            fitness,
            genome: engine.best_genome,
        };

        match serde_json::to_string_pretty(&shared) {
            Ok(json_str) => {
                if let Err(e) = std::fs::write("best_genome.json", json_str) {
                    println!("Warning: Failed to write best_genome.json: {}", e);
                } else {
                    println!(
                        "  [Saved best_genome.json to disk with fitness {:.6}]",
                        fitness
                    );
                }
            }
            Err(e) => {
                println!("Warning: Failed to serialize best genome: {}", e);
            }
        }
    }
    println!(
        "  [Best Fitness So Far] {:.6}",
        engine.best_fitness.max(fitness)
    );

    next_state.set(AppState::Mutate);
}

fn mutate_generation(
    mut engine: ResMut<GeneticEngine>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let mut rng = rand::rng();

    let pos_mutation = 1.0 + rng.random_range(-0.15..0.15);
    let vel_mutation = 1.0 + rng.random_range(-0.15..0.15);
    let spin_mutation = 1.0 + rng.random_range(-0.15..0.15);
    let mass_mutation = 1.0 + rng.random_range(-0.15..0.15);

    let vel_abs = rng.random_range(-0.5..=0.5);
    let pos_abs = rng.random_range(-10.0..=10.0);

    let mut new_genome = engine.best_genome;
    new_genome.pos_range = (new_genome.pos_range * pos_mutation + pos_abs)
        .max(200.0)
        .clamp(200.0, 3000.0);
    new_genome.vel_variance = (new_genome.vel_variance * vel_mutation + vel_abs)
        .max(0.0)
        .clamp(0.0, 500.0);
    new_genome.orbital_spin = (new_genome.orbital_spin * spin_mutation).clamp(-500.0, 500.0);
    new_genome.mass_max = (new_genome.mass_max * mass_mutation).clamp(100.0, 10000.0);

    engine.current_genome = new_genome;
    engine.generation += 1;

    println!(
        "--- [Applying Mutation to Best Genome for Gen {}] ---",
        engine.generation
    );
    println!(
        "  Mutated pos_range:    {:.2} -> {:.2}",
        engine.best_genome.pos_range, new_genome.pos_range
    );
    println!(
        "  Mutated vel_variance: {:.2} -> {:.2}",
        engine.best_genome.vel_variance, new_genome.vel_variance
    );
    println!(
        "  Mutated orbital_spin: {:.2} -> {:.2}",
        engine.best_genome.orbital_spin, new_genome.orbital_spin
    );
    println!(
        "  Mutated mass_max:     {:.2} -> {:.2}",
        engine.best_genome.mass_max, new_genome.mass_max
    );

    next_state.set(AppState::Init);
}

fn update_physics(
    time: Res<Time<Virtual>>,
    mut global_octree: ResMut<PhysicsOctree>,
    mut local_bodies: Local<Vec<body::Body>>,
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
        global_octree.0 = Some(octree::Octree::new(0.5, 2.0));
    }
    let octree = global_octree.0.as_mut().unwrap();

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

fn draw_octree_gizmos(
    global_octree: Res<PhysicsOctree>,
    sim_state: Res<SimState>,
    mut gizmos: Gizmos,
) {
    if !sim_state.show_octree {
        return;
    }

    if let Some(octree) = &global_octree.0 {
        for node in &octree.nodes {
            // Draw only the "Branch" nodes that divide the space into sub-parts
            if node.is_branch() {
                gizmos.cube(
                    Transform::from_translation(node.bounds.center)
                        .with_scale(Vec3::splat(node.bounds.size)),
                    Color::srgba(0.0, 1.0, 0.2, 0.1), //translucent neon green
                );
            }
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
