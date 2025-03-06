use bevy::prelude::*;
use bevy_ecs_ldtk::prelude::*;
use bevy_rapier2d::prelude::*;
use enum_map::EnumMap;

use crate::{animation::AnimationConfig, light::LightColor, player::PlayerHurtMarker};

use super::{entity::FixedEntityBundle, CurrentLevel, LevelSystems};

pub struct CrystalShardPlugin;

impl Plugin for CrystalShardPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CrystalShardMods>()
            .register_ldtk_entity::<CrystalShardBundle>("CrystalShard")
            .add_systems(
                PreUpdate,
                add_crystal_shard_sprites.in_set(LevelSystems::Processing),
            )
            .add_systems(Update, reset_shards.in_set(LevelSystems::Reset))
            .add_systems(
                FixedUpdate,
                on_player_intersect_shard.in_set(LevelSystems::Simulation),
            );
    }
}

#[derive(Component, Debug)]
pub struct CrystalShard {
    light_color: LightColor,
}

impl From<&EntityInstance> for CrystalShard {
    fn from(value: &EntityInstance) -> Self {
        let light_color = value
            .get_enum_field("light_color")
            .expect("All crystal shards should have a light_color enum field")
            .into();

        Self { light_color }
    }
}

#[derive(Bundle, LdtkEntity)]
pub struct CrystalShardBundle {
    #[from_entity_instance]
    shard: CrystalShard,
    #[from_entity_instance]
    physics: FixedEntityBundle,
    #[default]
    sensor: Sensor,
}

#[derive(Resource, Default)]
/// Sets a value to true if the light color was obtained from a crystal in the current level
pub struct CrystalShardMods(EnumMap<LightColor, bool>);

pub fn add_crystal_shard_sprites(
    mut commands: Commands,
    q_shards: Query<(Entity, &CrystalShard), Added<CrystalShard>>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    asset_server: Res<AssetServer>,
) {
    const CRYSTAL_SHARD_FRAMES: usize = 7;
    const CRYSTAL_SHARD_ROWS: usize = 4;

    let texture_atlas_layout = texture_atlas_layouts.add(TextureAtlasLayout::from_grid(
        UVec2::new(12, 16),
        CRYSTAL_SHARD_FRAMES as u32,
        CRYSTAL_SHARD_ROWS as u32,
        None,
        None,
    ));

    let shard_row = |shard: &CrystalShard| match shard.light_color {
        LightColor::Blue => 0,
        LightColor::Green => 1,
        LightColor::Purple => 2,
        LightColor::White => 3,
    };

    for (shard_entity, shard) in q_shards.iter() {
        let start_index = shard_row(shard) * CRYSTAL_SHARD_FRAMES;
        commands.entity(shard_entity).insert((
            Sprite {
                image: asset_server.load("crystal_shard_sheet.png"),
                texture_atlas: Some(TextureAtlas {
                    layout: texture_atlas_layout.clone(),
                    index: start_index,
                }),
                ..default()
            },
            AnimationConfig::new(
                start_index,
                start_index + CRYSTAL_SHARD_FRAMES - 1,
                12,
                true,
            ),
        ));
    }
}

pub fn reset_shards(
    mut q_shards: Query<&mut Visibility, With<CrystalShard>>,
    mut current_level: ResMut<CurrentLevel>,
    mut shard_mods: ResMut<CrystalShardMods>,
) {
    for mut visibility in q_shards.iter_mut() {
        *visibility = Visibility::Visible;
    }
    for (color, is_temporary) in shard_mods.0.iter_mut() {
        if *is_temporary {
            current_level.allowed_colors[color] = false;
        }
        // undo all temporary modifications on a level switch
        *is_temporary = false;
    }
}

pub fn on_player_intersect_shard(
    mut q_shards: Query<(Entity, &CrystalShard, &mut Visibility)>,
    mut q_player: Query<Entity, With<PlayerHurtMarker>>,
    rapier_context: Query<&RapierContext>,
    mut current_level: ResMut<CurrentLevel>,
    mut shard_mods: ResMut<CrystalShardMods>,
) {
    let Ok(rapier_context) = rapier_context.get_single() else {
        return;
    };
    let Ok(player_entity) = q_player.get_single_mut() else {
        return;
    };
    for (shard_entity, shard, mut visibility) in q_shards.iter_mut() {
        if let Some(true) = rapier_context.intersection_pair(player_entity, shard_entity) {
            if !current_level.allowed_colors[shard.light_color] {
                // only mark as temporary modification if not actually allowed
                shard_mods.0[shard.light_color] = true;
                current_level.allowed_colors[shard.light_color] = true;
            }

            // TODO: add fancy cutscene :)
            *visibility = Visibility::Hidden;
        }
    }
}
