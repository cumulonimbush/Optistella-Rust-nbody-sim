use std::f32::consts::{FRAC_PI_4, PI};

use bevy::{
    camera_controller::free_camera::{FreeCamera, FreeCameraPlugin /*, FreeCameraState*/},
    color::palettes::tailwind,
    prelude::*,
    // window::{CursorGrabMode, CursorOptions},
};

fn main() {
    App::new()
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

// struct CursorGrab;
// impl Plugin for CursorGrab {
//     fn build(&self, app: &mut App) {
//         app.add_systems(PostStartup, setup_cursor_grab)
//             .add_systems(Update, cursor_ungrab);
//     }
// }

// fn setup_cursor_grab(
//     mut cursor_options: Single<&mut CursorOptions>,
//     mut free_camera_query: Query<(&mut FreeCamera, &mut FreeCameraState)>,
// ) {
//     cursor_options.visible = false;
//     cursor_options.grab_mode = CursorGrabMode::Locked;
//     let (_, mut camera_state) = free_camera_query.single_mut().unwrap();
//     camera_state.enabled = true;
// }

// fn cursor_ungrab(
//     mut cursor_options: Single<&mut CursorOptions>,
//     mut free_camera_query: Query<(&mut FreeCamera, &mut FreeCameraState)>,
//     mouse: Res<ButtonInput<MouseButton>>,
//     key: Res<ButtonInput<KeyCode>>,
// ) {
//     let (_, mut free_camera_state) = free_camera_query.single_mut().unwrap();
//     if mouse.just_pressed(MouseButton::Left) {
//         cursor_options.visible = false;
//         cursor_options.grab_mode = CursorGrabMode::Locked;
//         free_camera_state.enabled = true;
//     }
//     if key.just_pressed(KeyCode::Escape) {
//         cursor_options.visible = true;
//         cursor_options.grab_mode = CursorGrabMode::None;
//         free_camera_state.enabled = false;
//     }
// }

fn spawn_lights(mut commands: Commands) {
    // Main light
    commands.spawn((
        PointLight {
            color: Color::from(tailwind::ORANGE_300),
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(0.0, 3.0, 0.0),
    ));
    // Light behind wall
    commands.spawn((
        PointLight {
            color: Color::WHITE,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(-3.5, 3.0, 0.0),
    ));
    // Light under floor
    commands.spawn((
        PointLight {
            color: Color::from(tailwind::RED_300),
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(0.0, -0.5, 0.0),
    ));
}

fn spawn_world(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let floor = meshes.add(Plane3d::new(Vec3::Y, Vec2::splat(10.0)));
    let sphere = meshes.add(Sphere::new(0.5));
    let wall = meshes.add(Cuboid::new(0.2, 4.0, 3.0));

    let blue_material = materials.add(Color::from(tailwind::BLUE_700));
    let red_material = materials.add(Color::from(tailwind::RED_950));
    let white_material = materials.add(Color::WHITE);

    // Top side of floor
    commands.spawn((
        Mesh3d(floor.clone()),
        MeshMaterial3d(white_material.clone()),
    ));
    // Under side of floor
    commands.spawn((
        Mesh3d(floor.clone()),
        MeshMaterial3d(white_material.clone()),
        Transform::from_xyz(0.0, -0.01, 0.0).with_rotation(Quat::from_rotation_x(PI)),
    ));
    // Blue sphere
    commands.spawn((
        Mesh3d(sphere.clone()),
        MeshMaterial3d(blue_material.clone()),
        Transform::from_xyz(3.0, 1.5, 0.0),
    ));
    // Tall wall
    commands.spawn((
        Mesh3d(wall.clone()),
        MeshMaterial3d(white_material.clone()),
        Transform::from_xyz(-3.0, 2.0, 0.0),
    ));
    // Cube behind wall
    commands.spawn((
        Mesh3d(cube.clone()),
        MeshMaterial3d(blue_material.clone()),
        Transform::from_xyz(-4.2, 0.5, 0.0),
    ));
    // Hidden cube under floor
    commands.spawn((
        Mesh3d(cube.clone()),
        MeshMaterial3d(red_material.clone()),
        Transform {
            translation: Vec3::new(3.0, -2.0, 0.0),
            rotation: Quat::from_euler(EulerRot::YXZEx, FRAC_PI_4, FRAC_PI_4, 0.0),
            ..default()
        },
    ));
}
