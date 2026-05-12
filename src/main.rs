use bevy::{
    camera_controller::free_camera::{FreeCamera, FreeCameraPlugin /*, FreeCameraState*/},
    core_pipeline::tonemapping::Tonemapping,
    post_process::bloom::Bloom,
    // window::{CursorGrabMode, CursorOptions},
    prelude::*,
};

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::BLACK))
        .add_plugins(DefaultPlugins)
        .add_plugins(FreeCameraPlugin)
        // .add_plugins(CursorGrab)
        .add_systems(Startup, (setup, spawn_lights, spawn_world))
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 1.0, 0.0).looking_at(Vec3::X, Vec3::Y),
        Tonemapping::TonyMcMapface,
        Bloom::NATURAL,
        FreeCamera {
            sensitivity: 0.2,
            friction: 25.0,
            walk_speed: 3.0,
            run_speed: 9.0,
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
            brightness: 100.0,
            affects_lightmapped_meshes: false,
        },
        Transform::from_xyz(0.0, 2.0, 0.0),
    ));
}

fn spawn_world(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let emissive_1 = materials.add(StandardMaterial {
        emissive: LinearRgba::rgb(0.0, 0.0, 150.0),
        ..default()
    });

    let sphere = meshes.add(Sphere::new(0.4).mesh().ico(5).unwrap());

    commands.spawn((
        Mesh3d(sphere.clone()),
        MeshMaterial3d(emissive_1),
        Transform::from_xyz(1.0, 0.0, 1.0),
    ));
}
