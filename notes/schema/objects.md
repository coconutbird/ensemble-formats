# objects.xml.xmb — Schema Notes

Loader: `BProtoObject::loadFromXml` @ `0x140341620`

## Verified Fields (engine reads via `stricmp`)

| Rust Field                  | XML Name                 | Type     | Engine Offset | Accessor              |
| --------------------------- | ------------------------ | -------- | ------------- | --------------------- |
| `name`                      | `@name`                  | String   | attribute     | —                     |
| `id`                        | `@id`                    | i32      | attribute     | —                     |
| `dbid`                      | `@dbid`                  | i32      | attribute     | —                     |
| `object_class`              | `ObjectClass`            | String   | —             | stricmp @ 0x140341819 |
| `visual`                    | `Visual`                 | String   | —             | stricmp @ 0x140341879 |
| `physics_info`              | `PhysicsInfo`            | String   | —             | stricmp @ 0x1403422d1 |
| `physics_replacement_info`  | `PhysicsReplacementInfo` | String   | —             | stricmp @ 0x14034235a |
| `tactics`                   | `Tactics`                | String   | —             | stricmp @ 0x14034775e |
| `hitpoints`                 | `Hitpoints`              | f32      | —             | stricmp @ 0x1403427d7 |
| `movement_type`             | `MovementType`           | String   | —             | stricmp @ 0x140341c55 |
| `velocity`                  | `Velocity`               | f32      | —             | stricmp @ 0x1403423e3 |
| `turn_rate`                 | `TurnRate`               | f32      | —             | stricmp @ 0x1403427aa |
| `portrait_icon`             | `PortraitIcon`           | String   | —             | stricmp @ 0x1403463a1 |
| `display_name_id`           | `DisplayNameID`          | i32      | —             | stricmp @ 0x1403434c5 |
| `rollover_text_id`          | `RolloverTextID`         | i32      | —             | stricmp @ 0x14034357b |
| `bounty`                    | `Bounty`                 | f32      | —             | stricmp @ 0x140342ea4 |
| `flags`                     | `Flag`                   | String[] | —             | stricmp @ 0x1403418a9 |
| `object_types`              | `ObjectType`             | String[] | —             | stricmp @ 0x1411b2dd8 |
| `hardpoints`                | `Hardpoint`              | struct[] | —             | stricmp @ 0x140341ca0 |
| `veterancy`                 | `Veterancy`              | struct[] | —             | stricmp @ 0x140343c25 |
| `flatten_min_x0`            | `FlattenMinX0`           | f32      | \*this+0x20   | getTextAsFloat        |
| `flatten_max_x0`            | `FlattenMaxX0`           | f32      | \*this+0x24   | getTextAsFloat        |
| `flatten_min_z0`            | `FlattenMinZ0`           | f32      | \*this+0x28   | getTextAsFloat        |
| `flatten_max_z0`            | `FlattenMaxZ0`           | f32      | \*this+0x2C   | getTextAsFloat        |
| `flatten_min_x1`            | `FlattenMinX1`           | f32      | \*this+0x30   | getTextAsFloat        |
| `flatten_max_x1`            | `FlattenMaxX1`           | f32      | \*this+0x34   | getTextAsFloat        |
| `obstruction_radius_x`      | `ObstructionRadiusX`     | f32      | \*this+0x44   | getTextAsFloat        |
| `obstruction_radius_y`      | `ObstructionRadiusY`     | f32      | \*this+0x48   | getTextAsFloat        |
| `obstruction_radius_z`      | `ObstructionRadiusZ`     | f32      | \*this+0x4C   | getTextAsFloat        |
| `ammo_max`                  | `AmmoMax`                | f32      | this+0x88     | getTextAsFloat        |
| `ammo_regen_rate`           | `AmmoRegenRate`          | f32      | this+0x8C     | getTextAsFloat        |
| `num_conversions`           | `NumConversions`         | i32      | \*this+0x184  | getTextAsInt          |
| `num_stasis_fields_to_stop` | `NumStasisFieldsToStop`  | i32      | \*this+0x188  | getTextAsInt          |

## Dead Data (in XML, engine ignores)

Fields present in game XML data but no corresponding `stricmp` in `loadFromXml`.

| XML Name | Notes                                                                                                                                                                                 |
| -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `LOS`    | No string literal "LOS" found in binary. Engine has `LOSType`, `LOSObstructable`, debug printf `"LOS: %f"` — but no `stricmp("LOS")` in `loadFromXml`. Kept in struct for round-trip. |

## IDA-Confirmed But Not Yet In Struct

_All confirmed fields have been added to the struct._

## Unverified Extra Fields (108 unique, from diagnostic warnings)

These appear in the XML data but have not yet been checked in IDA.
See test output from `debug_objects_i32_failure` for full list with counts.
