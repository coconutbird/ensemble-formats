# abilities.xml.xmb — Schema Notes

Loader: `BAbility__parseFromXml` @ `0x1400f53c0`

## Fields To Add (all 5 verified in IDA)

All 5 extra-field warnings are real — every string exists in the binary and
is referenced by `BAbility__parseFromXml`.

| XML Name               | Count | IDA String Addr  | xref in loader   | Notes                                      |
| ---------------------- | ----- | ---------------- | ---------------- | ------------------------------------------ |
| `DamageTakenModifier`  | 2x    | `0x14118f2f0`    | `0x1400f595d`    |                                            |
| `RecoverAnimAttachment`| 2x    | `0x14118f388`    | `0x1400f5d32`    |                                            |
| `RecoverEndAnim`       | 2x    | `0x14118f3b8`    | `0x1400f5e17`    |                                            |
| `RecoverStartAnim`     | 2x    | `0x14118f3a0`    | `0x1400f5da1`    |                                            |
| `SmartTargetRange`     | 4x    | `0x14118f420`    | `0x1400f617a`    |                                            |

## Dead Data

None — all extra fields are real.
