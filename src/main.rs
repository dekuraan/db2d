use bevy::utils::Instant;
use bevy::{prelude::*, utils::tracing::span::Entered};
use bevy_inspector_egui::Inspectable;
use bevy_inspector_egui::WorldInspectorPlugin;
use bevy_rapier2d::{prelude::*, rapier::parry::query::contact};
use core::panic;
use std::default;
use std::fmt::Result;

fn main() {
    App::build()
        .add_plugins(DefaultPlugins)
        .add_plugin(RapierPhysicsPlugin::<NoUserData>::default())
        .add_plugin(RapierRenderPlugin)
        .add_system(move_player.system())
        .add_plugin(WorldInspectorPlugin::new())
        .add_system(start_vaults.system())
        .add_system(perform_vaults.system())
        .add_system(complete_vaults.system())
        .add_startup_system(setup_physics.system())
        .run();
}

fn move_player(
    mut query: Query<(&mut RigidBodyVelocity,), With<Player>>,
    keyboard_input: Res<Input<KeyCode>>,
) {
    for (mut velocity,) in query.iter_mut() {
        let mut x = 0.0;
        let mut y = 0.0;

        if keyboard_input.pressed(KeyCode::W) {
            y += 1.0;
        }
        if keyboard_input.pressed(KeyCode::S) {
            y -= 1.0;
        }
        if keyboard_input.pressed(KeyCode::D) {
            x += 1.0;
        }
        if keyboard_input.pressed(KeyCode::A) {
            x -= 1.0;
        }

        velocity.linvel = (Vec2::new(x, y).normalize_or_zero() * 100.0).into();
    }
}
const PLAYER_RADIUS: f32 = 50.0;
fn spawn_player(commands: &mut Commands) {
    let rigid_body = RigidBodyBundle {
        position: [0.0, 0.0].into(),
        mass_properties: (RigidBodyMassPropsFlags::ROTATION_LOCKED).into(),
        body_type: RigidBodyType::Dynamic,
        ..Default::default()
    };
    let collider = ColliderBundle {
        shape: ColliderShape::ball(PLAYER_RADIUS),
        flags: ColliderFlags {
            active_events: (ActiveEvents::CONTACT_EVENTS).into(),
            ..Default::default()
        },
        material: ColliderMaterial {
            friction: 0.0,
            friction_combine_rule: CoefficientCombineRule::Min,
            restitution: 0.0,
            ..Default::default()
        },
        ..Default::default()
    };
    commands
        .spawn()
        .insert_bundle(rigid_body)
        .insert_bundle(collider)
        .insert(Vaulter)
        .insert(Player)
        .insert(ColliderPositionSync::Discrete)
        .insert(ColliderDebugRender::from(Color::GREEN));
}

fn spawn_wall<P: Into<RigidBodyPosition>, R: Into<Real>>(
    commands: &mut Commands,
    position: P,
    hx: R,
    hy: R,
) {
    let rigid_body = RigidBodyBundle {
        position: position.into(),
        body_type: RigidBodyType::Static,
        ..Default::default()
    };
    let collider = ColliderBundle {
        shape: ColliderShape::cuboid(hx.into(), hy.into()),
        material: ColliderMaterial {
            restitution: 0.0,
            ..Default::default()
        },
        ..Default::default()
    };
    commands
        .spawn()
        .insert_bundle(rigid_body)
        .insert_bundle(collider)
        .insert(ColliderPositionSync::Discrete)
        .insert(ColliderDebugRender::from(Color::BLACK));
}

fn spawn_window<P: Into<RigidBodyPosition>, R: Into<Real>>(
    commands: &mut Commands,
    position: P,
    hx: R,
    hy: R,
) {
    let rigid_body = RigidBodyBundle {
        position: position.into(),
        body_type: RigidBodyType::Static,
        ..Default::default()
    };
    let collider = ColliderBundle {
        // collider_type: ColliderType::Sensor,
        shape: ColliderShape::cuboid(hx.into(), hy.into()),
        material: ColliderMaterial {
            restitution: 0.0,
            ..Default::default()
        },
        ..Default::default()
    };
    commands
        .spawn()
        .insert_bundle(rigid_body)
        .insert_bundle(collider)
        .insert(Vaultable)
        .insert(ColliderPositionSync::Discrete)
        .insert(ColliderDebugRender::from(Color::BLUE));
}

fn setup_physics(mut commands: Commands) {
    commands.spawn_bundle(OrthographicCameraBundle::new_2d());
    spawn_player(&mut commands);
    spawn_window(&mut commands, (Vec2::new(100.0, 100.0), 30.0), 120.0, 20.0);
}

#[derive(Clone, Copy)]
struct Player;

#[derive(Clone, Copy, Inspectable, Debug)]
struct Vaultable;

enum PlayerState {
    Moving,
}

#[derive(Clone, Copy)]
struct Vaulter;

fn start_vaults(
    keyboard_input: Res<Input<KeyCode>>,
    vaulters: Query<(Entity, &Vaulter)>,
    vaultables: Query<(Entity,), (With<Vaultable>,)>,
    narrow_phase: Res<NarrowPhase>,
    time: Res<Time>,
    mut commands: Commands,
) {
    if keyboard_input.just_pressed(KeyCode::Space) {
        let player_e = vaulters.iter().next().unwrap().0;
        let player_ch = player_e.handle();
        if let Some(cp) = vaultables
            .iter()
            .filter_map(|(vaultable_e,)| narrow_phase.contact_pair(player_ch, vaultable_e.handle()))
            .filter(|cp| cp.has_any_active_contact)
            //todo: determine correct cp based on player velocity
            .next()
        {
            let vd: VaultDirection = cp.manifolds[0].local_n1[0].into();
            commands.entity(player_e).insert(Vaulting {
                vaultable_e: cp.collider2.entity(),
                start_time: time.seconds_since_startup(),
                direction: vd,
            });
        }
    }
}

const VAULT_TIME: f32 = 0.25;
//unsafe if an entity has Vaulting and Vaultable
fn perform_vaults(
    mut set: QuerySet<(
        Query<(&Vaulting, &Transform, &mut RigidBodyPosition)>,
        Query<(&Transform,), With<Vaultable>>,
    )>,
    time: Res<Time>,
    mut commands: Commands,
) {
    unsafe {
        for (v, tf, mut rb_p) in set.q0().iter_unsafe() {
            let vault_completion =
                (time.seconds_since_startup() - v.start_time) as f32 / VAULT_TIME;
            let local_vy: f32 = v.direction * vault_completion * (50.0 + 20.0);
            let local_vec = Vec3::new(0.0, local_vy, 0.0);
            let vb_tf = set.q1().get_component::<Transform>(v.vaultable_e).unwrap();
            let new_tl = vb_tf.translation;
            let new_tl: Vec3 = new_tl + vb_tf.rotation.mul_vec3(local_vec);
            dbg!(new_tl);
            let rb_tl = Vec2::new(new_tl.x, new_tl.y);
            rb_p.position.translation = rb_tl.into();
        }
    }
}

fn complete_vaults(
    time: Res<Time>,
    mut vaulting: Query<(Entity, &Vaulting, &mut Transform)>,
    mut commands: Commands,
) {
    for (e, v, mut tf) in vaulting.iter_mut() {
        let vault_completion = (time.seconds_since_startup() - v.start_time) as f32 / VAULT_TIME;
        if vault_completion > 1.0 {
            commands.entity(e).remove::<Vaulting>();
        }
    }
}

#[derive(Clone, Copy, Debug)]

struct Vaulting {
    vaultable_e: Entity,
    start_time: f64,
    direction: VaultDirection,
}

#[derive(Clone, Copy, Debug)]
enum VaultDirection {
    Pos,
    Neg,
}

impl From<f32> for VaultDirection {
    fn from(n: f32) -> Self {
        if n > 0.0 {
            return VaultDirection::Pos;
        } else if n < 0.0 {
            return VaultDirection::Neg;
        } else {
            panic!("Vault Direction normal == 0.0!");
        }
    }
}

impl std::ops::Mul<f32> for VaultDirection {
    type Output = f32;

    fn mul(self, rhs: f32) -> Self::Output {
        match self {
            VaultDirection::Pos => rhs * 1.0,
            VaultDirection::Neg => rhs * -1.0,
        }
    }
}
