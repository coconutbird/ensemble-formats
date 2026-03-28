# CB Parameter Inference — Manual Validation Results

## Test Coverage

Validated 6 permutations covering **8,156 models** out of ~10K Hogan Standard models.

| Permutation | Models | PS Params | Identified | Unknown | Issues |
|---|---|---|---|---|---|
| `003009a0` | 3138 | 4 | 4 | 0 | shared-UV suffix |
| `00020000c8000984` | 1364 | 12 | 5 | 7 | shared-UV suffix |
| `00000001003009a0` | 1338 | 6 | 6 | 0 | shared-UV suffix |
| `00000041003009a0` | 745 | 7 | 6 | 0 | **BUG: parallax slot**, shared-UV |
| `0000000000000984` | 630 | 4 | 4 | 0 | shared-UV suffix |
| `00000000b83009a0` | 361 | 7 | 5 | 2 | shared-UV suffix |

## Patterns — Always Correct

- **`normal_intensity`**: `mad cb8[N].x, temp, (1,1,1)` — 6/6 ✅
- **`emissive_intensity`**: `mul sampled_color * cb8[N].z` — 2/2 ✅
- **`uv_scroll`** (VS): `mad output, cb7, time, texcoord` — 6/6 ✅
- **`uv_scale`** (semantic class): always correct that it IS a UV scale — 6/6 ✅

## Bug: Parallax Texture Slot Confusion

**Permutation**: `00000041003009a0` (745 models)
**Symptom**: `cb8[0].x` labeled `uv_scale_t4` — actually `uv_scale_t0` (albedo).

In parallax permutations, the texcoord is first modified by a heightmap sample (t5),
then the CB parameter scales the *modified* texcoord before sampling albedo (t0).

```
sample r0.z, v4.xyx, t5.yzxw, s5     ; heightmap
mad r0.xy, r0.xy, r0.z, v4.xy         ; parallax offset
mul r0.zw, r0.xy, cb8[0].x            ; <-- UV SCALE (feeds t0, not t4)
mul r0.xy, r0.xy, cb8[1].y            ; <-- UV SCALE (feeds t4)
sample r1.xyz, r0.xyx, t4.xyzw, s4   ; emissive
sample r0.xyz, r0.zwz, t0.xyzw, s0   ; albedo
```

**Root cause**: `find_sampled_texture` follows the temp register forward and picks the
first `sample` it encounters. But in parallax, the SAME temp (r0) is reused:
the `.zw` components feed t0 while `.xy` feeds t4. The function finds t4 first.

**Fix**: Track which swizzle components of the temp carry the CB-scaled values and
match only `sample` instructions that actually read those components.

## Systematic Issue: Shared-UV Texture Suffix

When one `mul` produces UVs used by two `sample` instructions (common pattern —
albedo t0 shares UV with normal t1, BRDF t2 shares with specular t3), the label
only mentions the first texture.

Example: `cb8[0].xxyy => uv_scale_t0` but it also feeds t1.

**Fix**: Collect ALL texture slots fed by the temp and output `uv_scale_t0_t1`.

## Unknown Patterns Identified

Consistent patterns across multiple permutations that should become named:

| ASM Pattern | Semantic Name | Example |
|---|---|---|
| `log r, NdotV` / `mul r, r, cb` / `exp r, r` | `fresnel_power` | c8000984 insn#43 |
| `mul r, fresnel_result, cb` then adds to normal | `env_reflection_intensity` | b83009a0 insn#60 |
| `add r, -sampled, cb` in roughness output chain | `roughness_override_value` | b83009a0 insn#68 |
| `mul r, vtx_alpha, cb` in roughness output chain | `roughness_override_strength` | c8000984 insn#86 |
| `mad r, -spec*normal, cb.xyz` | `spec_override_color` | c8000984 insn#81 |
| `mul r, vtx_alpha, cb` near spec override | `spec_override_strength` | c8000984 insn#82 |
| `add cb, -1` then `mad vtx_alpha, r, 1` | `detail_blend_factor` | c8000984 insn#68 |

## Re-validation Checklist

After fixes, re-run on these 6 permutations:
- [ ] `003009a0` — baseline
- [ ] `00020000c8000984` — most unknowns
- [ ] `00000001003009a0` — emissive
- [ ] `00000041003009a0` — parallax bug
- [ ] `0000000000000984` — simple
- [ ] `00000000b83009a0` — env reflection + roughness
