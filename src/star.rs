use crate::config::*;
use bevy::prelude::*;
use rand::{RngExt, rng};
use std::{f32::consts::PI, ops::Range};

#[derive(Component)]
pub struct Star {
    //temp: i8,
    offset: Vec3,
    barn_id: i32,
}

#[derive(Component)]
pub struct Barn {
    pos: Vec3,
    vel: Vec3,
    acc: Vec3,
    id: i32,
}

fn generate_angle() -> f32 {
    let mut rng = rng();
    PI * 2.0 * rng.random_range::<f32, Range<f32>>(0.0..1.001)
}

pub fn spawn_barns(
    mut commands: Commands,
    mut material: ResMut<Assets<StandardMaterial>>,
    mut mesh: ResMut<Assets<Mesh>>,
) {
    let cold_material = material.add(StandardMaterial {
        emissive: LinearRgba::rgb(100.0, 0.0, 0.0),
        ..default()
    });
    let hot_material = material.add(StandardMaterial {
        emissive: LinearRgba::rgb(100.0, 50.0, 0.0),
        ..default()
    });
    let vhot_material = material.add(StandardMaterial {
        emissive: LinearRgba::rgb(0.0, 0.0, 150.0),
        ..default()
    });
    let omgsohot_material = material.add(StandardMaterial {
        emissive: LinearRgba::rgb(150.0, 150.0, 150.0),
        ..default()
    });

    let materials = [
        cold_material,
        hot_material,
        vhot_material,
        omgsohot_material,
    ];

    let sphere = mesh.add(Sphere::new(SPHERE_RADIUS).mesh().ico(SUB_DIVISION).unwrap());

    let mut rng = rng();
    let mut ids: Vec<i32> = Vec::with_capacity(STAR_COUNT / BARN_SIZE);
    for _ in 0..(STAR_COUNT / BARN_SIZE) {
        let mut id: i32 = rng.random();
        while ids.contains(&id) {
            id = rng.random();
        }
        ids.push(id);
        let mut angle1: f32 = generate_angle() / 2.0;
        let (mut sin_y, mut cos_y) = angle1.sin_cos();
        let mut angle2: f32 = generate_angle();
        let (mut sin_xz, mut cos_xz) = angle2.sin_cos();
        let mut radius = rng.random_range(0..GENERAL_RADIUS + 1) as f32;
        let mut pos: Vec3 = Vec3::new(
            radius * sin_y * cos_xz,
            radius * cos_y,
            radius * sin_y * sin_xz,
        );
        commands.spawn((
            Transform::from_xyz(pos.x, pos.y, pos.z),
            Barn {
                pos: pos.clone(),
                vel: Vec3::ZERO,
                acc: Vec3::ZERO,
                id: id,
            },
        ));

        for _ in 0..BARN_SIZE {
            let temp: u8 = rng.random();
            angle1 = generate_angle() / 2.0;
            angle2 = generate_angle();
            (sin_y, cos_y) = angle1.sin_cos();
            (sin_xz, cos_xz) = angle2.sin_cos();
            radius = rng.random_range(0..BARN_RADIUS + 1) as f32;
            pos = Vec3::new(
                radius * sin_y * cos_xz,
                radius * cos_y,
                radius * sin_y * sin_xz,
            );
            let s_material = materials[(temp / 64).min(3) as usize].clone();
            commands.spawn((
                Transform::from_xyz(pos.x, pos.y, pos.z),
                Mesh3d(sphere.clone()),
                MeshMaterial3d(s_material),
                Star {
                    offset: pos.clone(),
                    barn_id: id,
                },
            ));
        }
    }
}
