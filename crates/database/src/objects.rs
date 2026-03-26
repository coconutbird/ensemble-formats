//! Parser for `objects.xml.xmb` — the master proto-object database.
//!
//! Each `<Object>` element describes a game entity: unit, building, projectile, etc.

use alloc::string::String;
use alloc::vec::Vec;
use serde::Deserialize;

use crate::node_ext::expect_root;

/// A single proto-object definition from `objects.xml`.
///
/// Every field below is confirmed present in the engine's `BProtoObject::loadFromXml`
/// (IDA @ `0x140341620`) and cross-referenced against the 2006 source leak
/// (`BProtoObject::load`). Fields are sorted alphabetically within each group.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProtoObject {
    /// Object name (unique key), e.g. `"unsc_veh_warthog_01"`.
    #[serde(rename = "@name", default)]
    pub name: String,

    /// Numeric ID assigned in the XML (`id` attribute, often 0).
    #[serde(rename = "@id")]
    pub id: Option<i32>,

    /// Unused leftover from development — typo for `@id`, ignored by the engine.
    #[serde(rename = "@is")]
    pub is: Option<i32>,

    /// Database ID assigned by the engine (`dbid` attribute).
    #[serde(rename = "@dbid")]
    pub dbid: Option<i32>,

    /// Whether this is an update entry (from `objects_update.xml.xmb`).
    #[serde(rename = "@update")]
    pub update: Option<bool>,

    /// Ability trigger script names.
    #[serde(rename = "Ability", default)]
    pub ability: Vec<String>,

    /// Ability command ID (resolved via `BDatabase::getAbilityIDFromName`).
    #[serde(rename = "AbilityCommand")]
    pub ability_command: Option<String>,

    /// Acceleration override for projectiles. Stored on `mpStaticData`.
    #[serde(rename = "Acceleration")]
    pub acceleration: Option<f32>,

    /// Chance (0.0–1.0) the unit will perform an active scan. Has `@radiusScale` sub-attr.
    #[serde(rename = "ActiveScanChance")]
    pub active_scan_chance: Option<f32>,

    /// Resource type and amount to add on creation. Has `@Amount` sub-attr.
    #[serde(rename = "AddResource")]
    pub add_resource: Option<String>,

    /// AI asset value adjustment factor for AI threat calculations.
    #[serde(rename = "AIAssetValueAdjust")]
    pub ai_asset_value_adjust: Option<f32>,

    /// Maximum ammo capacity.
    #[serde(rename = "AmmoMax")]
    pub ammo_max: Option<f32>,

    /// Ammo regeneration rate per second.
    #[serde(rename = "AmmoRegenRate")]
    pub ammo_regen_rate: Option<f32>,

    /// DPS grade for attack rating display. Stored as `String` due to malformed data (`"190.9.0"`).
    #[serde(rename = "AttackGradeDPS")]
    pub attack_grade_dps: Option<String>,

    /// Auto lock-down type enum (resolved via `BDatabase::getAutoLockDownType`).
    #[serde(rename = "AutoLockDown")]
    pub auto_lock_down: Option<String>,

    /// Proto object name for the auto-spawned parking lot. Has `@Rotation` and `@Offset` sub-attrs.
    #[serde(rename = "AutoParkingLot")]
    pub auto_parking_lot: Option<String>,

    /// Proto squad to auto-train when the building finishes construction.
    #[serde(rename = "AutoTrainOnBuilt")]
    pub auto_train_on_built: Option<String>,

    /// Object type ID for the beam head visual (e.g. Scarab beam). (unsure)
    #[serde(rename = "BeamHead")]
    pub beam_head: Option<String>,

    /// Object type ID for the beam tail visual (e.g. Scarab beam). (unsure)
    #[serde(rename = "BeamTail")]
    pub beam_tail: Option<String>,

    /// Proto object ref that blocks this object's movement.
    #[serde(rename = "BlockMovementObject")]
    pub block_movement_object: Option<String>,

    /// Bounty value awarded on kill.
    #[serde(rename = "Bounty")]
    pub bounty: Option<f32>,

    /// Build offset vector (x, y, z) for building placement.
    #[serde(rename = "BuildOffset")]
    pub build_offset: Option<String>,

    /// Construction hit points. Only set if `mBuildPoints == 0.0`.
    #[serde(rename = "BuildPoints")]
    pub build_points: Option<f32>,

    /// Rotation angle for building placement.
    #[serde(rename = "BuildRotation")]
    pub build_rotation: Option<f32>,

    /// Proto object ref used for build stats display (e.g. shows another object's stats).
    #[serde(rename = "BuildStatsObject")]
    pub build_stats_object: Option<String>,

    /// Building strength display ID (resolved via `BDatabase::getProtoBuildingStrengthID`).
    #[serde(rename = "BuildingStrengthDisplay")]
    pub building_strength_display: Option<String>,

    /// Capture cost per civ. Complex sub-element with `@Civ` attr.
    #[serde(rename = "CaptureCost")]
    pub capture_cost: Option<String>,

    /// Child objects (parking lots, sockets, rally points, etc.). Contains `<Object>` sub-elements.
    #[serde(rename = "ChildObjects")]
    pub child_objects: Option<String>,

    /// Damage scalar applied to child objects attached to this object.
    #[serde(rename = "ChildObjectDamageTakenScalar")]
    pub child_object_damage_taken_scalar: Option<f32>,

    /// Combat value used for AI scoring and game statistics.
    #[serde(rename = "CombatValue")]
    pub combat_value: Option<f32>,

    /// Commands available on this building (Train, Research, Build, etc.). Has `@Type`/`@Position` sub-attrs.
    #[serde(rename = "Command", default)]
    pub command: Vec<String>,

    /// Object types this unit can contain/transport.
    #[serde(rename = "Contain", default)]
    pub contain: Vec<String>,

    /// Corpse death visual (proto visual name).
    #[serde(rename = "CorpseDeath")]
    pub corpse_death: Option<String>,

    /// Cost element — loaded via `BCost::load`. Contains resource sub-elements.
    #[serde(rename = "Cost")]
    pub cost: Option<String>,

    /// Cost escalation factor — multiplied per existing instance.
    #[serde(rename = "CostEscalation")]
    pub cost_escalation: Option<f32>,

    /// Proto object names whose count drives cost escalation.
    #[serde(rename = "CostEscalationObject", default)]
    pub cost_escalation_object: Vec<String>,

    /// Damage type sub-element (loaded via `loadDamageType`).
    #[serde(rename = "DamageType")]
    pub damage_type: Option<String>,

    /// Unused leftover from development — `DazeResist` is read from squads, not objects.
    #[serde(rename = "DazeResist")]
    pub daze_resist: Option<f32>,

    /// Death fade delay before fading begins (seconds).
    #[serde(rename = "DeathFadeDelayTime")]
    pub death_fade_delay_time: Option<f32>,

    /// Death fade duration (seconds).
    #[serde(rename = "DeathFadeTime")]
    pub death_fade_time: Option<f32>,

    /// Proto object ref to spawn on death.
    #[serde(rename = "DeathReplacement")]
    pub death_replacement: Option<String>,

    /// Proto squad to spawn on death. Has `@CheckPos` and `@MaxPopCount` sub-attrs.
    #[serde(rename = "DeathSpawnSquad")]
    pub death_spawn_squad: Option<String>,

    /// Display name localized string ID.
    #[serde(rename = "DisplayNameID")]
    pub display_name_id: Option<i32>,

    /// Enemy rollover text localized string ID.
    #[serde(rename = "EnemyRolloverTextID")]
    pub enemy_rollover_text_id: Option<i32>,

    /// Exist sound bone attachment name. Has `@bone` sub-attr.
    #[serde(rename = "ExistSound")]
    pub exist_sound: Option<String>,

    /// Direction index for exiting garrisoned units. (unsure)
    #[serde(rename = "ExitFromDirection")]
    pub exit_from_direction: Option<i32>,

    /// Extended sound bank name for additional audio events.
    #[serde(rename = "ExtendedSoundBank")]
    pub extended_sound_bank: Option<String>,

    /// Boolean flags (e.g. `"ForceToGaiaPlayer"`, `"Invulnerable"`, `"NoRender"`).
    #[serde(rename = "Flag", default)]
    pub flags: Vec<String>,

    /// Unused leftover from development — loaded by the Flash UI system, not the object loader.
    #[serde(rename = "FlashUI")]
    pub flash_ui: Option<String>,

    /// Terrain flatten region 0 max X bound (building placement).
    #[serde(rename = "FlattenMaxX0")]
    pub flatten_max_x0: Option<f32>,

    /// Terrain flatten region 1 max X bound (building placement).
    #[serde(rename = "FlattenMaxX1")]
    pub flatten_max_x1: Option<f32>,

    /// Terrain flatten region 0 max Z bound (building placement).
    #[serde(rename = "FlattenMaxZ0")]
    pub flatten_max_z0: Option<f32>,

    /// Terrain flatten region 1 max Z bound (building placement).
    #[serde(rename = "FlattenMaxZ1")]
    pub flatten_max_z1: Option<f32>,

    /// Terrain flatten region 0 min X bound (building placement).
    #[serde(rename = "FlattenMinX0")]
    pub flatten_min_x0: Option<f32>,

    /// Terrain flatten region 1 min X bound (building placement).
    #[serde(rename = "FlattenMinX1")]
    pub flatten_min_x1: Option<f32>,

    /// Terrain flatten region 0 min Z bound (building placement).
    #[serde(rename = "FlattenMinZ0")]
    pub flatten_min_z0: Option<f32>,

    /// Terrain flatten region 1 min Z bound (building placement).
    #[serde(rename = "FlattenMinZ1")]
    pub flatten_min_z1: Option<f32>,

    /// Flight level height for air units.
    #[serde(rename = "FlightLevel")]
    pub flight_level: Option<f32>,

    /// Fuel capacity for projectiles (determines range).
    #[serde(rename = "Fuel")]
    pub fuel: Option<f32>,

    /// Gaia rollover text localized string ID. Has optional `@civ` sub-attr.
    #[serde(rename = "GaiaRolloverTextID")]
    pub gaia_rollover_text_id: Option<i32>,

    /// Squad mode for garrisoned units (resolved via `BDatabase::getSquadMode`).
    #[serde(rename = "GarrisonSquadMode")]
    pub garrison_squad_mode: Option<String>,

    /// Time in seconds for garrison action. (unsure)
    #[serde(rename = "GarrisonTime")]
    pub garrison_time: Option<f32>,

    /// Gather link — object type for resource linking. Has `@Radius`, `@Target`, `@Self` sub-attrs.
    #[serde(rename = "GatherLink")]
    pub gather_link: Option<String>,

    /// Max number of gatherers that can work on this resource.
    #[serde(rename = "GathererLimit")]
    pub gatherer_limit: Option<i32>,

    /// Goto type enum: where the unit goes to (resolved via `BDatabase::getGotoType`).
    #[serde(rename = "GotoType")]
    pub goto_type: Option<String>,

    /// Ground IK bone name and settings. Has `@factor` sub-attr. (unsure)
    #[serde(rename = "GroundIK")]
    pub ground_ik: Option<String>,

    /// Ground IK tilt settings. Has `@factor` sub-attr and bone name as text.
    #[serde(rename = "GroundIKTilt")]
    pub ground_ik_tilt: Option<String>,

    /// Hardpoints (turret mount points).
    #[serde(rename = "Hardpoint", default)]
    pub hardpoints: Vec<Hardpoint>,

    /// Hit points.
    #[serde(rename = "Hitpoints")]
    pub hitpoints: Option<f32>,

    /// Hit zones for per-bone damage. Has `@Hitpoints`, `@Shieldpoints`, `@Active`, `@HasShields` sub-attrs.
    #[serde(rename = "HitZone", default)]
    pub hit_zone: Vec<String>,

    /// Hovering rumble controller data. Has `@LeftRumbleType`, `@RightRumbleType`, `@Duration`, etc.
    #[serde(rename = "HoveringRumble")]
    pub hovering_rumble: Option<String>,

    /// HP bar display ID. Has `@sizeX`, `@sizeY`, `@offset` sub-attrs.
    #[serde(rename = "HPBar")]
    pub hp_bar: Option<String>,

    /// Impact decal configuration. Has `@sizeX`, `@sizeZ`, `@timeFullyOpaque`, `@fadeOutTime`, `@orientation` sub-attrs.
    #[serde(rename = "ImpactDecal")]
    pub impact_decal: Option<String>,

    /// Kill beam proto object ref (Scarab scenario hack).
    #[serde(rename = "KillBeam")]
    pub kill_beam: Option<String>,

    /// Proto object level (e.g. tech level). Read as int.
    #[serde(rename = "Level")]
    pub level: Option<i32>,

    /// Level-up visual effect proto object ref.
    #[serde(rename = "LevelUpEffect")]
    pub level_up_effect: Option<String>,

    /// Lifespan in seconds (read as float, engine converts to milliseconds).
    #[serde(rename = "Lifespan")]
    pub lifespan: Option<f32>,

    /// Line of sight radius.
    #[serde(rename = "LOS")]
    pub los: Option<f32>,

    /// Maximum number of contained/transported units.
    #[serde(rename = "MaxContained")]
    pub max_contained: Option<i32>,

    /// Maximum number of simultaneous flame effects on this object.
    #[serde(rename = "MaxFlameEffects")]
    pub max_flame_effects: Option<i32>,

    /// Maximum projectile height for arc calculations.
    #[serde(rename = "MaxProjectileHeight")]
    pub max_projectile_height: Option<f32>,

    /// Maximum velocity (overrides auto-calculated `Velocity * cMaxVelocityMultiplier`).
    #[serde(rename = "MaxVelocity")]
    pub max_velocity: Option<f32>,

    /// Minimap color. Has `@red`, `@green`, `@blue` sub-attrs.
    #[serde(rename = "MinimapColor")]
    pub minimap_color: Option<String>,

    /// Minimap icon path. Has `@size` sub-attr.
    #[serde(rename = "MinimapIcon")]
    pub minimap_icon: Option<String>,

    /// Unused leftover from development — not read by `BProtoObject::loadFromXml`.
    #[serde(rename = "MinimapIconName")]
    pub minimap_icon_name: Option<String>,

    /// Movement type string: `"Land"`, `"Air"`, `"Flood"`, etc.
    #[serde(rename = "MovementType")]
    pub movement_type: Option<String>,

    /// Number of conversions (Flood mechanic). (unsure)
    #[serde(rename = "NumConversions")]
    pub num_conversions: Option<i32>,

    /// Number of stasis fields required to immobilize this unit.
    #[serde(rename = "NumStasisFieldsToStop")]
    pub num_stasis_fields_to_stop: Option<i32>,

    /// Object class: `"Object"`, `"Unit"`, `"Squad"`, `"Building"`, `"Projectile"`.
    #[serde(rename = "ObjectClass")]
    pub object_class: Option<String>,

    /// Object type tags (e.g. `"Military"`, `"CovInfantry"`). Resolved via `BDatabase::getObjectType`.
    #[serde(rename = "ObjectType", default)]
    pub object_types: Vec<String>,

    /// Obstruction radius X (half-width for pathfinding collision box).
    #[serde(rename = "ObstructionRadiusX")]
    pub obstruction_radius_x: Option<f32>,

    /// Obstruction radius Y (height for pathfinding collision box).
    #[serde(rename = "ObstructionRadiusY")]
    pub obstruction_radius_y: Option<f32>,

    /// Obstruction radius Z (half-depth for pathfinding collision box).
    #[serde(rename = "ObstructionRadiusZ")]
    pub obstruction_radius_z: Option<f32>,

    /// Parking lot max X bound (building placement).
    #[serde(rename = "ParkingMaxX")]
    pub parking_max_x: Option<f32>,

    /// Parking lot max Z bound (building placement).
    #[serde(rename = "ParkingMaxZ")]
    pub parking_max_z: Option<f32>,

    /// Parking lot min X bound (building placement).
    #[serde(rename = "ParkingMinX")]
    pub parking_min_x: Option<f32>,

    /// Parking lot min Z bound (building placement).
    #[serde(rename = "ParkingMinZ")]
    pub parking_min_z: Option<f32>,

    /// Initial velocity perturbance for projectiles. Sets `mFlagPerturbOnce`. Has `@minTime`/`@maxTime` sub-attrs.
    #[serde(rename = "PerturbInitialVelocity")]
    pub perturb_initial_velocity: Option<f32>,

    /// Chance (0.0–1.0) of projectile perturbance per tick.
    #[serde(rename = "PerturbanceChance")]
    pub perturbance_chance: Option<f32>,

    /// Maximum perturbance time interval (seconds).
    #[serde(rename = "PerturbanceMaxTime")]
    pub perturbance_max_time: Option<f32>,

    /// Minimum perturbance time interval (seconds).
    #[serde(rename = "PerturbanceMinTime")]
    pub perturbance_min_time: Option<f32>,

    /// Perturbance velocity magnitude.
    #[serde(rename = "PerturbanceVelocity")]
    pub perturbance_velocity: Option<f32>,

    /// Physics info name (resolved via `gPhysicsInfoManager::getOrCreate`).
    #[serde(rename = "PhysicsInfo")]
    pub physics_info: Option<String>,

    /// Physics replacement info name (used on death/destruction).
    #[serde(rename = "PhysicsReplacementInfo")]
    pub physics_replacement_info: Option<String>,

    /// Pick offset — Y offset for mouse picking ray test.
    #[serde(rename = "PickOffset")]
    pub pick_offset: Option<f32>,

    /// Pick priority enum (resolved via `BDatabase::getPickPriority`).
    #[serde(rename = "PickPriority")]
    pub pick_priority: Option<String>,

    /// Pick radius — radius for mouse picking ray test.
    #[serde(rename = "PickRadius")]
    pub pick_radius: Option<f32>,

    /// Placement rules enum (resolved via `BDatabase::getPlacementRules`).
    #[serde(rename = "PlacementRules")]
    pub placement_rules: Option<String>,

    /// Population cost entries. Has `@type` sub-attr for pop type name.
    #[serde(rename = "Pop", default)]
    pub pop: Vec<String>,

    /// Population cap addition entries. Has `@type` sub-attr for pop type name.
    #[serde(rename = "PopCapAddition", default)]
    pub pop_cap_addition: Vec<String>,

    /// Portrait icon path.
    #[serde(rename = "PortraitIcon")]
    pub portrait_icon: Option<String>,

    /// Proto power ID (resolved via `BDatabase::getProtoPowerIDByName`).
    #[serde(rename = "Power")]
    pub power: Option<String>,

    /// Prereq text localized string ID.
    #[serde(rename = "PrereqTextID")]
    pub prereq_text_id: Option<i32>,

    /// Rally point type: `"Military"` or `"Civilian"`.
    #[serde(rename = "RallyPoint")]
    pub rally_point: Option<String>,

    /// Ram dodge factor for ram-attack dodging. (unsure)
    #[serde(rename = "RamDodgeFactor")]
    pub ram_dodge_factor: Option<f32>,

    /// Rate element — resource gather/work rate. Has `@rate` sub-attr for rate type name.
    #[serde(rename = "Rate")]
    pub rate: Option<String>,

    /// Recovering visual effect proto object ref.
    #[serde(rename = "RecoveringEffect")]
    pub recovering_effect: Option<String>,

    /// Repair points (construction-like HP for repair actions).
    #[serde(rename = "RepairPoints")]
    pub repair_points: Option<f32>,

    /// Resource amount this object provides when gathered.
    #[serde(rename = "ResourceAmount")]
    pub resource_amount: Option<f32>,

    /// Reveal radius — fog-of-war reveal distance (distinct from LOS).
    #[serde(rename = "RevealRadius")]
    pub reveal_radius: Option<f32>,

    /// Reverse movement speed.
    #[serde(rename = "ReverseSpeed")]
    pub reverse_speed: Option<f32>,

    /// Role text localized string ID.
    #[serde(rename = "RoleTextID")]
    pub role_text_id: Option<i32>,

    /// Rollover text localized string ID.
    #[serde(rename = "RolloverTextID")]
    pub rollover_text_id: Option<i32>,

    /// Select type enum: `"Building"`, `"Resource"`, `"Unit"`, `"Rally"`.
    #[serde(rename = "SelectType")]
    pub select_type: Option<String>,

    /// Selected radius X (visual selection circle half-width).
    #[serde(rename = "SelectedRadiusX")]
    pub selected_radius_x: Option<f32>,

    /// Selected radius Z (visual selection circle half-depth).
    #[serde(rename = "SelectedRadiusZ")]
    pub selected_radius_z: Option<f32>,

    /// Shield hit points.
    #[serde(rename = "Shieldpoints")]
    pub shieldpoints: Option<f32>,

    /// Shield type — object type ID for shield (e.g. energy shield visual).
    #[serde(rename = "ShieldType")]
    pub shield_type: Option<String>,

    /// Single bone IK settings. (unsure)
    #[serde(rename = "SingleBoneIK")]
    pub single_bone_ik: Option<String>,

    /// Socket object type and settings. Has `@player` scope and `@AutoSocket` sub-attrs.
    #[serde(rename = "Socket")]
    pub socket: Option<String>,

    /// Sound entries (Create, Death, Select, etc.). Has `@Type`, `@Squad`, `@Action` sub-attrs.
    #[serde(rename = "Sound")]
    pub sound: Option<String>,

    /// Squad mode animation overrides. Has `@Mode` sub-attr.
    #[serde(rename = "SquadModeAnim", default)]
    pub squad_mode_anim: Vec<String>,

    /// Starting velocity for projectiles.
    #[serde(rename = "StartingVelocity")]
    pub starting_velocity: Option<f32>,

    /// Stats name localized string ID (used in post-game stats).
    #[serde(rename = "StatsNameID")]
    pub stats_name_id: Option<i32>,

    /// Sub-select sort priority index.
    #[serde(rename = "SubSelectSort")]
    pub sub_select_sort: Option<i32>,

    /// Surface type enum (resolved via `BDatabase::getSurfaceType`).
    #[serde(rename = "SurfaceType")]
    pub surface_type: Option<String>,

    /// Sweet spot IK settings. (unsure)
    #[serde(rename = "SweetSpotIK")]
    pub sweet_spot_ik: Option<String>,

    /// Tactics file base name (resolves to `data\tactics\{name}.xmb`).
    #[serde(rename = "Tactics")]
    pub tactics: Option<String>,

    /// Target beam proto object ref (Scarab scenario hack).
    #[serde(rename = "TargetBeam")]
    pub target_beam: Option<String>,

    /// Terrain height tolerance for placement (default 10.0 in 2006 source).
    #[serde(rename = "TerrainHeightTolerance")]
    pub terrain_height_tolerance: Option<f32>,

    /// Tracking delay in seconds (engine multiplies by 1000 and stores as DWORD ms).
    #[serde(rename = "TrackingDelay")]
    pub tracking_delay: Option<f32>,

    /// Unused leftover from development — `TrackInterceptDistance` is read from gamedata, not objects.
    #[serde(rename = "TrackInterceptDistance")]
    pub track_intercept_distance: Option<f32>,

    /// Train animation type (resolved via `gVisualManager::getAnimType`).
    #[serde(rename = "TrainAnim")]
    pub train_anim: Option<String>,

    /// Trainer type (int). Has optional `@ApplyFormation` bool sub-attr.
    #[serde(rename = "TrainerType")]
    pub trainer_type: Option<i32>,

    /// Train limits per unit/squad type. Has `@Type`, `@Count`, `@Bucket` sub-attrs.
    #[serde(rename = "TrainLimit", default)]
    pub train_limit: Vec<String>,

    /// True LOS height — height at which LOS checks originate. (unsure)
    #[serde(rename = "TrueLOSHeight")]
    pub true_los_height: Option<f32>,

    /// Turn rate (degrees per second).
    #[serde(rename = "TurnRate")]
    pub turn_rate: Option<f32>,

    /// Unused leftover from development — not read by `BProtoObject::loadFromXml`.
    #[serde(rename = "UIVisual")]
    pub ui_visual: Option<String>,

    /// Desired velocity. Also auto-sets `MaxVelocity` and `Acceleration`.
    #[serde(rename = "Velocity")]
    pub velocity: Option<f32>,

    /// Veterancy level definitions with XP thresholds and stat bonuses.
    #[serde(rename = "Veterancy", default)]
    pub veterancy: Vec<VeterancyLevel>,

    /// Visual proto name (resolved via `gVisualManager::getOrCreateProtoVisual`).
    #[serde(rename = "Visual")]
    pub visual: Option<String>,

    /// Visual display priority (resolved via `BDatabase::getVisualDisplayPriority`).
    #[serde(rename = "VisualDisplayPriority")]
    pub visual_display_priority: Option<String>,
}

/// A hardpoint on a proto-object (turret mount point).
///
/// Loaded by `BHardpoint::load`. Each hardpoint has a yaw and pitch bone
/// attachment, rotation rates, angle limits, and optional sound cues.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Hardpoint {
    /// Hardpoint name (unique within the object).
    #[serde(rename = "@name", default)]
    pub name: String,

    /// Whether the turret auto-centers when idle (default `true`).
    #[serde(rename = "@autocenter")]
    pub autocenter: Option<bool>,

    /// Combined yaw+pitch on a single bone. Auto-set `true` when yaw and pitch
    /// attachments are the same bone (unless `useYawAndPitchAsTolerance` is set).
    #[serde(rename = "@combined")]
    pub combined: Option<bool>,

    /// Use hard (clamped) pitch limits instead of soft limits.
    #[serde(rename = "@hardpitchlimits")]
    pub hard_pitch_limits: Option<bool>,

    /// Skip rotation rate limiting when the hardpoint has an active target.
    #[serde(rename = "@infiniteRateWhenHasTarget")]
    pub infinite_rate_when_has_target: Option<bool>,

    /// Bone attachment name for pitch rotation (resolved via `gVisualManager::getAttachmentType`).
    #[serde(rename = "@pitchattachment")]
    pub pitch_attachment: Option<String>,

    /// Maximum pitch angle (radians). `pitchMinAngle` defaults to `-pitchMaxAngle`.
    #[serde(rename = "@pitchMaxAngle")]
    pub pitch_max_angle: Option<f32>,

    /// Minimum pitch angle (radians). Defaults to `-pitchMaxAngle` if not set.
    #[serde(rename = "@pitchMinAngle")]
    pub pitch_min_angle: Option<f32>,

    /// Pitch rotation rate (radians/sec, default `cPiOver12`).
    #[serde(rename = "@pitchrate")]
    pub pitch_rate: Option<f32>,

    /// Turret rotation is relative to the unit's facing direction.
    #[serde(rename = "@relativeToUnit")]
    pub relative_to_unit: Option<bool>,

    /// Use single-bone IK for this hardpoint.
    #[serde(rename = "@singleboneik")]
    pub single_bone_ik: Option<bool>,

    /// Sound cue played when pitch rotation starts.
    #[serde(rename = "@StartPitchSound")]
    pub start_pitch_sound: Option<String>,

    /// Sound cue played when yaw rotation starts.
    #[serde(rename = "@StartYawSound")]
    pub start_yaw_sound: Option<String>,

    /// Sound cue played when pitch rotation stops.
    #[serde(rename = "@StopPitchSound")]
    pub stop_pitch_sound: Option<String>,

    /// Sound cue played when yaw rotation stops.
    #[serde(rename = "@StopYawSound")]
    pub stop_yaw_sound: Option<String>,

    /// Treat yaw/pitch max angles as tolerance values instead of hard limits.
    #[serde(rename = "@useYawAndPitchAsTolerance")]
    pub use_yaw_and_pitch_as_tolerance: Option<bool>,

    /// Bone attachment name for yaw rotation (resolved via `gVisualManager::getAttachmentType`).
    #[serde(rename = "@yawattachment")]
    pub yaw_attachment: Option<String>,

    /// Symmetric yaw limit (radians). Sets both left and right max angles to `±value`.
    #[serde(rename = "@yawMaxAngle")]
    pub yaw_max_angle: Option<f32>,

    /// Left yaw limit (radians, default `-π`). Overrides the left side of `yawMaxAngle`.
    #[serde(rename = "@YawLeftMaxAngle")]
    pub yaw_left_max_angle: Option<f32>,

    /// Right yaw limit (radians, default `π`). Overrides the right side of `yawMaxAngle`.
    #[serde(rename = "@YawRightMaxAngle")]
    pub yaw_right_max_angle: Option<f32>,

    /// Yaw rotation rate (radians/sec, default `cPiOver12`).
    #[serde(rename = "@yawrate")]
    pub yaw_rate: Option<f32>,
}

/// A veterancy (experience) level entry (`BProtoObjectLevel`).
///
/// Each level defines an XP threshold and stat multipliers stored as half-floats
/// in the engine. Only levels with `Level > 0` are loaded.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct VeterancyLevel {
    /// Veterancy level number (must be > 0 to be loaded).
    #[serde(rename = "@Level", default)]
    pub level: i32,

    /// XP threshold required to reach this level.
    #[serde(rename = "@XP")]
    pub xp: Option<f32>,

    /// Damage output multiplier at this level.
    #[serde(rename = "@Damage")]
    pub damage: Option<f32>,

    /// Damage taken multiplier at this level (< 1.0 = more resistant).
    #[serde(rename = "@DamageTaken")]
    pub damage_taken: Option<f32>,

    /// Movement velocity multiplier at this level.
    #[serde(rename = "@Velocity")]
    pub velocity: Option<f32>,

    /// Accuracy multiplier at this level.
    #[serde(rename = "@Accuracy")]
    pub accuracy: Option<f32>,

    /// Work rate multiplier at this level (gathering, building, etc.).
    #[serde(rename = "@WorkRate")]
    pub work_rate: Option<f32>,

    /// Weapon range multiplier at this level.
    #[serde(rename = "@WeaponRange")]
    pub weapon_range: Option<f32>,
}

/// Parse all proto-objects from an `objects.xml.xmb` document.
pub fn parse(doc: &xmb::Document) -> crate::Result<Vec<ProtoObject>> {
    let root = expect_root(doc, "Objects")?;
    let objects: Vec<ProtoObject> = root
        .children
        .iter()
        .filter(|c| c.name == "Object")
        .map(bdt_serde::from_node)
        .collect::<Result<_, _>>()?;
    Ok(objects)
}
