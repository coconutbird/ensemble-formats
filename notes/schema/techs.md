# techs.xml.xmb — Schema Notes

Tech loader: `BTech__loadFromXml` @ `0x140356AD0`
Effect loader: `BTechEffect__loadFromXml` @ `0x14040E2C0`

## Fields To Add — Tech level (in `BTech__loadFromXml`)

| XML Name               | Count | IDA String Addr  | xref in loader   | Notes                                      |
| ---------------------- | ----- | ---------------- | ---------------- | ------------------------------------------ |
| `DisplayNameID`        | 134x  | `0x14118f320`    | `0x140357041`    | also in many other loaders                 |
| `RolloverTextID`       | 134x  | `0x14118f340`    | `0x1403570f3`    | also in many other loaders                 |
| `PrereqTextID`         | 88x   | `0x1411b24f0`    | `0x1403571a5`    | also in Object/Squad/Power loaders         |
| `Prereqs`              | 80x   | `0x1411b4118`    | `0x1403574ae`    |                                            |
| `ResearchAnim`         | 11x   | `0x1411b4120`    | `0x140357433`    |                                            |
| `ResearchCompleteSound`| 118x  | `0x1411b4168`    | `0x140357385`    |                                            |
| `Alpha`                | 2x    | `0x14119b9e8`    | `0x140356bae`    | also in Civ/Database loaders               |

## Fields To Add — Effect level (in `BTechEffect__loadFromXml`)

These are attributes on the `<Effect>` sub-element of `<Tech>`.

| XML Name        | Count | IDA String Addr  | xref in loader   | Notes                                      |
| --------------- | ----- | ---------------- | ---------------- | ------------------------------------------ |
| `@action`       | 222x  | `0x14118f484`    | `0x14040e3ae`    | "Action" string                            |
| `@allactions`   | 289x  | `0x1411be3e8`    | `0x14040e38d`    | "AllActions" string                        |
| `@CommandData`  | 388x  | `0x1411be550`    | `0x14040ed9f`    |                                            |
| `@commandType`  | 388x  | `0x1411be578`    | `0x14040ed17`    | "CommandType" string                       |
| `@FromType`     | 47x   | `0x1411be588`    | `0x14040f055`    |                                            |
| `@ToType`       | 47x   | `0x1411be594`    | `0x14040f08f`    |                                            |
| `@Hardpoint`    | 2x    | `0x1411b2010`    | `0x14040ebd6`    | also in Object/Weapon loaders              |
| `@hpbar`        | 3x    | `0x1411be5c4`    | `0x14040ea56`    |                                            |
| `@iconName`     | 26x   | `0x1411be5e8`    | `0x14040e9ba`    |                                            |
| `@iconType`     | 26x   | `0x1411be540`    | `0x14040e8d0`    |                                            |
| `@impactEffect` | 2x    | `0x1411a4528`    | —                | "ImpactEffect" string                      |
| `@popType`      | 2x    | `0x1411be560`    | `0x14040ec54`    | "PopType" string                           |
| `@power`        | 28x   | `0x1411a2800`    | `0x14040e6e8`    | "Power" string, also in many loaders       |
| `@Resource`     | 41x   | `0x1411a4280`    | `0x14040eb03`    | also in Leader/UI loaders                  |
| `@squadName`    | 1x    | `0x1411be5d0`    | `0x14040eaaf`    |                                            |
| `@unitType`     | 6x    | `0x1411b4108`    | —                | "unitType" string, in BTech__loadFromXml   |
| `@Ability`      | 16x   | `0x14118f2e8`    | `0x14040e9fd`    | "Ability" string                           |
| `@Alpha`        | 2x    | `0x14119b9e8`    | —                | same string as tech-level Alpha            |

Also in `BTech__loadFromXml` (found via decompilation, not global string search):

| XML Name        | Count | xref in loader   | Notes                                      |
| --------------- | ----- | ---------------- | ------------------------------------------ |
| `Cost`          | 134x  | `0x140357261`    | calls `BProtoObject__loadResourceCost`     |
| `Icon`          | 134x  | `0x14035732c`    | reads text value into string at +40        |
| `StatsObject`   | 4x    | `0x140357b94`    | calls `BDatabase__getProtoObjectID`        |

## Dead Data

None — all 27 extra fields are real (25 found via xrefs, 2 via decompilation).
