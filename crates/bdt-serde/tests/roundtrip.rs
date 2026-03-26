//! Round-trip integration tests: serialize via `to_node`, deserialize via `from_node`.

use bdt::variant::Variant;
use bdt_serde::{from_node, to_node};
use serde::{Deserialize, Serialize};

/// Round-trip helper: serialize → Node → deserialize.
fn roundtrip<T>(name: &str, value: &T) -> T
where
    T: Serialize + for<'de> Deserialize<'de> + core::fmt::Debug,
{
    let node = to_node(name, value).expect("to_node failed");
    from_node::<T>(&node).expect("from_node failed")
}

// ---------------------------------------------------------------------------
// $text + @attr
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct DamageType {
    #[serde(rename = "$text")]
    name: String,
    #[serde(rename = "@AttackRating")]
    attack_rating: Option<bool>,
}

#[test]
fn text_and_attr() {
    let dt = DamageType {
        name: "Melee".into(),
        attack_rating: Some(true),
    };
    let node = to_node("DamageType", &dt).unwrap();

    assert_eq!(node.name, "DamageType");
    assert_eq!(node.text, Variant::String("Melee".into()));
    assert_eq!(node.attributes.len(), 1);
    assert_eq!(node.attributes[0].name, "AttackRating");
    assert_eq!(node.attributes[0].value, Variant::Bool(true));
}

#[test]
fn roundtrip_text_and_attr() {
    let dt = DamageType {
        name: "Melee".into(),
        attack_rating: Some(true),
    };
    assert_eq!(roundtrip("DamageType", &dt), dt);
}

#[test]
fn none_attr_omitted() {
    let dt = DamageType {
        name: "Ranged".into(),
        attack_rating: None,
    };
    let node = to_node("DamageType", &dt).unwrap();
    assert_eq!(node.attributes.len(), 0);
}

#[test]
fn roundtrip_none_attr() {
    let dt = DamageType {
        name: "Ranged".into(),
        attack_rating: None,
    };
    assert_eq!(roundtrip("DamageType", &dt), dt);
}

// ---------------------------------------------------------------------------
// Nested struct → child element
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Weapon {
    #[serde(rename = "@Name")]
    name: String,
    #[serde(rename = "DamageType")]
    damage_type: DamageType,
}

#[test]
fn child_struct() {
    let w = Weapon {
        name: "Sword".into(),
        damage_type: DamageType {
            name: "Melee".into(),
            attack_rating: Some(false),
        },
    };
    let node = to_node("Weapon", &w).unwrap();

    assert_eq!(node.name, "Weapon");
    assert_eq!(node.attributes.len(), 1);
    assert_eq!(node.attributes[0].name, "Name");
    assert_eq!(node.children.len(), 1);
    assert_eq!(node.children[0].name, "DamageType");
    assert_eq!(node.children[0].text, Variant::String("Melee".into()));
}

#[test]
fn roundtrip_child_struct() {
    let w = Weapon {
        name: "Sword".into(),
        damage_type: DamageType {
            name: "Melee".into(),
            attack_rating: Some(false),
        },
    };
    assert_eq!(roundtrip("Weapon", &w), w);
}

// ---------------------------------------------------------------------------
// Vec<T> → repeated same-name children
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Squad {
    #[serde(rename = "@Name")]
    name: String,
    #[serde(rename = "Weapon", default)]
    weapons: Vec<Weapon>,
}

#[test]
fn vec_children() {
    let s = Squad {
        name: "Infantry".into(),
        weapons: vec![
            Weapon {
                name: "Sword".into(),
                damage_type: DamageType {
                    name: "Melee".into(),
                    attack_rating: None,
                },
            },
            Weapon {
                name: "Bow".into(),
                damage_type: DamageType {
                    name: "Ranged".into(),
                    attack_rating: Some(true),
                },
            },
        ],
    };
    let node = to_node("Squad", &s).unwrap();

    assert_eq!(node.children.len(), 2);
    assert_eq!(node.children[0].name, "Weapon");
    assert_eq!(node.children[1].name, "Weapon");
    assert_eq!(
        node.children[0].attributes[0].value,
        Variant::String("Sword".into())
    );
    assert_eq!(
        node.children[1].attributes[0].value,
        Variant::String("Bow".into())
    );
}

#[test]
fn roundtrip_vec_children() {
    let s = Squad {
        name: "Infantry".into(),
        weapons: vec![
            Weapon {
                name: "Sword".into(),
                damage_type: DamageType {
                    name: "Melee".into(),
                    attack_rating: None,
                },
            },
            Weapon {
                name: "Bow".into(),
                damage_type: DamageType {
                    name: "Ranged".into(),
                    attack_rating: Some(true),
                },
            },
        ],
    };
    assert_eq!(roundtrip("Squad", &s), s);
}

#[test]
fn empty_vec() {
    let s = Squad {
        name: "Empty".into(),
        weapons: vec![],
    };
    let node = to_node("Squad", &s).unwrap();
    assert_eq!(node.children.len(), 0);
}

// ---------------------------------------------------------------------------
// Numeric attribute types
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Stats {
    #[serde(rename = "@hp")]
    hp: i32,
    #[serde(rename = "@armor")]
    armor: u32,
    #[serde(rename = "@speed")]
    speed: f32,
}

#[test]
fn numeric_attrs() {
    let s = Stats {
        hp: -10,
        armor: 42,
        speed: 3.5,
    };
    let node = to_node("Stats", &s).unwrap();

    assert_eq!(node.attributes.len(), 3);
    assert_eq!(node.attributes[0].value, Variant::Int(-10));
    assert_eq!(node.attributes[1].value, Variant::UInt(42));
    assert_eq!(node.attributes[2].value, Variant::Float(3.5));
}

#[test]
fn roundtrip_numeric() {
    let s = Stats {
        hp: -10,
        armor: 42,
        speed: 3.5,
    };
    assert_eq!(roundtrip("Stats", &s), s);
}

// ---------------------------------------------------------------------------
// String child → leaf node
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Config {
    #[serde(rename = "Label")]
    label: String,
}

#[test]
fn string_child() {
    let c = Config {
        label: "hello".into(),
    };
    let node = to_node("Config", &c).unwrap();

    assert_eq!(node.children.len(), 1);
    assert_eq!(node.children[0].name, "Label");
    assert_eq!(node.children[0].text, Variant::String("hello".into()));
}

#[test]
fn roundtrip_string_child() {
    let c = Config {
        label: "hello".into(),
    };
    assert_eq!(roundtrip("Config", &c), c);
}
