use std::time::Duration;

use bevy::{ecs::system::SystemId, input::common_conditions::input_just_pressed, prelude::*};
use bevy_ecs_ldtk::prelude::*;
use bevy_rapier2d::prelude::*;

use crate::{
    camera::{
        camera_position_from_level, CameraControlType, CameraMoveEvent, CameraTransition,
        CameraTransitionEvent,
    },
    level::{
        entity::HurtMarker, shard::reset_shard_effects_on_kill, start_flag::StartFlag,
        CurrentLevel, LevelSystems,
    },
    shared::{AnimationState, GameState, ResetLevel, LYRA_RESPAWN_EPSILON},
};

use super::{
    light::{AngleMarker, PlayerLightInventory},
    movement::PlayerMovement,
    PlayerHurtMarker, PlayerMarker,
};

pub struct PlayerKillPlugin;

impl Plugin for PlayerKillPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<KillAnimationCallbacks>()
            .add_event::<KillPlayerEvent>()
            .add_systems(Update, reset_player_on_kill.in_set(LevelSystems::Reset))
            .add_systems(
                Update,
                (
                    quick_reset
                        .run_if(input_just_pressed(KeyCode::KeyR))
                        .run_if(in_state(GameState::Playing)),
                    // reset player will try to preserve the current color, the calculations for
                    // which depend on proper values for the current level's allowed colors
                    reset_player_on_level_switch
                        .after(reset_shard_effects_on_kill)
                        .in_set(LevelSystems::Reset),
                ),
            )
            .add_systems(
                FixedUpdate,
                kill_player_on_hurt_intersection.in_set(LevelSystems::Simulation),
            )
            .add_systems(
                FixedUpdate,
                (start_kill_animation, play_death_sound_on_kill)
                    .run_if(on_event::<KillPlayerEvent>),
            );
    }
}

/// [`System`] that will kill the player on press of the R key
pub fn quick_reset(mut ev_kill_player: EventWriter<KillPlayerEvent>) {
    ev_kill_player.send(KillPlayerEvent);
}

pub fn play_death_sound_on_kill(
    mut commands: Commands,
    q_player: Query<Entity, With<PlayerMarker>>,
    asset_server: Res<AssetServer>,
) {
    let Ok(player) = q_player.get_single() else {
        return;
    };
    commands.entity(player).with_child((
        AudioPlayer::new(asset_server.load("sfx/death.wav")),
        PlaybackSettings::DESPAWN,
    ));
}

/// [`System`] that runs on [`GameState::Respawning`]. Will turn the state back into playing
/// immediately.
pub fn reset_player_on_kill(
    mut commands: Commands,
    // angle marker despawn should realistically happen in a diff system?
    q_angle_marker: Query<Entity, With<AngleMarker>>,
    mut ev_reset_level: EventReader<ResetLevel>,
    q_start_flag: Query<(&StartFlag, &EntityInstance)>,
    current_level: Res<CurrentLevel>,
    mut ev_move_camera: EventWriter<CameraMoveEvent>,
    mut q_player: Query<&mut Transform, With<PlayerMarker>>,
) {
    // check that we recieved a ResetLevel event asking us to Respawn
    if !ev_reset_level.read().any(|x| *x == ResetLevel::Respawn) {
        return;
    }
    let Ok(mut player_transform) = q_player.get_single_mut() else {
        return;
    };

    if let Ok(angle_marker) = q_angle_marker.get_single() {
        commands.entity(angle_marker).despawn_recursive();
    }

    for (flag, instance) in q_start_flag.iter() {
        if current_level.level_iid == flag.level_iid {
            player_transform.translation.x =
                instance.world_x.expect("Lightborne uses Free world layout") as f32;
            player_transform.translation.y =
                -instance.world_y.expect("Lightborne uses Free world layout") as f32
                    + LYRA_RESPAWN_EPSILON;
            // add small height so Lyra is not stuck into the floor
            ev_move_camera.send(CameraMoveEvent {
                to: camera_position_from_level(
                    current_level.level_box,
                    player_transform.translation.xy(),
                ),
                variant: CameraControlType::Instant,
            });
            return;
        }
    }

    panic!("Couldn't find start flag to respawn at");
}

/// Resets the player inventory and movement information on a [`LevelSwitchEvent`]
pub fn reset_player_on_level_switch(
    mut q_player: Query<(&mut PlayerMovement, &mut PlayerLightInventory), With<PlayerMarker>>,
    current_level: Res<CurrentLevel>,
) {
    let Ok((mut movement, mut inventory)) = q_player.get_single_mut() else {
        return;
    };

    let old_color = inventory.current_color;

    *movement = PlayerMovement::default();
    *inventory = PlayerLightInventory::new();

    // if the new level has the current color as an allowed color, preserve it
    if let Some(color) = old_color {
        if current_level.allowed_colors[color] {
            inventory.current_color = old_color;
        }
    }
}

/// Kills player upon touching a HURT_BOX
pub fn kill_player_on_hurt_intersection(
    rapier_context: Query<&RapierContext>,
    q_player: Query<Entity, With<PlayerHurtMarker>>,
    q_hurt: Query<Entity, With<HurtMarker>>,
    mut ev_kill_player: EventWriter<KillPlayerEvent>,
) {
    let Ok(rapier) = rapier_context.get_single() else {
        return;
    };
    let Ok(player) = q_player.get_single() else {
        return;
    };

    for hurt in q_hurt.iter() {
        if rapier.intersection_pair(player, hurt) == Some(true) {
            ev_kill_player.send(KillPlayerEvent);
            return;
        }
    }
}

/// Systems that kill the player should send this event instead of ResetLevel::Respawn, so the
/// transition is started.
#[derive(Event)]
pub struct KillPlayerEvent;

#[derive(Resource)]
pub struct KillAnimationCallbacks {
    // once the screen is completely black
    cb1: SystemId,
    // once the screen is ready for play
    cb2: SystemId,
}

impl FromWorld for KillAnimationCallbacks {
    fn from_world(world: &mut World) -> Self {
        KillAnimationCallbacks {
            cb1: world.register_system(after_slide_to_black),
            cb2: world.register_system(after_slide_from_black),
        }
    }
}

pub fn start_kill_animation(
    mut ev_transition_camera: EventWriter<CameraTransitionEvent>,
    callbacks: Res<KillAnimationCallbacks>,
    cur_game_state: Res<State<GameState>>,
    mut next_game_state: ResMut<NextState<GameState>>,
    mut next_anim_state: ResMut<NextState<AnimationState>>,
) {
    if *cur_game_state.get() == GameState::Animating {
        return;
    }
    ev_transition_camera.send(CameraTransitionEvent {
        duration: Duration::from_millis(400),
        ease_fn: EaseFunction::SineInOut,
        callback: Some(callbacks.cb1),
        effect: CameraTransition::SlideToBlack,
    });
    next_game_state.set(GameState::Animating);
    next_anim_state.set(AnimationState::Respawn);
}

pub fn after_slide_to_black(
    mut ev_transition_camera: EventWriter<CameraTransitionEvent>,
    mut ev_reset_level: EventWriter<ResetLevel>,
    callbacks: Res<KillAnimationCallbacks>,
) {
    ev_transition_camera.send(CameraTransitionEvent {
        duration: Duration::from_millis(400),
        ease_fn: EaseFunction::SineInOut,
        callback: Some(callbacks.cb2),
        effect: CameraTransition::SlideFromBlack,
    });
    ev_reset_level.send(ResetLevel::Respawn);
}

pub fn after_slide_from_black(mut next_game_state: ResMut<NextState<GameState>>) {
    next_game_state.set(GameState::Playing);
}
