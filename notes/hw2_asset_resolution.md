# HW2 Retail Asset Resolution

How Halo Wars 2 maps game objects to their art/animation/physics assets at runtime.
No `.dep` files exist in retail — dependencies are embedded in the data itself.

## Overview

```
all_objects.xml.xmb          "give me cov_veh_gorgon_01"
        │
        ├─ Visual ──────────► gorgon01.vis.xmb        (the manifest)
        │                         │
        │                         ├─ model "gorgon01"
        │                         │    ├─ mesh ──────► mesh_gorgon01.ugx
        │                         │    ├─ damagefile ► gorgon01.dmg.xmb
        │                         │    ├─ anims ─────► idle01_main.uax, walk_01.uax, ...
        │                         │    └─ attachments
        │                         │         ├─ ModelRef "RocketLauncher_Right"
        │                         │         ├─ ModelRef "AutocannonTurret"
        │                         │         ├─ LightFile "Lights/GorgonLightsCannon"
        │                         │         └─ FoliageForceFile "Foliage/gorgon"
        │                         │
        │                         ├─ model "RocketLauncher_Right"
        │                         │    ├─ mesh ──────► mesh_rocket_right_0.ugx
        │                         │    ├─ damagefile ► rocket_launcher_right.dmg.xmb
        │                         │    └─ anims ─────► missile_attack01_rocket_right.uax
        │                         │
        │                         ├─ model "RocketLauncher_Left"
        │                         │    └─ (same pattern)
        │                         │
        │                         ├─ model "AutocannonTurret"
        │                         │    ├─ mesh ──────► mesh_cannon_turret_0.ugx
        │                         │    └─ anims ─────► cannon_attack01_cannon_turret.uax
        │                         │
        │                         ├─ model "Driver"        (external unit ref)
        │                         └─ model "SpartanDriver" (external unit ref)
        │
        ├─ PhysicsInfo ─────► units/.../physics/intact.hkt
        ├─ Tactics ─────────► cov_veh_gorgon_01.tactics.xmb
        └─ (stats, hardpoints, veterancy, etc.)
```

## The Three Databases

### 1. `all_objects.xml.xmb` — Master Object List

Packed inside `data/db/lists/lists.pkg`. Equivalent to HW1's `proto.xml`.
Each `<Object>` entry defines a game entity with inheritance:

```xml
<Object name="cov_veh_gorgon_01_base" dbid="9688193681871967002">
    <Visual>units\covenant\vehicles\gorgon01\gorgon01.vis</Visual>
    <PhysicsInfo>units\covenant\vehicles\gorgon01\physics\intact</PhysicsInfo>
    <Tactics>cov_veh_gorgon_01.tactics</Tactics>
    <!-- stats, hardpoints, veterancy, etc. -->
</Object>

<Object name="cov_veh_gorgon_01_mp" parent_element="cov_veh_gorgon_01_base">
    <Visual>units\covenant\vehicles\gorgon01\gorgon02.vis</Visual>
    <Tactics>cov_veh_gorgon_01_mp.tactics</Tactics>
</Object>

<Object name="cov_veh_gorgon_01_sp" parent_element="cov_veh_gorgon_01_base"/>
<Object name="cov_veh_gorgon_01"    parent_element="cov_veh_gorgon_01_sp"/>
```

Inheritance chain: `cov_veh_gorgon_01` → `_sp` → `_base`.
MP variant overrides `Visual` and `Tactics` only.

### 2. `all_squads.xml.xmb` — Squad Definitions

Also in `lists.pkg`. Maps squad names to their unit composition:

```xml
<Squad name="cov_veh_gorgon_01_base">
    <Units>
        <Unit count="1" role="normal">cov_veh_gorgon_01</Unit>
    </Units>
</Squad>
```

### 3. Tactics — `data/db/tactics/*.tactics.xmb`

Packed in `tactics.pkg`. Defines weapons, actions, and abilities.
Referenced by name from `all_objects.xml` (no path, just the base name).

## The `.vis.xmb` File — The Asset Manifest

The `.vis` (visual) file is the central manifest for all renderable assets.
It lives alongside the meshes in the unit directory.

### Structure

```xml
<visual defaultmodel="gorgon01">
    <!-- Primary model -->
    <model name="gorgon01">
        <component>
            <asset type="Model">
                <file>units\covenant\vehicles\gorgon01\mesh_gorgon01</file>
                <damagefile>units/covenant/vehicles/gorgon01/gorgon01</damagefile>
            </asset>
            <!-- Bone attachments for sub-models -->
            <attach type="ModelRef" name="RocketLauncher_Right"
                    tobone="socketbone_rocket_right_0"
                    frombone="GrannyRootBone_rocket_right_0"/>
            <attach type="LightFile" name="Lights/GorgonLightsCannon"
                    tobone="bone_vfx_LightCannon01"/>
        </component>
        <anim type="Idle">
            <asset type="Anim">
                <file>animations/covenant/vehicles/gorgon01/idle01_main</file>
            </asset>
        </anim>
        <!-- more anims: Jog, Death, CannonAttack, BoostJump*, etc. -->
    </model>

    <!-- Sub-models (resolved by name matching the ModelRef above) -->
    <model name="RocketLauncher_Right">
        <component>
            <asset type="Model">
                <file>units/covenant/vehicles/gorgon01/mesh_rocket_right_0</file>
                <damagefile>units/covenant/vehicles/gorgon01/rocket_launcher_right</damagefile>
            </asset>
        </component>
        <anim type="MissileAttackRight">...</anim>
    </model>

    <model name="AutocannonTurret">
        <component>
            <asset type="Model">
                <file>units/covenant/vehicles/gorgon01/mesh_cannon_turret_0</file>
            </asset>
        </component>
    </model>
</visual>
```

### Key Points

- **`<file>` paths have no extension** — the engine appends `.ugx` for meshes, `.uax` for anims
- **Sub-models are defined in the same `.vis`** — the `<model name="X">` matches the
  `<attach type="ModelRef" name="X">` in the parent model's component
- **`<damagefile>` paths** resolve to `.dmg.xmb` files (damage state definitions)
- **Attachment types**: `ModelRef`, `LightFile`, `FoliageForceFile`, `TerrainEffect`, `Particle`

## The `.dmg.xmb` File — Damage Model

Defines progressive destruction. Each `<event>` fires at a health threshold:

- `togglepart` — show damage decals on the mesh
- `swappart` — swap intact geometry for damaged version (both baked into the `.ugx`)
- `throwpart` — detach a mesh part with physics (`physics="units_covenant_vehicles_gorgon01_armour_back"`)
- `attachparticle` — spawn VFX at a bone
- `explosivedeath` — final destruction with debris physics

Physics references in `throwpart` resolve to `.hkt` files in the `physics/` subdirectory
using the convention: underscores replace path separators.

## Directory Layout

Retail assets are organized by faction/category/unit:

```
data/
├── units/
│   ├── covenant/
│   │   ├── vehicles/
│   │   │   └── gorgon01/
│   │   │       ├── gorgon01.vis.xmb           # visual manifest (SP)
│   │   │       ├── gorgon02.vis.xmb           # visual manifest (MP)
│   │   │       ├── gorgon01.dmg.xmb           # main body damage model
│   │   │       ├── rocket_launcher_left.dmg.xmb
│   │   │       ├── rocket_launcher_right.dmg.xmb
│   │   │       ├── mesh_gorgon01.ugx          # main body mesh (1.3 MB)
│   │   │       ├── mesh_cannon_turret_0.ugx   # turret sub-mesh
│   │   │       ├── mesh_rocket_left_0.ugx     # left launcher sub-mesh
│   │   │       ├── mesh_rocket_right_0.ugx    # right launcher sub-mesh
│   │   │       └── physics/
│   │   │           ├── intact.hkt             # intact collision
│   │   │           ├── intact.physics.xmb
│   │   │           ├── core.hkt
│   │   │           ├── fract.hkt
│   │   │           ├── armour_back.hkt         # debris physics shapes
│   │   │           ├── armour_nose.hkt
│   │   │           ├── armour_top_left.hkt
│   │   │           ├── armour_top_right.hkt
│   │   │           ├── armour_leg_right.hkt
│   │   │           ├── armour_rocket_left_01.hkt
│   │   │           ├── armour_rocket_left_02.hkt
│   │   │           └── armour_rocket_right_01.hkt
│   │   ├── infantry/
│   │   ├── aerial/
│   │   └── structures/
│   ├── unsc/
│   └── forerunner/
├── animations/                                # .uax files mirror unit paths
│   └── covenant/vehicles/gorgon01/
│       ├── idle01_main.uax
│       ├── walk_01.uax
│       ├── death01.uax
│       └── ...
├── environment/                               # terrain, props, sky domes
├── archetypes/                                # foliage, grass, flora
├── db/
│   ├── lists/
│   │   ├── lists.pkg                          # bundles all .xmb below
│   │   ├── all_objects.xml.xmb                # master object database (1.7 MB)
│   │   ├── all_squads.xml.xmb                 # squad compositions
│   │   └── objecttypes.xml.xmb               # type enum definitions
│   ├── tactics/
│   │   ├── tactics.pkg
│   │   └── cov_veh_gorgon_01.tactics.xmb
│   └── objects/fx/                            # VFX visual definitions
├── lights/                                    # .light files
├── vfx/                                       # particle effect definitions
└── textures/                                  # .ddx texture files
```

## Resolving "Give Me All Assets for Gorgon"

To collect every file needed to render a unit:

1. **Parse `all_objects.xml.xmb`** — find the object by name, resolve inheritance,
   extract the `<Visual>` path

2. **Parse the `.vis.xmb`** — walk every `<model>` section:
   - Collect all `<file>` paths from `<asset type="Model">` → append `.ugx`
   - Collect all `<file>` paths from `<asset type="Anim">` → append `.uax`
   - Collect all `<damagefile>` paths → append `.dmg.xmb`
   - Collect attachment references (`LightFile`, `FoliageForceFile`, `TerrainEffect`)

3. **Parse each `.dmg.xmb`** — extract `throwpart` physics references → resolve to `.hkt`

4. **Collect physics** from the `<PhysicsInfo>` path in `all_objects.xml`

5. **Textures** are referenced inside the `.ugx` material data (not in the XML chain)

### File Type Summary

| Extension      | Format       | Description                                       |
| -------------- | ------------ | ------------------------------------------------- |
| `.ugx`         | ECF          | Geometry (vertices, indices, skeleton, materials) |
| `.uax`         | ECF          | Animation clips                                   |
| `.vis.xmb`     | ECF+XMB      | Visual manifest (models, anims, attachments, VFX) |
| `.dmg.xmb`     | ECF+XMB      | Damage model (destruction sequence)               |
| `.tactics.xmb` | ECF+XMB      | Combat tactics (weapons, abilities)               |
| `.hkt`         | Havok        | Physics collision/debris shapes                   |
| `.physics.xmb` | ECF+XMB      | Physics metadata                                  |
| `.ddx`         | ECF          | Textures (DDS variant)                            |
| `.ufx`         | ECF          | Visual effects                                    |
| `.pkg`         | Ensemble PKG | Archive bundling multiple `.xmb` files            |
