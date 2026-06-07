use crate::config::*;
use crate::physics::*;
use bevy::prelude::*;
use rand::RngExt;

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
    #[serde(default)]
    pub generation: usize,
}

pub fn load_best_genome() -> Option<(Genome, f32, usize)> {
    let path = std::path::Path::new("best_genome.json");
    if !path.exists() {
        return None;
    }
    // Retry up to 5 times in case of sharing violations/interleaved writes
    for _ in 0..5 {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(shared) = serde_json::from_str::<SharedGenome>(&content) {
                return Some((shared.genome, shared.fitness, shared.generation));
            }
            if let Ok(genome) = serde_json::from_str::<Genome>(&content) {
                return Some((genome, -1.0, 0));
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    None
}

pub fn save_best_genome(shared: &SharedGenome) -> Result<(), std::io::Error> {
    let json_str = serde_json::to_string_pretty(shared)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    // Use process ID to create a unique temp file name
    let temp_name = format!("best_genome.json.{}.tmp", std::process::id());
    let temp_path = std::path::Path::new(&temp_name);
    let target_path = std::path::Path::new("best_genome.json");

    std::fs::write(temp_path, json_str)?;

    let mut rename_err = None;
    for _ in 0..5 {
        match std::fs::rename(temp_path, target_path) {
            Ok(_) => return Ok(()),
            Err(e) => {
                rename_err = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }

    // Clean up temp file if rename failed
    let _ = std::fs::remove_file(temp_path);
    Err(rename_err.unwrap_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::Other, "Rename failed after retries")
    }))
}

#[derive(Resource)]
pub struct GeneticEngine {
    pub generation: usize,
    pub current_tick: usize,
    pub max_ticks: usize,
    pub current_genome: Genome,
    pub best_genome: Genome,
    pub best_fitness: f32,
    pub best_generation: usize,
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
            best_generation: 1,
            initial_body_count: BODY_COUNT as usize,
            initial_rms_radius: 1.0,
        }
    }
}

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

pub fn init_population(
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
        if let Some((disk_genome, disk_fitness, disk_gen)) = load_best_genome() {
            println!(
                "Loaded optimized genome from best_genome.json with fitness: {:.6} (Gen: {})",
                disk_fitness, disk_gen
            );
            genome = disk_genome;
            engine.current_genome = disk_genome;
            engine.best_genome = disk_genome;
            engine.best_fitness = disk_fitness;
            engine.best_generation = disk_gen;
            engine.generation = disk_gen;
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
            engine.best_generation = 1;
        }
    } else {
        // Island Model Migration: check if there is a better genome on disk
        if let Some((disk_genome, disk_fitness, disk_gen)) = load_best_genome() {
            if disk_fitness > engine.best_fitness {
                println!(
                    "[MİGRASYON] Diskten daha iyi bir genom tespit edildi! Fitness: {:.6} (Gen: {}) (Lokal En İyi: {:.6})",
                    disk_fitness, disk_gen, engine.best_fitness
                );
                engine.best_fitness = disk_fitness;
                engine.best_genome = disk_genome;
                engine.best_generation = disk_gen;
                engine.generation = disk_gen;

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

    // Place the Sun at the center of the system.
    // This will prevent bodies from flying out and force them to orbit
    let sun_mass = genome.mass_max * 150.0; // 150 times heavier than the largest planet
    let sun_radius = (sun_mass / 100.0).cbrt().max(2.0);

    if is_visual {
        commands.spawn((
            Mesh3d(base_mesh.as_ref().unwrap().clone()),
            MeshMaterial3d(material_palette[255].clone()), // En parlak materyal
            Transform::from_translation(Vec3::ZERO).with_scale(Vec3::splat(sun_radius)),
            Position(Vec3::ZERO),
            Velocity(Vec3::ZERO),
            Mass(sun_mass),
            Radius(sun_radius),
        ));
    } else {
        commands.spawn((
            Position(Vec3::ZERO),
            Velocity(Vec3::ZERO),
            Mass(sun_mass),
            Radius(sun_radius),
        ));
    }

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

pub fn track_simulation(
    mut engine: ResMut<GeneticEngine>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    engine.current_tick += 1;
    if engine.current_tick >= engine.max_ticks {
        next_state.set(AppState::Evaluate);
    }
}

pub fn evaluate_generation(
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

    // If the universe expands beyond the initial diameter, it will be penalized quadratically.
    // If the universe expands 2 times, its score will drop to 25%, if it expands 3 times, it will drop to 11%, and if it escapes quickly, it will drop to 0%.
    let s_contain = if current_rms_radius > initial_rms {
        (initial_rms / current_rms_radius).powi(2)
    } else {
        1.0 // No penalty if it collapses or is the same size
    };

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

    // Read the current best genome on disk to prevent overwriting a better score (Island Model coordination)
    let (disk_genome, disk_fitness, disk_gen) = load_best_genome()
        .map(|(g, f, g_gen)| (Some(g), f, g_gen))
        .unwrap_or((None, -1.0, 0));

    if fitness > engine.best_fitness || disk_fitness > engine.best_fitness {
        // Something is better than our local best!
        if disk_fitness > fitness {
            // The disk has the absolute best genome. Migrate it locally.
            if let Some(dg) = disk_genome {
                engine.best_fitness = disk_fitness;
                engine.best_genome = dg;
                engine.best_generation = disk_gen;
                engine.generation = disk_gen;
                println!(
                    "  [MİGRASYON] Diskten daha iyi bir genom tespit edildi! Fitness: {:.6} (Gen: {}) (Lokal Aday: {:.6})",
                    disk_fitness, disk_gen, fitness
                );
            }
        } else {
            // Our new fitness is the absolute best (or equal to disk, but better than local). Save to disk.
            let new_gen = disk_gen + 1;
            engine.best_fitness = fitness;
            engine.best_genome = engine.current_genome;
            engine.best_generation = new_gen;
            engine.generation = new_gen;
            println!("  *** NEW BEST GENOME SET! (Gen {}) ***", new_gen);

            let shared = SharedGenome {
                fitness,
                genome: engine.best_genome,
                generation: new_gen,
            };
            if let Err(e) = save_best_genome(&shared) {
                println!("Warning: Failed to write best_genome.json: {}", e);
            } else {
                println!(
                    "  [Saved best_genome.json to disk with fitness {:.6} from Gen {}]",
                    fitness, new_gen
                );
            }
        }
    } else if engine.best_fitness > disk_fitness {
        // Our local best is better than what's on disk (e.g. disk was deleted or corrupted).
        // Restore/write local best to disk.
        println!("  [KORUMA] Lokal en iyi genom disktekinden daha iyi. Diske yazılıyor...");
        let new_gen = disk_gen + 1;
        engine.best_generation = new_gen;
        engine.generation = new_gen;
        let shared = SharedGenome {
            fitness: engine.best_fitness,
            genome: engine.best_genome,
            generation: new_gen,
        };
        if let Err(e) = save_best_genome(&shared) {
            println!("Warning: Failed to write best_genome.json: {}", e);
        } else {
            println!(
                "  [Saved best_genome.json to disk with fitness {:.6} from Gen {}]",
                engine.best_fitness, new_gen
            );
        }
    }
    println!(
        "  [Best Fitness So Far] {:.6}",
        engine.best_fitness.max(fitness)
    );

    next_state.set(AppState::Mutate);
}

pub fn mutate_generation(
    mut engine: ResMut<GeneticEngine>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let mut rng = rand::rng();

    // Simulated Annealing (Mutation Decay)
    // As we approach the 2500th generation, the mutation range decreases from 15% to 0.5% (fine-tuning).
    let progress = (engine.generation as f32 / 2500.0).clamp(0.0, 1.0);
    let dynamic_mutation_rate = 0.15 * (1.0 - progress) + 0.005 * progress;

    // Hypermutation (Escape Local Minima)
    // 5% chance, a huge 40% jump to escape the local optimum.
    let is_hyper = rng.random_range(0.0..1.0) < 0.05;
    let mut_rate = if is_hyper {
        0.40
    } else {
        dynamic_mutation_rate
    };

    let pos_mutation = 1.0 + rng.random_range(-mut_rate..mut_rate);
    let vel_mutation = 1.0 + rng.random_range(-mut_rate..mut_rate);
    let spin_mutation = 1.0 + rng.random_range(-mut_rate..mut_rate);
    let mass_mutation = 1.0 + rng.random_range(-mut_rate..mut_rate);

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

    if is_hyper {
        println!(
            "  [!] HYPERMUTATION TRIGGERED! Escaping local optimum with ±{:.1}% leap...",
            mut_rate * 100.0
        );
    } else {
        println!(
            "  [Annealing] Fine-tuning with dynamic rate: ±{:.2}%",
            mut_rate * 100.0
        );
    }

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
