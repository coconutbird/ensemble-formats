# powers.xml.xmb — Schema Notes

Loader: `BPower__parseFromXml` @ `0x14034cb00`

## Fields To Add (17 of 19 verified in IDA)

| XML Name                 | Count | IDA String Addr  | xref in loader   | Notes                                      |
| ------------------------ | ----- | ---------------- | ---------------- | ------------------------------------------ |
| `CameraEffectIn`         | 5x    | `0x1411b39a0`    | `0x14034dfc4`    |                                            |
| `CameraEffectOut`        | 5x    | `0x1411b3990`    | `0x14034e052`    |                                            |
| `CameraEnableUserScroll` | 9x    | `0x1411b3950`    | `0x14034e134`    |                                            |
| `CameraEnableUserYaw`    | 9x    | `0x1411b3938`    | `0x14034e20d`    |                                            |
| `CameraEnableUserZoom`   | 9x    | `0x1411b3a10`    | `0x14034e2e6`    |                                            |
| `CameraPitchMax`         | 10x   | `0x14119f310`    | `0x14034df8a`    | also in `sub_1401CB990` (camera config)    |
| `CameraPitchMin`         | 10x   | `0x14119f350`    | `0x14034df50`    | also in `sub_1401CB990`                    |
| `CameraZoomMax`          | 10x   | `0x14119f2e8`    | `0x14034df16`    | also in `sub_1401CB990`                    |
| `CameraZoomMin`          | 10x   | `0x14119f290`    | `0x14034dedc`    | also in `sub_1401CB990`                    |
| `Minigame`               | 1x    | `0x1411b38a8`    | `0x14034ddb7`    |                                            |
| `MultiRechargePower`     | 2x    | `0x1411b38b8`    | `0x14034d851`    |                                            |
| `NotDisruptable`         | 1x    | `0x1411b38d0`    | `0x14034d830`    |                                            |
| `SequentialRecharge`     | 2x    | `0x1411b37e8`    | `0x14034d7cd`    |                                            |
| `ShowInPowerMenu`        | 1x    | `0x1411b39c8`    | `0x14034e69a`    |                                            |
| `ShowTargetHighlight`    | 3x    | `0x1411b38e0`    | `0x14034d893`    | 2 xrefs in loader                          |
| `TechPrereq`             | 3x    | `0x1411a96f8`    | `0x14034da2d`    | also in `BLeader__parseFromXml`            |
| `UnitPower`              | 2x    | `0x1411b3810`    | `0x14034d80f`    |                                            |

| `Pop`                    | 4x    | (inlined)        | `0x14034d384`    | Found via decompilation — `BDatabase__getPopID` |

## Dead Data

| XML Name  | Notes                                                    |
| --------- | -------------------------------------------------------- |
| `FlashUI` | String exists (`0x1411c17e0`) but only in UI loaders     |
