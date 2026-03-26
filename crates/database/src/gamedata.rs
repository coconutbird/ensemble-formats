//! Parser for `gamedata.xml.xmb` — global game constants and settings.
//!
//! Contains resource definitions, population types, difficulty settings,
//! code proto-object mappings, and various gameplay constants.

use alloc::string::String;
use alloc::vec::Vec;
use serde::Deserialize;

use crate::node_ext::expect_root;

/// Global game data from `gamedata.xml`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct GameData {
    /// Resource definitions wrapper.
    #[serde(rename = "Resources")]
    pub resources: Option<ResourcesWrapper>,
    /// Rate definitions wrapper.
    #[serde(rename = "Rates")]
    pub rates: Option<RatesWrapper>,
    /// Population type names wrapper.
    #[serde(rename = "Pops")]
    pub pops: Option<PopsWrapper>,
    /// Ref count names wrapper.
    #[serde(rename = "RefCounts")]
    pub ref_counts: Option<RefCountsWrapper>,
    /// HUD item names wrapper.
    #[serde(rename = "HUDItems")]
    pub hud_items: Option<HUDItemsWrapper>,
    /// Flashable item names wrapper.
    #[serde(rename = "FlashableItems")]
    pub flashable_items: Option<FlashableItemsWrapper>,
    /// Unit flag names wrapper.
    #[serde(rename = "UnitFlags")]
    pub unit_flags: Option<UnitFlagsWrapper>,
    /// Squad flag names wrapper.
    #[serde(rename = "SquadFlags")]
    pub squad_flags: Option<SquadFlagsWrapper>,
    /// Player state names wrapper.
    #[serde(rename = "PlayerStates")]
    pub player_states: Option<PlayerStatesWrapper>,
    /// Code proto-object mappings wrapper.
    #[serde(rename = "CodeProtoObjects")]
    pub code_proto_objects: Option<CodeProtoObjectsWrapper>,
    /// Code object type mappings wrapper.
    #[serde(rename = "CodeObjectTypes")]
    pub code_object_types: Option<CodeObjectTypesWrapper>,

    // Difficulty settings
    #[serde(rename = "DifficultyEasy")]
    pub difficulty_easy: Option<f32>,
    #[serde(rename = "DifficultyNormal")]
    pub difficulty_normal: Option<f32>,
    #[serde(rename = "DifficultyHard")]
    pub difficulty_hard: Option<f32>,
    #[serde(rename = "DifficultyLegendary")]
    pub difficulty_legendary: Option<f32>,
    #[serde(rename = "DifficultyDefault")]
    pub difficulty_default: Option<f32>,

    // Supply pad settings
    #[serde(rename = "UnscSupplyPadBonus")]
    pub unsc_supply_pad_bonus: Option<f32>,
    #[serde(rename = "UnscSupplyPadBreakEvenPoint")]
    pub unsc_supply_pad_break_even_point: Option<f32>,
    #[serde(rename = "CovSupplyPadBonus")]
    pub cov_supply_pad_bonus: Option<f32>,
    #[serde(rename = "CovSupplyPadBreakEvenPoint")]
    pub cov_supply_pad_break_even_point: Option<f32>,

    // Transport settings
    #[serde(rename = "TransportMax")]
    pub transport_max: Option<i32>,

    // Cryo/freeze settings
    #[serde(rename = "TimeFrozenToThaw")]
    pub time_frozen_to_thaw: Option<f32>,
    #[serde(rename = "TimeFreezingToThaw")]
    pub time_freezing_to_thaw: Option<f32>,
    #[serde(rename = "DefaultCryoPoints")]
    pub default_cryo_points: Option<f32>,
    #[serde(rename = "DefaultThawSpeed")]
    pub default_thaw_speed: Option<f32>,
    #[serde(rename = "FreezingSpeedModifier")]
    pub freezing_speed_modifier: Option<f32>,
    #[serde(rename = "FreezingDamageModifier")]
    pub freezing_damage_modifier: Option<f32>,
    #[serde(rename = "FrozenDamageModifier")]
    pub frozen_damage_modifier: Option<f32>,

    // Leader power charge
    #[serde(rename = "LeaderPowerChargeResource")]
    pub leader_power_charge_resource: Option<String>,
    #[serde(rename = "LeaderPowerChargeRate")]
    pub leader_power_charge_rate: Option<String>,

    // AI difficulty
    #[serde(rename = "DifficultySPCAIDefault")]
    pub difficulty_spcai_default: Option<f32>,

    // Combat modifiers
    #[serde(rename = "GarrisonDamageMultiplier")]
    pub garrison_damage_multiplier: Option<f32>,
    #[serde(rename = "ConstructionDamageMultiplier")]
    pub construction_damage_multiplier: Option<f32>,
    #[serde(rename = "CaptureDecayRate")]
    pub capture_decay_rate: Option<f32>,
    #[serde(rename = "UnitLeashLength")]
    pub unit_leash_length: Option<f32>,

    // Cloaking
    #[serde(rename = "CloakingDelay")]
    pub cloaking_delay: Option<f32>,
    #[serde(rename = "ReCloakDelay")]
    pub recloak_delay: Option<f32>,
    #[serde(rename = "CloakDetectFrequency")]
    pub cloak_detect_frequency: Option<f32>,

    // Shields
    #[serde(rename = "ShieldBarColor")]
    pub shield_bar_color: Option<String>,
    #[serde(rename = "ShieldRegenDelay")]
    pub shield_regen_delay: Option<f32>,
    #[serde(rename = "ShieldRegenTime")]
    pub shield_regen_time: Option<f32>,
    #[serde(rename = "AmmoBarColor")]
    pub ammo_bar_color: Option<String>,

    // Projectiles
    #[serde(rename = "ProjectileGravity")]
    pub projectile_gravity: Option<f32>,
    #[serde(rename = "HeightBonusDamage")]
    pub height_bonus_damage: Option<f32>,
    #[serde(rename = "ProjectileTumbleRate")]
    pub projectile_tumble_rate: Option<f32>,
    #[serde(rename = "TrackInterceptDistance")]
    pub track_intercept_distance: Option<f32>,

    // Attack tolerances
    #[serde(rename = "StationaryTargetAttackToleranceAngle")]
    pub stationary_target_attack_tolerance_angle: Option<f32>,
    #[serde(rename = "MovingTargetAttackToleranceAngle")]
    pub moving_target_attack_tolerance_angle: Option<f32>,
    #[serde(rename = "MovingTargetTrackingAttackToleranceAngle")]
    pub moving_target_tracking_attack_tolerance_angle: Option<f32>,
    #[serde(rename = "MovingTargetRangeMultiplier")]
    pub moving_target_range_multiplier: Option<f32>,

    // Revealers
    #[serde(rename = "AttackedRevealerLOS")]
    pub attacked_revealer_los: Option<f32>,
    #[serde(rename = "AttackedRevealerLifespan")]
    pub attacked_revealer_lifespan: Option<f32>,
    #[serde(rename = "AttackRevealerLOS")]
    pub attack_revealer_los: Option<f32>,
    #[serde(rename = "AttackRevealerLifespan")]
    pub attack_revealer_lifespan: Option<f32>,
    #[serde(rename = "MinimumRevealerSize")]
    pub minimum_revealer_size: Option<f32>,

    // Ratings
    #[serde(rename = "AttackRatingMultiplier")]
    pub attack_rating_multiplier: Option<f32>,
    #[serde(rename = "DefenseRatingMultiplier")]
    pub defense_rating_multiplier: Option<f32>,
    #[serde(rename = "GoodAgainstMinAttackGrade")]
    pub good_against_min_attack_grade: Option<f32>,
    #[serde(rename = "GoodAgainstMinAttackRating")]
    pub good_against_min_attack_rating: Option<f32>,

    // Opportunity
    #[serde(rename = "OpportunityBeingAttackedPriBonus")]
    pub opportunity_being_attacked_pri_bonus: Option<f32>,
    #[serde(rename = "OpportunityDistPriFactor")]
    pub opportunity_dist_pri_factor: Option<f32>,

    // Misc combat
    #[serde(rename = "ChanceToRocket")]
    pub chance_to_rocket: Option<f32>,
    #[serde(rename = "BuildingSelfDestructTime")]
    pub building_self_destruct_time: Option<f32>,
    #[serde(rename = "GoodAgainstReticle")]
    pub good_against_reticle: Option<String>,

    // Economy
    #[serde(rename = "TributeAmount")]
    pub tribute_amount: Option<f32>,
    #[serde(rename = "TributeCost")]
    pub tribute_cost: Option<f32>,
    #[serde(rename = "AirStrikeLoiterTime")]
    pub air_strike_loiter_time: Option<f32>,

    // Damage bank
    #[serde(rename = "MaxDamageBankPctAdjust")]
    pub max_damage_bank_pct_adjust: Option<f32>,
    #[serde(rename = "DamageBankTimer")]
    pub damage_bank_timer: Option<f32>,

    // Corpses
    #[serde(rename = "MaxNumCorpses")]
    pub max_num_corpses: Option<i32>,
    #[serde(rename = "MaxCorpseDisposalCount")]
    pub max_corpse_disposal_count: Option<i32>,
    #[serde(rename = "InfantryCorpseDecayTime")]
    pub infantry_corpse_decay_time: Option<f32>,
    #[serde(rename = "CorpseSinkingSpacing")]
    pub corpse_sinking_spacing: Option<f32>,

    // Infection
    #[serde(rename = "InfectionMap")]
    pub infection_map: Option<String>,

    // Pathing
    #[serde(rename = "MaxSquadPathsPerFrame")]
    pub max_squad_paths_per_frame: Option<i32>,
    #[serde(rename = "MaxPlatoonPathsPerFrame")]
    pub max_platoon_paths_per_frame: Option<i32>,

    // XP
    #[serde(rename = "DamageReceivedXPFactor")]
    pub damage_received_xp_factor: Option<f32>,

    // Fatalities
    #[serde(rename = "FatalityTransitionScale")]
    pub fatality_transition_scale: Option<f32>,
    #[serde(rename = "FatalityMaxTransitionTime")]
    pub fatality_max_transition_time: Option<f32>,
    #[serde(rename = "FatalityCooldown")]
    pub fatality_cooldown: Option<f32>,
    #[serde(rename = "FatalityPositionOffsetTolerance")]
    pub fatality_position_offset_tolerance: Option<f32>,
    #[serde(rename = "FatalityOrientationOffsetTolerance")]
    pub fatality_orientation_offset_tolerance: Option<f32>,
    #[serde(rename = "FatalityExclusionRange")]
    pub fatality_exclusion_range: Option<f32>,

    // Recycle
    #[serde(rename = "RecyleRefundRate")]
    pub recyle_refund_rate: Option<f32>,

    // Burning
    #[serde(rename = "BurningEffectLimits")]
    pub burning_effect_limits: Option<BurningEffectLimitsWrapper>,

    // Base rebuild
    #[serde(rename = "BaseRebuildTimer")]
    pub base_rebuild_timer: Option<f32>,

    // Objective arrows
    #[serde(rename = "ObjectiveArrowRadialOffset")]
    pub objective_arrow_radial_offset: Option<f32>,
    #[serde(rename = "ObjectiveArrowSwitchOffset")]
    pub objective_arrow_switch_offset: Option<f32>,
    #[serde(rename = "ObjectiveArrowMaxIndex")]
    pub objective_arrow_max_index: Option<f32>,
    #[serde(rename = "ObjectiveArrowYOffset")]
    pub objective_arrow_y_offset: Option<f32>,

    // Overrun
    #[serde(rename = "OverrunMinVel")]
    pub overrun_min_vel: Option<f32>,
    #[serde(rename = "OverrunJumpForce")]
    pub overrun_jump_force: Option<f32>,
    #[serde(rename = "OverrunDistance")]
    pub overrun_distance: Option<f32>,

    // Co-op
    #[serde(rename = "CoopResourceSplitRate")]
    pub coop_resource_split_rate: Option<f32>,

    // Game over
    #[serde(rename = "GameOverDelay")]
    pub game_over_delay: Option<f32>,

    // Hero settings
    #[serde(rename = "HeroDownedLOS")]
    pub hero_downed_los: Option<f32>,
    #[serde(rename = "HeroHPRegenTime")]
    pub hero_hp_regen_time: Option<f32>,
    #[serde(rename = "HeroRevivalDistance")]
    pub hero_revival_distance: Option<f32>,
    #[serde(rename = "HeroPercentHPRevivalThreshhold")]
    pub hero_percent_hp_revival_threshhold: Option<f32>,
    #[serde(rename = "MaxDeadHeroTransportDist")]
    pub max_dead_hero_transport_dist: Option<f32>,

    // Transport settings (extended)
    #[serde(rename = "TransportClearRadiusScale")]
    pub transport_clear_radius_scale: Option<f32>,
    #[serde(rename = "TransportMaxSearchRadiusScale")]
    pub transport_max_search_radius_scale: Option<f32>,
    #[serde(rename = "TransportMaxSearchLocations")]
    pub transport_max_search_locations: Option<i32>,
    #[serde(rename = "TransportBlockTime")]
    pub transport_block_time: Option<f32>,
    #[serde(rename = "TransportLoadBlockTime")]
    pub transport_load_block_time: Option<f32>,
    #[serde(rename = "TransportMaxBlockAttempts")]
    pub transport_max_block_attempts: Option<f32>,
    #[serde(rename = "TransportIncomingHeight")]
    pub transport_incoming_height: Option<f32>,
    #[serde(rename = "TransportIncomingOffset")]
    pub transport_incoming_offset: Option<f32>,
    #[serde(rename = "TransportOutgoingHeight")]
    pub transport_outgoing_height: Option<f32>,
    #[serde(rename = "TransportOutgoingOffset")]
    pub transport_outgoing_offset: Option<f32>,
    #[serde(rename = "TransportPickupHeight")]
    pub transport_pickup_height: Option<f32>,
    #[serde(rename = "TransportDropoffHeight")]
    pub transport_dropoff_height: Option<f32>,

    // Hitch
    #[serde(rename = "HitchOffset")]
    pub hitch_offset: Option<f32>,

    // Animal life (AL) settings
    #[serde(rename = "ALMaxWanderFrequency")]
    pub al_max_wander_frequency: Option<f32>,
    #[serde(rename = "ALPredatorCheckFrequency")]
    pub al_predator_check_frequency: Option<f32>,
    #[serde(rename = "ALPreyCheckFrequency")]
    pub al_prey_check_frequency: Option<f32>,
    #[serde(rename = "ALOppCheckRadius")]
    pub al_opp_check_radius: Option<f32>,
    #[serde(rename = "ALFleeDistance")]
    pub al_flee_distance: Option<f32>,
    #[serde(rename = "ALFleeMovementModifier")]
    pub al_flee_movement_modifier: Option<f32>,
    #[serde(rename = "ALMinWanderDistance")]
    pub al_min_wander_distance: Option<f32>,
    #[serde(rename = "ALMaxWanderDistance")]
    pub al_max_wander_distance: Option<f32>,
    #[serde(rename = "ALSpawnerCheckFrequency")]
    pub al_spawner_check_frequency: Option<f32>,

    // Dot damage
    #[serde(rename = "Dot")]
    pub dot: Option<String>,

    // Squad leash
    #[serde(rename = "SquadLeashLength")]
    pub squad_leash_length: Option<f32>,
    #[serde(rename = "SquadAggroLength")]
    pub squad_aggro_length: Option<f32>,
}

/// Wrapper for `<BurningEffectLimits>`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct BurningEffectLimitsWrapper {
    #[serde(rename = "@DefaultLimit")]
    pub default_limit: Option<i32>,
    #[serde(rename = "BurningEffectLimitEntry", default)]
    pub entries: Vec<BurningEffectLimitEntry>,
}

/// A burning effect limit entry.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct BurningEffectLimitEntry {
    #[serde(rename = "@Limit")]
    pub limit: Option<i32>,
    #[serde(rename = "$text", default)]
    pub value: String,
}

/// Wrapper for `<Resources>` containing `<Resource>` children.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ResourcesWrapper {
    #[serde(rename = "Resource", default)]
    pub entries: Vec<ResourceDef>,
}

/// A resource definition.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ResourceDef {
    #[serde(rename = "$text", default)]
    pub name: String,
    #[serde(rename = "@Deductable")]
    pub deductable: Option<bool>,
}

/// Wrapper for `<Rates>` containing `<Rate>` children.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RatesWrapper {
    #[serde(rename = "Rate", default)]
    pub entries: Vec<String>,
}

/// Wrapper for `<Pops>` containing `<Pop>` children.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PopsWrapper {
    #[serde(rename = "Pop", default)]
    pub entries: Vec<String>,
}

/// Wrapper for `<RefCounts>` containing `<RefCount>` children.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RefCountsWrapper {
    #[serde(rename = "RefCount", default)]
    pub entries: Vec<String>,
}

/// Wrapper for `<HUDItems>` containing `<HUDItem>` children.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct HUDItemsWrapper {
    #[serde(rename = "HUDItem", default)]
    pub entries: Vec<String>,
}

/// Wrapper for `<FlashableItems>` containing `<Item>` children.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct FlashableItemsWrapper {
    #[serde(rename = "Item", default)]
    pub entries: Vec<String>,
}

/// Wrapper for `<UnitFlags>` containing `<UnitFlag>` children.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UnitFlagsWrapper {
    #[serde(rename = "UnitFlag", default)]
    pub entries: Vec<String>,
}

/// Wrapper for `<SquadFlags>` containing `<SquadFlag>` children.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SquadFlagsWrapper {
    #[serde(rename = "SquadFlag", default)]
    pub entries: Vec<String>,
}

/// Wrapper for `<PlayerStates>` containing `<PlayerState>` children.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PlayerStatesWrapper {
    #[serde(rename = "PlayerState", default)]
    pub entries: Vec<String>,
}

/// Wrapper for `<CodeProtoObjects>`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CodeProtoObjectsWrapper {
    #[serde(rename = "CodeProtoObject", default)]
    pub entries: Vec<CodeProtoObject>,
}

/// A code proto-object mapping.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CodeProtoObject {
    #[serde(rename = "@Type", default)]
    pub object_type: String,
    #[serde(rename = "$text", default)]
    pub proto_name: String,
}

/// Wrapper for `<CodeObjectTypes>`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CodeObjectTypesWrapper {
    #[serde(rename = "CodeObjectType", default)]
    pub entries: Vec<CodeObjectType>,
}

/// A code object type mapping.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CodeObjectType {
    #[serde(rename = "@Type", default)]
    pub object_type: String,
    #[serde(rename = "$text", default)]
    pub value: String,
}

/// Parse game data from a `gamedata.xml.xmb` document.
pub fn parse(doc: &xmb::Document) -> crate::Result<GameData> {
    let root = expect_root(doc, "GameData")?;
    let gd: GameData = bdt_serde::from_node(root)?;
    Ok(gd)
}
