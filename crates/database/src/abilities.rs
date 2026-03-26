//! Parser for `abilities.xml.xmb` — ability definitions.
//!
//! Each `<Ability>` element describes a unit ability (lockdown, ram, barrage, etc.).

use alloc::string::String;
use alloc::vec::Vec;
use serde::Deserialize;

use crate::node_ext::expect_root;

/// A single ability definition from `abilities.xml`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Ability {
    /// Ability name (unique key), e.g. `"UnscLockdown"`.
    #[serde(rename = "@Name", default)]
    pub name: String,
    /// Display name string ID.
    #[serde(rename = "DisplayNameID")]
    pub display_name_id: Option<i32>,
    /// Secondary display name string ID.
    #[serde(rename = "DisplayName2ID")]
    pub display_name_2_id: Option<i32>,
    /// Rollover text string ID.
    #[serde(rename = "RolloverTextID")]
    pub rollover_text_id: Option<i32>,
    /// Ability type: `"Work"`, `"ChangeMode"`, `"Unload"`, `"CommandMenu"`, etc.
    #[serde(rename = "Type")]
    pub ability_type: Option<String>,
    /// Squad mode to enter, e.g. `"Lockdown"`, `"HitAndRun"`.
    #[serde(rename = "SquadMode")]
    pub squad_mode: Option<String>,
    /// Whether to keep the squad mode after ability ends.
    #[serde(rename = "KeepSquadMode")]
    pub keep_squad_mode: Option<bool>,
    /// Target type: `"Unit"`, `"Location"`, `"UnitOrLocation"`.
    #[serde(rename = "TargetType")]
    pub target_type: Option<String>,
    /// Recovery start trigger: `"Attack"`.
    #[serde(rename = "RecoverStart")]
    pub recover_start: Option<String>,
    /// Recovery type: `"Ability"`.
    #[serde(rename = "RecoverType")]
    pub recover_type: Option<String>,
    /// Recovery time in seconds.
    #[serde(rename = "RecoverTime")]
    pub recover_time: Option<f32>,
    /// Duration in seconds.
    #[serde(rename = "Duration")]
    pub duration: Option<f32>,
    /// Movement speed modifier while active.
    #[serde(rename = "MovementSpeedModifier")]
    pub movement_speed_modifier: Option<f32>,
    /// Movement modifier type: `"Mode"`.
    #[serde(rename = "MovementModifierType")]
    pub movement_modifier_type: Option<String>,
    /// Whether this ability can be used in hetero-command groups.
    #[serde(rename = "CanHeteroCommand")]
    pub can_hetero_command: Option<bool>,
    /// Whether to suppress the ability reticle.
    #[serde(rename = "NoAbilityReticle")]
    pub no_ability_reticle: Option<bool>,
    /// Whether to avoid interrupting the current attack.
    #[serde(rename = "DontInterruptAttack")]
    pub dont_interrupt_attack: Option<bool>,
    /// Sprinting modifier.
    #[serde(rename = "SprintingModifier")]
    pub sprinting_modifier: Option<f32>,
    /// Icon path.
    #[serde(rename = "Icon")]
    pub icon: Option<String>,
    /// Damage taken modifier while ability is active.
    #[serde(rename = "DamageTakenModifier")]
    pub damage_taken_modifier: Option<f32>,
    /// Recovery animation attachment point.
    #[serde(rename = "RecoverAnimAttachment")]
    pub recover_anim_attachment: Option<String>,
    /// Recovery end animation.
    #[serde(rename = "RecoverEndAnim")]
    pub recover_end_anim: Option<String>,
    /// Recovery start animation.
    #[serde(rename = "RecoverStartAnim")]
    pub recover_start_anim: Option<String>,
    /// Smart target range.
    #[serde(rename = "SmartTargetRange")]
    pub smart_target_range: Option<f32>,
}

/// Parse all abilities from an `abilities.xml.xmb` document.
pub fn parse(doc: &xmb::Document) -> crate::Result<Vec<Ability>> {
    let root = expect_root(doc, "Abilities")?;
    let abilities: Vec<Ability> = root
        .children
        .iter()
        .filter(|c| c.name == "Ability")
        .map(bdt_serde::from_node)
        .collect::<Result<_, _>>()?;
    Ok(abilities)
}
