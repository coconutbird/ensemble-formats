# Hogan Shader Family Analysis

Compiled shader data extracted from HW2 `.ufx` files using `d3dasm`.
All shaders are SM5 DXBC compiled with `Microsoft (R) HLSL Shader Compiler 10.0.10011.16384`.

## Shader Families

### hogan_standard

**Purpose:** PBR surface shader for static and skinned meshes (deferred GBuffer writer).

**Pixel Shader Textures (base):**

| Slot | Name                   | Type         |
| ---- | ---------------------- | ------------ |
| 0    | `Albedo_Map`           | texture 2dMS |
| 1    | `Normal_Map`           | texture 2dMS |
| 2    | `BRDF_Parameter_Map`   | texture 2dMS |
| 3    | `Specular_Reflectance` | texture 2dMS |

**Optional textures (controlled by permutation bitmask):**

| Name                    | Example bitmask    | Notes                   |
| ----------------------- | ------------------ | ----------------------- |
| `Sand_Map` (slot 4)     | `00020000A83009A0` | Desert terrain blend    |
| `Emissive_Map` (slot 4) | `00000101003009A0` | Self-illumination       |
| `Parallax_Map`          | `0000004100300960` | Height-based parallax   |
| `DissolveMask`          | `0004000100300960` | Destruction/fade effect |

**Constant Buffers:**

- `UbershaderPSC` (512 bytes) — generic parameter blob (`rp_parameter_ps`)
- `InstanceSC` (32 bytes) — `InstanceControl`, `TeamTintColour`, `EmissiveTintColour`, `CamouflageColour`, `BoneIndex`

**Output:** 4 render targets (deferred GBuffer — albedo, normal, material, velocity/misc).

**Instruction count:** ~34 (base permutation).

**Quality levels:** Permutation sets contain up to 4 entries:

- Slots 0–1 (Standard & High): full texture set
- Slots 2–3 (Shadow & Shadow+): no textures, depth-only output

### hogan_decal

**Purpose:** Projected decals (deferred GBuffer writer, same pipeline as standard).

**Pixel Shader Textures:** Same as `hogan_standard` base — `Albedo_Map`, `Normal_Map`, `BRDF_Parameter_Map`, `Specular_Reflectance`.

**Differences from standard:**

- Vertex shader takes `COLOR` input (vertex color for projection blending)
- VS outputs 7 interpolants (vs 5 for standard)
- Slightly more PS instructions (~40 vs ~34), likely projection math
- Always single-slot permutation sets (no quality levels)

**Constant Buffers:** Same as `hogan_standard`.

**Output:** 4 render targets (same deferred GBuffer).

### hogan_water

**Purpose:** Water surface rendering (forward shader, NOT deferred).

**Pixel Shader Textures:**

| Slot | Name                       | Type              | Notes                        |
| ---- | -------------------------- | ----------------- | ---------------------------- |
| 0    | `Normal_Map`               | texture 2dMS      | Water surface normals        |
| 1    | `Foam_Map`                 | texture 2dMS      | Shore/wave foam              |
| 2    | `WaveMask_Map`             | texture 2dMS      | Wave pattern mask            |
| 5    | `ShadowDepthTexture`       | texture 2dMS      | Scene shadow map             |
| 6    | `IBLTexture`               | texture 2dMSarray | Image-based lighting cubemap |
| 10   | `IBLSHCoefficientsBuffer`  | structured buf    | SH lighting coefficients     |
| 11   | `accumulatedVolumeFog_TEX` | texture 2darray   | Volumetric fog               |
| 12   | `EnvBRDFTexture`           | texture 2dMS      | Environment BRDF LUT         |
| 13   | `SpotShadowDepthTexture`   | texture 2dMS      | Spot light shadow map        |
| 15   | `tileVector`               | structured buf    | Tiled lighting data          |

**Optional textures (by permutation):**

- `Flow_Map` — flow-based UV animation
- `AlphaMask_Map` — edge/shore alpha masking
- `Secondary_Normal_Map` — detail normal layer
- `RefractionSceneTexture` / `DepthSceneTexture` — underwater refraction
- `ReflectionPlaneTexture` — planar reflections

**Constant Buffers:**

- `DefaultXSC` (560 bytes) — camera matrices, time, fog, AO, HDR params
- `DefaultPSC` — additional scene params
- `TileLightingLightVector` — tiled forward lighting
- `UbershaderPSC` (512 bytes) — generic parameter blob

**Output:** Forward-rendered (does its own lighting, shadows, fog inline).

**Key difference:** Completely separate rendering pipeline from standard/decal. Scene-level textures (shadows, IBL, refraction) are bound directly — not a deferred GBuffer writer.

## Permutation Bitmask → Texture Set (hogan_standard)

Unique texture sets observed across all `hogan_standard` permutations, ordered by frequency:

| Count | Texture Set                                                | Example Bitmask    |
| ----- | ---------------------------------------------------------- | ------------------ |
| 181   | _(shadow-only, no textures)_                               | `0000000000300900` |
| 82    | Albedo                                                     | `0008000000000960` |
| 82    | Albedo + Normal + BRDF + Spec                              | `0000000000000944` |
| 82    | Albedo + Normal + BRDF + Spec + Sand                       | `0000000008000984` |
| 62    | Albedo + Normal + BRDF + Spec + Emissive                   | `0000000100300920` |
| 26    | Albedo + Normal + BRDF + Spec + Emissive + DissolveMask    | `0004000100300960` |
| 24    | DissolveMask only                                          | `000c0000000009a0` |
| 18    | Albedo + DissolveMask                                      | `000c000000000960` |
| 18    | Albedo + Normal + BRDF + Spec + EnvBRDF + IBL + Shadow     | `0000000000000948` |
| 14    | Albedo + Normal + BRDF + Spec + DissolveMask               | `0004000000300960` |
| 14    | Albedo + Normal + BRDF + Spec + Emissive + Parallax        | `0000004100300960` |
| 11    | Albedo + Normal + BRDF + Spec + Parallax                   | `0000004000000984` |
| 10    | Albedo + Normal + BRDF + Spec + Emissive + Sand            | `00000001183009a0` |
| 8     | Albedo + DissolveMask + Parallax                           | `000c004000000960` |
| 8     | Albedo + Normal + BRDF + Spec + Emissive + Parallax + Sand | `00000041183009a0` |

## Decal Permutations

| Count | Texture Set                               | Bitmask            |
| ----- | ----------------------------------------- | ------------------ |
| 11    | Albedo + Normal + BRDF + Spec             | `0000000000000017` |
| 1     | Albedo + Normal + BRDF + Spec + Shoreline | `0000000000069015` |

## Water Permutations

All water permutations include scene-level textures (Shadow, IBL, EnvBRDF, SpotShadow).
Material-specific textures vary:

| Count | Material Textures                                                       | Bitmask            |
| ----- | ----------------------------------------------------------------------- | ------------------ |
| 9     | Normal + Foam + SecondaryNormal + Refraction + Depth                    | `000000000003acab` |
| 4     | Normal + Foam + Flow + AlphaMask + SecondaryNormal + Refraction + Depth | `000000000091bcab` |
| 4     | Normal + Foam + AlphaMask + SecondaryNormal + Depth                     | `000000000017a4b5` |
| 3     | Normal + Foam + Flow + SecondaryNormal + Refraction + Depth             | `000000000081baab` |
| 1     | Normal + Foam + WaveMask                                                | `000000000203a015` |
