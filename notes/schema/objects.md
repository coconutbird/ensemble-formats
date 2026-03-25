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
| `build_points`              | `BuildPoints`            | f32      | \*this+0x74   | getTextAsFloat        |
| `select_type`               | `SelectType`             | i32/enum | \*this+0x78   | enum lookup           |
| `goto_type`                 | `GotoType`               | i32/enum | \*this+0x7C   | enum lookup           |
| `selected_radius_x`         | `SelectedRadiusX`        | f32      | \*this+0x80   | getTextAsFloat        |
| `selected_radius_z`         | `SelectedRadiusZ`        | f32      | \*this+0x84   | getTextAsFloat        |
| `trainer_type`              | `TrainerType`            | i32      | \*this+0x8C   | BLocString__parseID   |
| `death_spawn_squad`         | `DeathSpawnSquad`        | i32      | r14+0xB0      | getProtoSquadID       |
| `combat_value`              | `CombatValue`            | f32      | \*this+0xC4   | getTextAsFloat        |
| `resource_amount`           | `ResourceAmount`         | f32      | \*this+0xC8   | getTextAsFloat        |
| `placement_rules`           | `PlacementRules`         | i32/enum | \*this+0xCC   | enum lookup           |
| `death_fade_time`           | `DeathFadeTime`          | f32      | \*this+0xD0   | getTextAsFloat        |
| `death_fade_delay_time`     | `DeathFadeDelayTime`     | f32      | \*this+0xD4   | getTextAsFloat        |
| `train_anim`                | `TrainAnim`              | i32      | \*this+0xD8   | anim lookup           |
| `rally_point`               | `RallyPoint`             | i32/enum | \*this+0x110  | stricmp enum          |
| `max_projectile_height`     | `MaxProjectileHeight`    | f32      | \*this+0x114  | getTextAsFloat        |
| `death_replacement`         | `DeathReplacement`       | i32      | \*this+0x118  | getProtoObjectID      |
| `surface_type`              | `SurfaceType`            | i8/enum  | \*this+0x11C  | enum lookup (byte)    |
| `repair_points`             | `RepairPoints`           | f32      | \*this+0x390  | getTextAsFloat        |
| `cost_escalation`           | `CostEscalation`         | f32      | \*this+0x394  | getTextAsFloat        |
| `starting_velocity`         | `StartingVelocity`       | f32      | \*this+0x39C  | getTextAsFloat        |
| `acceleration`              | `Acceleration`           | f32      | \*this+0x3A0  | getTextAsFloat        |
| `fuel`                      | `Fuel`                   | f32      | \*this+0x3A4  | getTextAsFloat        |
| `perturbance_chance`        | `PerturbanceChance`      | f32      | \*this+0x3A8  | getTextAsFloat        |
| `perturbance_velocity`      | `PerturbanceVelocity`    | f32      | \*this+0x3AC  | getTextAsFloat        |
| `perturbance_min_time`      | `PerturbanceMinTime`     | f32      | \*this+0x3B0  | getTextAsFloat        |
| `perturbance_max_time`      | `PerturbanceMaxTime`     | f32      | \*this+0x3B4  | getTextAsFloat        |
| `perturb_initial_velocity`  | `PerturbInitialVelocity` | f32      | \*this+0x3C0  | getTextAsFloat        |
| `terrain_height_tolerance`  | `TerrainHeightTolerance` | f32      | \*this+0x60   | getTextAsFloat        |
| `max_velocity`              | `MaxVelocity`            | f32      | r14+0x60      | getTextAsFloat        |
| `reverse_speed`             | `ReverseSpeed`           | f32      | \*this+0x388  | getTextAsFloat        |
| `pick_radius`               | `PickRadius`             | f32      | \*this+0x6C   | getTextAsFloat        |
| `pick_offset`               | `PickOffset`             | f32      | \*this+0x70   | getTextAsFloat        |
| `pick_priority`             | `PickPriority`           | i32/enum | \*this+0x74   | enum lookup           |
| `tracking_delay`            | `TrackingDelay`          | f32      | —             | getTextAsFloatValue   |
| `lifespan`                  | `Lifespan`               | f32→i32  | \*this+0xBC   | float×const→int       |
| `block_movement_object`     | `BlockMovementObject`    | i32      | \*this+0xB4   | getProtoObjectID      |

## Dead Data (in XML, engine ignores)

Fields present in game XML data but no corresponding `stricmp` in `loadFromXml`.

| XML Name | Notes                                                                                                                                                                                 |
| -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `LOS`    | No string literal "LOS" found in binary. Engine has `LOSType`, `LOSObstructable`, debug printf `"LOS: %f"` — but no `stricmp("LOS")` in `loadFromXml`. Kept in struct for round-trip. |

## IDA-Confirmed In `loadFromXml` — Not Yet In Struct

### Simple Scalar Fields (need type verification)

| XML Name                      | xref in `loadFromXml` | Notes                                     |
| ----------------------------- | --------------------- | ----------------------------------------- |
| `SubSelectSort`               | 0x1403482fb           |                                           |
| `VisualDisplayPriority`       | 0x1403485c1           |                                           |
| `TrueLOSHeight`               | 0x140348621           |                                           |
| `FlightLevel`                 | 0x14034780c           |                                           |
| `RamDodgeFactor`              | 0x1403483c7           |                                           |
| `GathererLimit`               | 0x14034409e           |                                           |
| `GarrisonTime`                | 0x14034864e           |                                           |
| `RevealRadius`                | 0x1403487b5           |                                           |
| `BuildRotation`               | 0x14034867b           |                                           |
| `BuildOffset`                 | 0x1403486a8           |                                           |
| `ExitFromDirection`           | 0x140347839           |                                           |
| `PopCapAddition`              | 0x14034756c           |                                           |
| `AIAssetValueAdjust`          | 0x140342ecb           |                                           |
| `AutoParkingLot`              | 0x1403486d5           |                                           |
| `ChildObjectDamageTakenScalar`| 0x1403485f4           |                                           |
| `BuildingStrengthDisplay`     | 0x14034873e           |                                           |
| `AttackGradeDPS`              | 0x14034839a           |                                           |
| `MaxContained`                | 0x1403480df           |                                           |
| `ShieldType`                  | 0x140348776           |                                           |
| `Shieldpoints`                | 0x1403427fe           | also at 0x140347b6a (2 xrefs)            |
| `KillBeam`                    | 0x14034880f           |                                           |
| `TargetBeam`                  | 0x1403487df           |                                           |
| `BeamHead`                    | 0x140347c9e           |                                           |
| `BeamTail`                    | 0x140347cdf           |                                           |

### String/Lookup Fields (need type verification)

| XML Name                | xref in `loadFromXml` | Notes                                    |
| ----------------------- | --------------------- | ---------------------------------------- |
| `MinimapIcon`           | 0x1403463de           |                                          |
| `MinimapColor`          | 0x14034646b           |                                          |
| `ExtendedSoundBank`     | 0x140346364           |                                          |
| `LevelUpEffect`         | 0x140347dbf           |                                          |
| `ImpactDecal`           | 0x140346022           |                                          |
| `StatsNameID`           | 0x140343633           |                                          |
| `RoleTextID`            | 0x1403439cf           |                                          |
| `PrereqTextID`          | 0x140343917           |                                          |
| `CostEscalationObject`  | 0x140342bee           |                                          |
| `AddResource`           | 0x140343f75           |                                          |
| `AbilityCommand`        | 0x140343af2           |                                          |
| `HPBar`                 | 0x1403478d8           |                                          |

### Complex Sub-element Fields (need sub-structs)

| XML Name               | xref in `loadFromXml` | Notes                                          |
| ---------------------- | --------------------- | ---------------------------------------------- |
| `SingleBoneIK`         | 0x140341d0b           |                                                |
| `SweetSpotIK`          | 0x140341f19           |                                                |
| `GroundIK`             | 0x140341e47           |                                                |
| `PhysicsInfo`          | 0x1403422d1           | delegates to `BPhysicsInfo__loadFromXml`        |
| `SquadModeAnim`        | 0x140343062           | has `@SquadMode` attribute + anim lookup        |
| `Sound`                | 0x140341891           | child element (complex handler), 2 xrefs        |
| `Command`              | 0x14034658a           |                                                |
| `DamageType`           | 0x14034543d           |                                                |
| `Contain`              | 0x14034821b           |                                                |
| `Socket`               | 0x140346fb5           | 2 xrefs in loadFromXml                          |
| `ChildObjects`         | 0x140346cd1           |                                                |
| `Lifespan`             | 0x140344146           | already in struct (case variant `LifeSpan`)     |

## Dead Data / Not In `loadFromXml`

| XML Name                | Where Referenced                    | Notes                                   |
| ----------------------- | ----------------------------------- | --------------------------------------- |
| `TrackInterceptDistance` | `BDatabase__loadGameData` only      | Not a ProtoObject field                 |
| `DazeResist`            | `BProtoSquad__loadFromXml` only     | Squad field, not ProtoObject            |
| `FlashUI`               | Flash UI system funcs only          | Not loaded in `loadFromXml`             |
| `MinimapIconName`       | No exact string match               | `MinimapIcon` exists instead            |
| `UIVisual`              | No exact string match               | Not found in binary                     |
| `ShieldPoints`          | Case variant of `Shieldpoints`      | Engine uses `_stricmp`, same handler    |
| `LifeSpan`              | Case variant of `Lifespan`          | Engine uses `_stricmp`, same handler    |
| `Physicsinfo`           | Case variant of `PhysicsInfo`       | Engine uses `_stricmp`, same handler    |

Note: `Power`, `Ability`, `Pop`, `Cost`, `Rate`, `LOS` are ALL confirmed in `loadFromXml`
via `reference_code/ida.cpp` decompilation — earlier xref-only analysis was incomplete.
