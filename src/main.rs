mod body;
mod config;
mod octree;
mod physics;
mod genalg;

use bevy::{
    camera_controller::free_camera::{FreeCamera, FreeCameraPlugin},
    core_pipeline::tonemapping::Tonemapping,
    post_process::bloom::Bloom,
    prelude::*,
    time::TimeUpdateStrategy,
    window::WindowMode,
};
use rand::RngExt;
use crate::config::*;
use physics::*;
use genalg::*;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

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
            .add_systems(Update, (handle_keyboard_controls, update_metrics));
    }

    app.init_resource::<GeneticEngine>()
        .init_state::<AppState>()
        .add_systems(OnEnter(AppState::Init), init_population)
        .add_systems(Startup, spawn_text)
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

fn update_metrics(
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
    query: Query<(), With<Mass>>,
    mut sim_state: ResMut<SimState>,
    mut textq: Query<&mut Text, With<TechnicText>>,
) {
    sim_state.tick_counter += 1;
    if sim_state.tick_counter % 60 == 0 {
        let mut fps = 0.0;
        let mut text = textq.single_mut().unwrap();
        if let Some(fps_diagnostic) =
            diagnostics.get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FPS)
        {
            if let Some(fps_value) = fps_diagnostic.smoothed() {
                fps = fps_value;
            }
        }
        let body_count = query.iter().count();
        text.0 = format!(
            "FPS: {:.1}\nActive Objects: {}\nSimulation Speed: {:.1}x\nPaused {}",
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

#[derive(Component)]
struct TechnicText;

fn spawn_text(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(10),
            left: px(10),
            ..default()
        },
        children![Text::new(concat![
            "ESC to focus/unfocus\n",
            "wasd to move\n",
            "arrow keys to change speed\n",
            "ctrl to move faster\n",
            "space to move up\n",
            "shift to move down\n",
            "p to pause\n",
            "use mouse to look around"
        ]),]
    ));
    
    commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(10),
                right: px(10),
                ..default()
            },
            children![(TechnicText, Text::new(""))],
    ));
}
