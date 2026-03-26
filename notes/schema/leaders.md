# leaders.xml.xmb — Schema Notes

Loader: `BLeader__parseFromXml` @ `0x140253540`

## Fields To Add (1 of 2 verified in IDA)

| XML Name             | Count | IDA String Addr  | xref in loader   | Notes                                      |
| -------------------- | ----- | ---------------- | ---------------- | ------------------------------------------ |
| `ReverseHotDropCost` | 3x    | `0x1411a9778`    | `0x1402546e3`    |                                            |

| `@Test`              | 1x    | (inlined)        | `0x1402535d4`    | Found via decompilation — `BXMLReader__getAttributeAsString(v2, "Test", ...)` → `getTextAsBool` |

## Dead Data

(none — all fields confirmed real)
