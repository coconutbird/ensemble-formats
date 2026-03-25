//! Parser for `gamedata.xml.xmb` — global game constants and settings.
//!
//! Contains resource definitions, population types, difficulty settings,
//! code proto-object mappings, and various gameplay constants.

use alloc::string::String;
use alloc::vec::Vec;

use crate::node_ext::{NodeExt, expect_root};

/// Global game data from `gamedata.xml`.
#[derive(Debug, Clone, Default)]
pub struct GameData {
    /// Resource definitions (Supplies, Power, etc.).
    pub resources: Vec<ResourceDef>,
    /// Rate definitions.
    pub rates: Vec<String>,
    /// Population type names.
    pub pops: Vec<String>,
    /// Ref count names.
    pub ref_counts: Vec<String>,
    /// HUD item names.
    pub hud_items: Vec<String>,
    /// Flashable item names.
    pub flashable_items: Vec<String>,
    /// Unit flag names.
    pub unit_flags: Vec<String>,
    /// Squad flag names.
    pub squad_flags: Vec<String>,
    /// Player state names.
    pub player_states: Vec<String>,
    /// Code proto-object mappings (type → proto name).
    pub code_proto_objects: Vec<CodeProtoObject>,
    /// Code object type mappings.
    pub code_object_types: Vec<CodeObjectType>,

    // Difficulty settings
    pub difficulty_easy: Option<f32>,
    pub difficulty_normal: Option<f32>,
    pub difficulty_hard: Option<f32>,
    pub difficulty_legendary: Option<f32>,
    pub difficulty_default: Option<f32>,

    // Supply pad settings
    pub unsc_supply_pad_bonus: Option<f32>,
    pub unsc_supply_pad_break_even_point: Option<f32>,
    pub cov_supply_pad_bonus: Option<f32>,
    pub cov_supply_pad_break_even_point: Option<f32>,

    // Transport settings
    pub transport_max: Option<i32>,

    // Cryo/freeze settings
    pub time_frozen_to_thaw: Option<f32>,
    pub time_freezing_to_thaw: Option<f32>,
    pub default_cryo_points: Option<f32>,
    pub default_thaw_speed: Option<f32>,
    pub freezing_speed_modifier: Option<f32>,
    pub freezing_damage_modifier: Option<f32>,
    pub frozen_damage_modifier: Option<f32>,
}

/// A resource definition.
#[derive(Debug, Clone, Default)]
pub struct ResourceDef {
    pub name: String,
    pub deductable: Option<bool>,
}

/// A code proto-object mapping.
#[derive(Debug, Clone, Default)]
pub struct CodeProtoObject {
    pub object_type: String,
    pub proto_name: String,
}

/// A code object type mapping.
#[derive(Debug, Clone, Default)]
pub struct CodeObjectType {
    pub name: String,
    pub value: String,
}

/// Parse game data from a `gamedata.xml.xmb` document.
pub fn parse(doc: &xmb::Document) -> crate::Result<GameData> {
    let root = expect_root(doc, "GameData")?;
    let mut gd = GameData::default();

    // Resources
    if let Some(res) = root.child("Resources") {
        for r in res.children_named("Resource") {
            gd.resources.push(ResourceDef {
                name: r.text_string(),
                deductable: r.attr_bool("Deductable"),
            });
        }
    }

    // Simple list sections
    parse_string_list(root, "Rates", "Rate", &mut gd.rates);
    parse_string_list(root, "Pops", "Pop", &mut gd.pops);
    parse_string_list(root, "RefCounts", "RefCount", &mut gd.ref_counts);
    parse_string_list(root, "HUDItems", "HUDItem", &mut gd.hud_items);
    parse_string_list(root, "FlashableItems", "Item", &mut gd.flashable_items);
    parse_string_list(root, "UnitFlags", "UnitFlag", &mut gd.unit_flags);
    parse_string_list(root, "SquadFlags", "SquadFlag", &mut gd.squad_flags);
    parse_string_list(root, "PlayerStates", "PlayerState", &mut gd.player_states);

    // Code proto objects
    if let Some(cpo) = root.child("CodeProtoObjects") {
        for c in cpo.children_named("CodeProtoObject") {
            gd.code_proto_objects.push(CodeProtoObject {
                object_type: c.attr_str("Type").unwrap_or_default(),
                proto_name: c.text_string(),
            });
        }
    }

    // Code object types
    if let Some(cot) = root.child("CodeObjectTypes") {
        for c in cot.children_named("CodeObjectType") {
            gd.code_object_types.push(CodeObjectType {
                name: c.attr_str("name").unwrap_or_default(),
                value: c.text_string(),
            });
        }
    }

    // Scalar values
    gd.difficulty_easy = root.child_f32("DifficultyEasy");
    gd.difficulty_normal = root.child_f32("DifficultyNormal");
    gd.difficulty_hard = root.child_f32("DifficultyHard");
    gd.difficulty_legendary = root.child_f32("DifficultyLegendary");
    gd.difficulty_default = root.child_f32("DifficultyDefault");
    gd.unsc_supply_pad_bonus = root.child_f32("UnscSupplyPadBonus");
    gd.unsc_supply_pad_break_even_point = root.child_f32("UnscSupplyPadBreakEvenPoint");
    gd.cov_supply_pad_bonus = root.child_f32("CovSupplyPadBonus");
    gd.cov_supply_pad_break_even_point = root.child_f32("CovSupplyPadBreakEvenPoint");
    gd.transport_max = root.child_i32("TransportMax");
    gd.time_frozen_to_thaw = root.child_f32("TimeFrozenToThaw");
    gd.time_freezing_to_thaw = root.child_f32("TimeFreezingToThaw");
    gd.default_cryo_points = root.child_f32("DefaultCryoPoints");
    gd.default_thaw_speed = root.child_f32("DefaultThawSpeed");
    gd.freezing_speed_modifier = root.child_f32("FreezingSpeedModifier");
    gd.freezing_damage_modifier = root.child_f32("FreezingDamageModifier");
    gd.frozen_damage_modifier = root.child_f32("FrozenDamageModifier");

    Ok(gd)
}

fn parse_string_list(root: &bdt::Node, parent: &str, child: &str, out: &mut Vec<String>) {
    if let Some(section) = root.child(parent) {
        for item in section.children_named(child) {
            out.push(item.text_string());
        }
    }
}
