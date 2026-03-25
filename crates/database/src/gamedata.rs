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
    #[serde(rename = "@name", default)]
    pub name: String,
    #[serde(rename = "$text", default)]
    pub value: String,
}

/// Parse game data from a `gamedata.xml.xmb` document.
pub fn parse(doc: &xmb::Document) -> crate::Result<GameData> {
    let root = expect_root(doc, "GameData")?;
    let gd: GameData = bdt_serde::from_node(root)?;
    Ok(gd)
}
