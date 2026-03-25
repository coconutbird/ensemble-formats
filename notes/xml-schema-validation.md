# XML Schema Validation via Engine Cross-Reference

## The Problem

The XML/XMB data files are not the source of truth for what fields exist.
The engine's `loadFromXml` functions are. There are three categories:

### 1. Engine reads it, XML has it → **Valid field** (add to Rust struct)

The field string exists in the binary's `loadFromXml` and appears in game data.
This is the happy path — add it to our struct with the correct type.

### 2. XML has it, engine ignores it → **Dead data** (skip or warn)

The field appears in the XML but NO `stricmp` in `loadFromXml` checks for it.
The game ships data the engine never reads. Could be:

- Legacy fields from development that were never cleaned up
- Fields intended for a different loader (e.g. scenario editor vs runtime)
- Copy-paste artifacts in the data files

### 3. Engine reads it, XML doesn't have it → **Default-only field** (optional in struct)

The `loadFromXml` has a `stricmp` for this field name, but no XML entry uses it.
The engine supports it but it always falls through to its default value.

## Workflow

### Step 1: Get the extra field list

Run the integration test to see all unverified fields and their counts:

```
cargo test -p database-cli --test validate_hw1 -- debug_objects_i32_failure --nocapture
```

This prints every unique "extra field" warning with occurrence count, e.g.:

```
Acceleration                             79x
AmmoMax                                  52x
CombatValue                              175x
```

### Step 2: Search for the field string in IDA

```
find_regex("^FieldName$")
```

- **Match found** → proceed to step 3.
- **No match** → the string doesn't exist in the binary at all. **Dead data.**
  Document in `notes/schema/<file>.md` under "Dead Data".

### Step 3: Check xrefs into the loader function

```
xrefs_to("<string address>")
```

Look for a reference inside the relevant `loadFromXml` function.

- **Xref in loadFromXml** → **real field**. Proceed to step 4.
- **Xref exists but NOT in loadFromXml** → used elsewhere (e.g. save, editor).
  Still likely dead for runtime loading. Investigate further or mark as dead.

### Step 4: Read the disassembly to get type and offset

Look at the instructions after the `stricmp` match:

```asm
mov     rdx, [r14]           ; dereference this ptr
add     rdx, XXh             ; struct offset → document this
lea     rcx, [rsp+...]       ; xml reader context
call    BXMLReader__getTextAsFloat   ; → f32
```

Type mapping:

- `BXMLReader__getTextAsFloat` → `f32`
- `BXMLReader__getTextAsInt` → `i32`
- `BXMBData__getValueAsFloat` + `mulss` + `cvttss2si` → `Fract24` (use `f32`)
- String copy / `getTextAsString` → `String`
- Nested `stricmp` loop → child element, may need a sub-struct or `Vec<String>`

### Step 5: Comment in IDA

```
set_comments(addr, "field: FieldName (f32) @ *this+0xXX")
```

Keeps the IDA database annotated for future reference.

### Step 6: Add to Rust struct

Add the field to the struct in `crates/database/src/<file>.rs`:

```rust
/// Description from IDA analysis.
#[serde(rename = "FieldName")]
pub field_name: Option<f32>,
```

### Step 7: Run tests and verify

```
cargo test -p database-cli --test validate_hw1 -- validate_base_game --nocapture
```

Warning count should drop. Zero failures expected.

### Step 8: Update schema notes

Move the field into the "Verified Fields" table in `notes/schema/<file>.md`.

## Batching

Steps 2-3 can be batched — search and xref-check many fields at once,
then add them all to the struct in one edit. A typical batch:

1. `find_regex` for 10-20 field names in parallel
2. `xrefs_to` for all found string addresses in parallel
3. `disasm` around each xref to read type/offset
4. One `str-replace-editor` call to add all fields to the struct
5. One test run to verify

## Known Loader Functions

| File          | Loader Function           | Address       |
| ------------- | ------------------------- | ------------- |
| objects.xml   | BProtoObject::loadFromXml | `0x140341620` |
| squads.xml    | BProtoSquad::loadFromXml  | TBD           |
| techs.xml     | BProtoTech::loadFromXml   | TBD           |
| powers.xml    | BPower::parseFromXml      | `0x14034cb00` |
| abilities.xml | BAbility::parseFromXml    | `0x1400f53c0` |
| civs.xml      | BCiv::parseFromXml        | `0x140193000` |
| leaders.xml   | BLeader::loadFromXml      | TBD           |
| gamedata.xml  | —                         | TBD           |

## Per-File Schema Notes

Detailed findings for each file live in `notes/schema/`:

- `notes/schema/objects.md` — verified fields, dead data, pending fields
- `notes/schema/squads.md`
- `notes/schema/techs.md`
- `notes/schema/abilities.md`
- `notes/schema/powers.md`
- `notes/schema/civs.md`
- `notes/schema/leaders.md`
- `notes/schema/gamedata.md`

## Progress

| File            | Warnings            | Verified  | Dead Data | Remaining   |
| --------------- | ------------------- | --------- | --------- | ----------- |
| objects.xml     | 11,443 → was 12,991 | 32 fields | 1 (LOS)   | ~108 unique |
| squads.xml      | 813                 | 0         | 0         | TBD         |
| techs.xml       | 2,395               | 0         | 0         | TBD         |
| abilities.xml   | 12                  | 0         | 0         | TBD         |
| powers.xml      | 114                 | 0         | 0         | TBD         |
| civs.xml        | 0                   | —         | —         | Done        |
| leaders.xml     | 5                   | 0         | 0         | TBD         |
| weapontypes.xml | 0                   | —         | —         | Done        |
| damagetypes.xml | 0                   | —         | —         | Done        |
| gamedata.xml    | 129                 | 0         | 0         | TBD         |

## Why This Matters

If we blindly add every XML field to our Rust structs, we might map fields
the engine never reads — meaning we'd be preserving dead data that has no
gameplay effect. The engine binary is the canonical schema.

Conversely, if a field IS in the engine but not in any XML, we should still
have it in our struct as `Option<T>` so round-tripping works if someone
adds it to their mod data.
