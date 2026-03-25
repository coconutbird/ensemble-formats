//! Parser for `abilities.xml.xmb` — ability definitions.
//!
//! Each `<Ability>` element describes a unit ability (lockdown, ram, barrage, etc.).

use alloc::string::String;
use alloc::vec::Vec;

use crate::node_ext::{NodeExt, expect_root};

/// A single ability definition from `abilities.xml`.
#[derive(Debug, Clone, Default)]
pub struct Ability {
    /// Ability name (unique key), e.g. `"UnscLockdown"`.
    pub name: String,
    /// Display name string ID.
    pub display_name_id: Option<i32>,
    /// Secondary display name string ID.
    pub display_name_2_id: Option<i32>,
    /// Rollover text string ID.
    pub rollover_text_id: Option<i32>,
    /// Ability type: `"Work"`, `"ChangeMode"`, `"Unload"`, `"CommandMenu"`, etc.
    pub ability_type: Option<String>,
    /// Squad mode to enter, e.g. `"Lockdown"`, `"HitAndRun"`.
    pub squad_mode: Option<String>,
    /// Whether to keep the squad mode after ability ends.
    pub keep_squad_mode: Option<bool>,
    /// Target type: `"Unit"`, `"Location"`, `"UnitOrLocation"`.
    pub target_type: Option<String>,
    /// Recovery start trigger: `"Attack"`.
    pub recover_start: Option<String>,
    /// Recovery type: `"Ability"`.
    pub recover_type: Option<String>,
    /// Recovery time in seconds.
    pub recover_time: Option<f32>,
    /// Duration in seconds.
    pub duration: Option<f32>,
    /// Movement speed modifier while active.
    pub movement_speed_modifier: Option<f32>,
    /// Movement modifier type: `"Mode"`.
    pub movement_modifier_type: Option<String>,
    /// Whether this ability can be used in hetero-command groups.
    pub can_hetero_command: Option<bool>,
    /// Whether to suppress the ability reticle.
    pub no_ability_reticle: Option<bool>,
    /// Whether to avoid interrupting the current attack.
    pub dont_interrupt_attack: Option<bool>,
    /// Sprinting modifier.
    pub sprinting_modifier: Option<f32>,
    /// Icon path.
    pub icon: Option<String>,
}

/// Parse all abilities from an `abilities.xml.xmb` document.
pub fn parse(doc: &xmb::Document) -> crate::Result<Vec<Ability>> {
    let root = expect_root(doc, "Abilities")?;
    let mut abilities = Vec::new();

    for node in root.children_named("Ability") {
        abilities.push(parse_ability(node));
    }

    Ok(abilities)
}

fn parse_ability(node: &bdt::Node) -> Ability {
    Ability {
        name: node.attr_str("Name").unwrap_or_default(),
        display_name_id: node.child_i32("DisplayNameID"),
        display_name_2_id: node.child_i32("DisplayName2ID"),
        rollover_text_id: node.child_i32("RolloverTextID"),
        ability_type: node.child_text("Type"),
        squad_mode: node.child_text("SquadMode"),
        keep_squad_mode: node.child_bool("KeepSquadMode"),
        target_type: node.child_text("TargetType"),
        recover_start: node.child_text("RecoverStart"),
        recover_type: node.child_text("RecoverType"),
        recover_time: node.child_f32("RecoverTime"),
        duration: node.child_f32("Duration"),
        movement_speed_modifier: node.child_f32("MovementSpeedModifier"),
        movement_modifier_type: node.child_text("MovementModifierType"),
        can_hetero_command: node.child_bool("CanHeteroCommand"),
        no_ability_reticle: node.child_bool("NoAbilityReticle"),
        dont_interrupt_attack: node.child_bool("DontInterruptAttack"),
        sprinting_modifier: node.child_f32("SprintingModifier"),
        icon: node.child_text("Icon"),
    }
}
