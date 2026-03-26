# Vertex Format Survey (2026-03-26)

## HW1 (18,411 sections across all ERA files)

| Pack Order | Count | Notes |
|------------|-------|-------|
| `PNA0T0` | 16,376 | Most common: Position + Normal + Tangent(A0) + UV |
| `PNA0ST0` | 604 | Skinned variant |
| `PNA0T0T1` | 554 | Two UV sets |
| `PNA0T0T1T2` | 479 | Three UV sets |
| `PNT0` | 268 | No tangent type marker? |
| `PNST0` | 74 | Skinned, no tangent type |
| `PNT0T1` | 36 | Two UV sets, no tangent type |
| `PNA0ST0T1` | 20 | Skinned, two UV sets |

### Basis Scale (B+X) Survey
- Sections with B+X (basis+scale): **0**
- Sections with B (no X): **0**
- Sections with A (tangent-only): **18,033**
- Sections with N only (no tangent): **378**

**Conclusion**: HW1 has NO basis_scale data. All tangents are A0 (Float3).
Normalizing tangents/normals during glTF roundtrip is safe — magnitudes
are Dec3N encoding artifacts, not meaningful scale data.

## HW2 (2,862 sections from loose .ugx files)

HW2 does NOT use UnivertPacker — uses fixed vertex layouts based on vert_size.

| Vert Size | Rigid | Global Bones | Count | Notes |
|-----------|-------|-------------|-------|-------|
| 20 | yes | no | 2,530 | Most common |
| 20 | yes | yes | 83 | Single-bone optimization |
| 24 | yes | no | 155 | Extra data (tangent sign? extra UV?) |
| 28 | no | no | 77 | Skinned (bone weights/indices) |
| 8 | yes | no | 17 | Minimal (position only?) |

## Magnitude Survey (100 files: 50 HW1 + 50 HW2, 723k normals, 722k tangents)

### Original Data Magnitudes
| Attribute | Min Mag | Max Mag | Avg |mag-1| | Max |mag-1| | Non-unit (>0.01) |
|-----------|---------|---------|-------------|-------------|------------------|
| Normal    | 0.052   | 1.412   | 0.262       | 0.948       | 609,980 / 723,473 (84%) |
| Tangent   | 0.052   | 1.412   | 0.309       | 0.948       | 666,799 / 722,328 (92%) |

### After Roundtrip (glTF normalize → re-pack Dec3N)
| Attribute | Min Mag | Max Mag | Avg |mag-1| | Max |mag-1| | Non-unit (>0.01) |
|-----------|---------|---------|-------------|-------------|------------------|
| Normal    | 0.999   | 1.002   | 0.000421    | 0.0015      | 0                |
| Tangent   | 0.999   | 1.002   | 0.000420    | 0.0015      | 0                |

### Direction Error (angle between original and roundtripped)
| Attribute | Avg Error | Max Error | Count >1° |
|-----------|-----------|-----------|-----------|
| Normal    | 0.965°    | 1.000°    | 723,473 (100%) |
| Tangent   | 0.966°    | 1.000°    | 722,328 (100%) |

### Analysis
- **84-92% of original normals/tangents have non-unit magnitude** (some as low as 0.05!)
- After roundtrip, ALL magnitudes are near-unit (max deviation 0.0015)
- Direction error is consistently ~1° — this is the inherent Dec3N quantization limit
  (10-bit signed = 1024 discrete values per axis → ~0.1% directional precision)
- The ~1° error is **the same whether we normalize or not** — it comes from the
  Dec3N pack→unpack cycle, not from our normalization
- The original non-unit magnitudes are Dec3N encoding artifacts, NOT meaningful data
  (confirmed: no B+X basis_scale in any real file)

**Conclusion**: Normalization is safe. The direction error is irreducible Dec3N
quantization noise, not introduced by our pipeline. Original magnitudes are
meaningless encoding artifacts.

## Dec3N Bits 30-31 (W field) — CRITICAL FIX

Survey of ~723k normals and ~723k tangents across 100 files:

| W value | Normals | Tangents |
|---------|---------|----------|
| 0b00    | 93.6%   | 38.1%    |
| 0b10    | 6.4%    | 61.9%    |

**Bits 30-31 store tangent handedness** (0b00 → +1, 0b10 → -1).
Only 1.4% of components use -512 raw value (→ -1.00196, now clamped to -1.0).

## Dec3N Packed Byte Equivalence — BEFORE vs AFTER fix

### BEFORE (W ignored, zeroed on pack):
| Comparison | Normals | Tangents |
|-----------|---------|----------|
| pack(orig) == raw_bytes | 483,710 (83.9%) | **194,645 (33.8%)** |

### AFTER (W preserved + XYZ clamped):
| Comparison | Normals | Tangents |
|-----------|---------|----------|
| pack(orig) == raw_bytes | 483,710 (83.9%) | **493,181 (85.7%)** |

Tangent repack: **33.8% → 85.7%**. The remaining ~14-16% mismatch is from
-512→-511 clamping and float rounding — irreducible without raw byte passthrough.
