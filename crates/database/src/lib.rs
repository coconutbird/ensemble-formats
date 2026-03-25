//! HW1 game database parser.
//!
//! Provides typed access to game data stored in XMB files:
//! - `objects.xml.xmb` — proto objects (units, buildings, projectiles)
//! - `squads.xml.xmb` — squad definitions
//! - `techs.xml.xmb` — tech tree
//! - `abilities.xml.xmb` — ability definitions
//! - `powers.xml.xmb` — leader power definitions
//! - `civs.xml.xmb` — civilization definitions
//! - `leaders.xml.xmb` — leader definitions
//! - `weapontypes.xml.xmb` — weapon type damage tables
//! - `damagetypes.xml.xmb` — damage type definitions
//! - `gamedata.xml.xmb` — global game constants
//! - `*.vis.xmb` — visual definitions (models, animations, attachments)
//! - `*.tactics.xmb` — combat tactics (weapons, actions)
//! - `*.physics.xmb` / `*.blueprint.xmb` / `*.shp.xmb` — physics data

#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

pub mod abilities;
pub mod civs;
pub mod damagetypes;
pub mod gamedata;
pub mod leaders;
pub mod node_ext;
pub mod objects;
pub mod physics;
pub mod powers;
pub mod squads;
pub mod tactics;
pub mod techs;
pub mod visual;
pub mod weapontypes;

use alloc::string::String;
use alloc::vec::Vec;

pub use abilities::Ability;
pub use civs::Civ;
pub use damagetypes::DamageType;
pub use gamedata::GameData;
pub use leaders::Leader;
pub use objects::ProtoObject;
pub use physics::{Blueprint, Physics, Shape};
pub use powers::Power;
pub use squads::Squad;
pub use tactics::TacticData;
pub use techs::Tech;
pub use visual::Visual;
pub use weapontypes::WeaponType;

/// Errors that can occur when parsing database files.
#[derive(Debug)]
#[cfg_attr(feature = "std", derive(thiserror::Error))]
pub enum Error {
    #[cfg_attr(feature = "std", error("xmb error: {0}"))]
    Xmb(#[cfg_attr(feature = "std", from)] xmb::Error),

    #[cfg_attr(feature = "std", error("deserialize error: {0}"))]
    Deserialize(#[cfg_attr(feature = "std", from)] bdt_serde::Error),

    #[cfg_attr(feature = "std", error("missing root element"))]
    MissingRoot,

    #[cfg_attr(
        feature = "std",
        error("unexpected root element: expected '{expected}', got '{actual}'")
    )]
    UnexpectedRoot { expected: String, actual: String },
}

pub type Result<T> = core::result::Result<T, Error>;

/// A complete HW1 game database, loaded from XMB documents.
#[derive(Debug, Default)]
pub struct Database {
    pub objects: Vec<ProtoObject>,
    pub squads: Vec<Squad>,
    pub techs: Vec<Tech>,
    pub abilities: Vec<Ability>,
    pub powers: Vec<Power>,
    pub civs: Vec<Civ>,
    pub leaders: Vec<Leader>,
    pub weapon_types: Vec<WeaponType>,
    pub damage_types: Vec<DamageType>,
    pub game_data: Option<GameData>,
}

impl Database {
    /// Create an empty database.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load proto objects from an `objects.xml.xmb` document.
    pub fn load_objects(&mut self, doc: &xmb::Document) -> Result<usize> {
        let objs = objects::parse(doc)?;
        let count = objs.len();
        self.objects = objs;
        Ok(count)
    }

    /// Load squads from a `squads.xml.xmb` document.
    pub fn load_squads(&mut self, doc: &xmb::Document) -> Result<usize> {
        let squads = squads::parse(doc)?;
        let count = squads.len();
        self.squads = squads;
        Ok(count)
    }

    /// Load techs from a `techs.xml.xmb` document.
    pub fn load_techs(&mut self, doc: &xmb::Document) -> Result<usize> {
        let techs = techs::parse(doc)?;
        let count = techs.len();
        self.techs = techs;
        Ok(count)
    }

    /// Load abilities from an `abilities.xml.xmb` document.
    pub fn load_abilities(&mut self, doc: &xmb::Document) -> Result<usize> {
        let abs = abilities::parse(doc)?;
        let count = abs.len();
        self.abilities = abs;
        Ok(count)
    }

    /// Load powers from a `powers.xml.xmb` document.
    pub fn load_powers(&mut self, doc: &xmb::Document) -> Result<usize> {
        let pows = powers::parse(doc)?;
        let count = pows.len();
        self.powers = pows;
        Ok(count)
    }

    /// Load civilizations from a `civs.xml.xmb` document.
    pub fn load_civs(&mut self, doc: &xmb::Document) -> Result<usize> {
        let civs = civs::parse(doc)?;
        let count = civs.len();
        self.civs = civs;
        Ok(count)
    }

    /// Load leaders from a `leaders.xml.xmb` document.
    pub fn load_leaders(&mut self, doc: &xmb::Document) -> Result<usize> {
        let leaders = leaders::parse(doc)?;
        let count = leaders.len();
        self.leaders = leaders;
        Ok(count)
    }

    /// Load weapon types from a `weapontypes.xml.xmb` document.
    pub fn load_weapon_types(&mut self, doc: &xmb::Document) -> Result<usize> {
        let wts = weapontypes::parse(doc)?;
        let count = wts.len();
        self.weapon_types = wts;
        Ok(count)
    }

    /// Load damage types from a `damagetypes.xml.xmb` document.
    pub fn load_damage_types(&mut self, doc: &xmb::Document) -> Result<usize> {
        let dts = damagetypes::parse(doc)?;
        let count = dts.len();
        self.damage_types = dts;
        Ok(count)
    }

    /// Load game data from a `gamedata.xml.xmb` document.
    pub fn load_game_data(&mut self, doc: &xmb::Document) -> Result<()> {
        self.game_data = Some(gamedata::parse(doc)?);
        Ok(())
    }
}
