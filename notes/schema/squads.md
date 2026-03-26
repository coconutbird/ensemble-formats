# squads.xml.xmb — Schema Notes

Loader: `BProtoSquad::loadFromXml` @ `0x140350d90`

## Fields To Add (all 15 verified in IDA)

All 15 extra-field warnings are real — every string exists in the binary and
is referenced by `BProtoSquad::loadFromXml`.

| XML Name              | Count | IDA String Addr  | xref in loader   | Notes                                      |
| --------------------- | ----- | ---------------- | ---------------- | ------------------------------------------ |
| `@formationType`      | 59x   | `0x1411b3b08`    | `0x140350e7f`    | attribute on Squad node                    |
| `@update`             | 1x    | —                | —                | attribute (already handled by objects too)  |
| `AbilityRecoveryBar`  | 113x  | `0x1411b3b88`    | `0x140351da6`    |                                            |
| `BobbleHead`          | 12x   | `0x1411a35e8`    | `0x140351fb8`    | also in `BHPBar__loadXML`                  |
| `CanAttackWhileMoving`| 1x    | `0x1411b3de8`    | `0x140353345`    |                                            |
| `CryoPoints`          | 1x    | `0x1411b3b38`    | `0x14035202a`    |                                            |
| `DazeResist`          | 13x   | `0x1411b3b28`    | `0x140352057`    |                                            |
| `LeashDeadzone`       | 98x   | `0x1411b3c38`    | `0x140352a69`    |                                            |
| `LeashRecallDelay`    | 98x   | `0x1411b3c20`    | `0x140352a96`    |                                            |
| `MinimapScale`        | 111x  | `0x1411b3cd8`    | `0x140352b7e`    |                                            |
| `Selection`           | 117x  | `0x1411b3ad8`    | `0x140351752`    | also in `sub_140E0B2D0`                    |
| `Sound`               | 52x   | `0x14119b098`    | `0x140352f6b`    | also in Object + other loaders             |
| `StatsNameID`         | 6x    | `0x1411b2448`    | `0x1403512ba`    | also in `BProtoObject::loadFromXml`        |
| `TurnRadius`          | 17x   | `0x1411b3c60`    | `0x14035296f`    |                                            |
| `VeterancyBar`        | 114x  | `0x1411a3608`    | `0x140351b94`    | also in `BHPBar__loadXML`                  |

## Dead Data

None — all extra fields are real.
