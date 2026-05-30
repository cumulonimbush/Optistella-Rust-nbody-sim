use bevy::{
    camera_controller::free_camera::{FreeCamera, FreeCameraPlugin},
    core_pipeline::tonemapping::Tonemapping,
    post_process::bloom::Bloom,
    prelude::*,
    window::WindowMode,
};
use rand::RngExt;
use std::collections::HashMap;
use crate::config::*;

mod body;
mod octree;
mod config;

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
) {
    let mut rng = rand::rng();

    // Spawn 500 spheres
    let sphere_mesh = meshes.add(Sphere::new(BODY_MESH_RADIUS).mesh().ico(4).unwrap());

    for _ in 0..BODY_COUNT {
        let pos = Vec3::new(
            rng.random_range(-BODY_POS_RANGE..BODY_POS_RANGE),
            rng.random_range(-BODY_POS_RANGE..BODY_POS_RANGE),
            rng.random_range(-BODY_POS_RANGE..BODY_POS_RANGE),
        );
        // Orbital-like velocities or random expansion velocities
        let vel = Vec3::new(
            rng.random_range(-BODY_VEL_RANGE..BODY_VEL_RANGE),
            rng.random_range(-BODY_VEL_RANGE..BODY_VEL_RANGE),
            rng.random_range(-BODY_VEL_RANGE..BODY_VEL_RANGE),
        );
        let mass: f32 = rng.random_range(BODY_MASS_RANGE[0]..BODY_MASS_RANGE[1]);
        // Recycle mesh radius as object radius
        let radius = BODY_MESH_RADIUS;

        // harmonized color based on mass (heavier bodies are hotter/brighter)
        let intensity = (mass / 1000.0).clamp(0.1, 1.0);
        let color = Color::hsl(30.0 + intensity * 40.0, 0.9, 0.4 + intensity * 0.3);
        let emissive_color = LinearRgba::rgb(intensity * 10.0, intensity * 5.0, intensity * 2.0);

        let sphere_material = materials.add(StandardMaterial {
            base_color: color,
            emissive: emissive_color,
            metallic: 0.2,
            perceptual_roughness: 0.5,
            ..default()
        });

        commands.spawn((
            Mesh3d(sphere_mesh.clone()),
            MeshMaterial3d(sphere_material),
            Transform::from_translation(pos),
            Position(pos),
            Velocity(vel),
            Mass(mass),
            Radius(radius),
        ));
    }
}

fn update_physics(
    time: Res<Time>,
    mut query: Query<(&mut Position, &mut Velocity, &Mass, &mut Transform)>,
) {
    let dt = time.delta_secs().min(0.03); // Cap dt to avoid large time step instability

    // Gather active body data to construct the Octree
    let mut bodies = Vec::with_capacity(BODY_COUNT as usize);
    for (pos, vel, mass, _) in query.iter() {
        bodies.push(body::Body::new(pos.0, vel.0, mass.0, BODY_MESH_RADIUS));
    }

    if bodies.is_empty() {
        return;
    }

    // Build standard Bounds3D containing all active bodies
    let bounds = octree::Bounds3D::new_containing(&bodies);

    // Theta = 0.5 for Barnes-Hut accuracy, Epsilon = 2.0 for soft potential (no infinite acceleration)
    let mut octree = octree::Octree::new(0.5, 2.0);
    octree.clear(bounds);

    for body in &bodies {
        octree.insert(body.pos, body.mass);
    }
    octree.propagate();

    // Apply acceleration calculated from Octree
    for (mut pos, mut vel, _mass, mut transform) in query.iter_mut() {
        let acc = octree.acc(pos.0);
        vel.0 += acc * dt;
        pos.0 += vel.0 * dt;
        transform.translation = pos.0;
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
) {
    // Copy all entity data to a temporary vector for reading. (No Borrow checker)
    let mut bodies = Vec::new();
    for (entity, pos, vel, mass, radius, _) in query.iter() {
        bodies.push((entity, pos.0, vel.0, mass.0, radius.0));
    }

    let n = bodies.len();
    if n < 2 {
        return;
    }

    // Union-Find series: Everyone is initially their own parent.
    let mut parent = (0..n).collect::<Vec<usize>>();

    // Union-Find yhelper func
    fn find(i: usize, parent: &mut Vec<usize>) -> usize {
        if parent[i] == i {
            i
        } else {
            let root = find(parent[i], parent);
            parent[i] = root; // Path compression
            root
        }
    }

    // Find intersections and merge sets.
    for i in 0..n {
        for j in (i + 1)..n {
            let p1 = bodies[i].1;
            let p2 = bodies[j].1;
            let r1 = bodies[i].4;
            let r2 = bodies[j].4;

            let d_sq = (p1 - p2).length_squared();
            let r_sum = r1 + r2;

            if d_sq < r_sum * r_sum {
                let root_i = find(i, &mut parent);
                let root_j = find(j, &mut parent);
                if root_i != root_j {
                    // Make the heavier object (or the one with the smaller index) the root.
                    if bodies[root_i].3 >= bodies[root_j].3 {
                        parent[root_j] = root_i;
                    } else {
                        parent[root_i] = root_j;
                    }
                }
            }
        }
    }

    // Calculation of the total mass and momentum of the clusters.
    // Key: Root Index, Value: (Total Mass, Total Momentum (Mass * Vel), Center of Mass (Mass * Pos))
    let mut cluster_data: HashMap<usize, (f32, Vec3, Vec3)> = HashMap::new();

    for i in 0..n {
        let root = find(i, &mut parent);
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
        let root = find(i, &mut parent);
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
                    t.scale = Vec3::splat(new_radius / 0.4);
                }
            }
        }
    }
}
