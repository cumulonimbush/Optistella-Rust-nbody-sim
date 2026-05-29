use bevy::{
    camera_controller::free_camera::{FreeCamera, FreeCameraPlugin},
    core_pipeline::tonemapping::Tonemapping,
    post_process::bloom::Bloom,
    prelude::*,
    window::WindowMode,
};
use rand::RngExt;
use std::collections::HashMap;

mod body;
mod octree;

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
        .add_systems(Update, (update_physics)
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
    let sphere_mesh = meshes.add(Sphere::new(0.4).mesh().ico(4).unwrap());

    for _ in 0..500 {
        let pos = Vec3::new(
            rng.random_range(-60.0..60.0),
            rng.random_range(-60.0..60.0),
            rng.random_range(-60.0..60.0),
        );
        // Orbital-like velocities or random expansion velocities
        let vel = Vec3::new(
            rng.random_range(-15.0..15.0),
            rng.random_range(-15.0..15.0),
            rng.random_range(-15.0..15.0),
        );
        let mass: f32 = rng.random_range(10.0..1000.0);
        let radius = 0.4;

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
    let mut bodies = Vec::with_capacity(500);
    for (pos, vel, mass, _) in query.iter() {
        bodies.push(body::Body::new(pos.0, vel.0, mass.0, 0.4));
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
