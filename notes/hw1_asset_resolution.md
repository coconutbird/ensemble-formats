# HW1 (Definitive Edition) Retail Asset Resolution

How Halo Wars: Definitive Edition maps game objects to their art/animation/physics assets.
No `.dep` files exist in retail — dependencies are embedded in the XML data.

## Overview

```
objects.xml.xmb              "give me unsc_veh_warthog_01"
        │
        ├─ Visual ──────────► warthog_01.vis.xmb        (the manifest)
        │                         │
        │                         ├─ model "Default"
        │                         │    ├─ mesh ──────► warthog_damage_01.ugx
        │                         │    ├─ damagefile ► warthog_01.dmg.xmb
        │                         │    ├─ anims ─────► idle_01.uax, walk_01.uax, ...
        │                         │    └─ attachments
        │                         │         ├─ ModelRef "Turret"
        │                         │         ├─ ModelRef "PassengerLegs"
        │                         │         ├─ ModelRef "PassengerTop"
        │                         │         └─ ModelRef "Wheel" (×4, different bones)
        │                         │
        │                         ├─ model "Turret"
        │                         │    ├─ mesh ──────► mg_turret.ugx
        │                         │    └─ attach ────► ModelRef "Turretgun", "Gunner"
        │                         │
        │                         ├─ model "Turretgun"
        │                         │    └─ mesh ──────► mg_gun.ugx
        │                         │
        │                         ├─ model "Gunner"
        │                         │    └─ mesh ──────► warthogmarine_01.ugx
        │                         │
        │                         ├─ model "PassengerLegs" / "PassengerTop"
        │                         │    └─ mesh ──────► warthogSniperLegs_01.ugx / ...
        │                         │
        │                         └─ model "Wheel"
        │                              └─ mesh ──────► wheel_01.ugx
        │
        ├─ Tactics ─────────► unsc_veh_warthog_01.tactics.xmb
        └─ (stats, hardpoints, veterancy, sounds, etc.)
```

## ERA Archive Layout

HW1 distributes files across multiple `.era` archives:

| Archive              | Size   | Contents                                                                |
| -------------------- | ------ | ----------------------------------------------------------------------- |
| `root.era`           | 49 MB  | Metadata: `.vis.xmb`, `.pfx`, `objects.xml.xmb`, tactics, physics, etc. |
| `root_update.era`    | 3.4 MB | Delta patches: `objects_update.xml.xmb`, `squads_update.xml.xmb`, etc.  |
| `scenarioshared.era` | 357 MB | Shared art: `.ugx` (326), `.uax` (1240), `.ddx` (1123), `.dmg.xmb` (58) |
| `release.era`        | 94 MB  | Per-mission: Shield Exterior (207 files — environment, flood, terrain)  |
| `repository.era`     | 61 MB  | Per-mission: Shield Interior (292 files — plants, creatures, terrain)   |
| `PHXscn*.era`        | varies | Per-campaign-mission assets                                             |
| `*.era` (map names)  | varies | Per-multiplayer-map assets                                              |
| `locale_*.era`       | varies | Localized strings and UI assets                                         |

**Key split**: `root.era` has the manifests (`.vis.xmb`) while `scenarioshared.era`
has the actual geometry (`.ugx`), animations (`.uax`), textures (`.ddx`), and damage models (`.dmg.xmb`).

### Archive Load Order (confirmed via IDA — `BArchiveManager::reloadRootArchive` at `0x14017CE90`)

```
1. root.era            → qword_1415005B8  (addFileCache + enableFileCache)
2. root_update.era     → qword_1415005C0  (addFileCache + enableFileCache)
3. locale.era          → loaded by BArchiveManager::reloadLocaleArchive
4. locale_update.era   → loaded after locale.era
5. scenarioshared.era  → loaded during scenario load (beginScenarioLoad)
6. <map>.era           → loaded per-mission (release.era, repository.era, etc.)
```

**`root_update.era` is NOT a file-level override.** The update ERA contains delta
XML files (`objects_update.xml.xmb`, `squads_update.xml.xmb`, `techs_update.xml.xmb`)
that use `update="true"` on their elements. `BDatabase::loadXmlData` loads these
as separate files and merges updated entries into the existing database by matching
object names.

Example from `objects_update.xml.xmb`:

```xml
<Object name="game_CTF_flag_01" is="0" id="2061" dbid="4355" update="true">
    <PhysicsInfo>sentinel</PhysicsInfo>
    <ObjectClass>Squad</ObjectClass>
    <!-- replaces/augments the existing game_CTF_flag_01 definition -->
</Object>
```

The update files also add entirely new objects (e.g., `env_shieldinterior_redriver_01`,
`sys_icon_53_01`) and contain UI asset patches (minimap icons, HUD widgets, flash controls).

### Per-Mission Archives (`release.era`, `repository.era`)

These are **NOT** general-purpose archives. Each corresponds to a specific campaign
mission and contains environment art unique to that map:

- **`release.era`** — "Shield Exterior" mission: flood pods, shield doors, dark rocks,
  terrain tiles, wreckage, plus the scenario definition (`.scn.xmb`, `.sc2.xmb`)
- **`repository.era`** — "Shield Interior" mission: canyon geometry, plants, grass,
  birds, forerunner structures, plus jackal animations for that mission's encounters

Both also contain `pfxfilelist.txt` and `visfilelist.txt` for that mission's preload lists.

## The Two Databases

### 1. `objects.xml.xmb` — Master Object List

In `root.era` at `data\objects.xml.xmb`. ~2600 objects, ~2300 with `<Visual>` paths.

```xml
<Object name="unsc_veh_warthog_01" id="0" dbid="156">
    <Visual>unsc\vehicle\warthog_01\warthog_01.vis</Visual>
    <PhysicsInfo>warthog</PhysicsInfo>
    <Tactics>unsc_veh_warthog_01.tactics</Tactics>
    <Hardpoint name="Turret" yawrate="360" pitchrate="180" .../>
    <Hardpoint name="PassengerTurret" .../>
    <!-- stats, sounds, veterancy, etc. -->
</Object>
```

**No inheritance** — unlike HW2's `parent_element` system, HW1 objects are flat.
Each object is self-contained with all its properties inline.

### 2. `squads.xml.xmb` — Squad Definitions

Also in `root.era` at `data\squads.xml.xmb`:

```xml
<Squad name="unsc_veh_warthog_01" dbid="885">
    <Units>
        <Unit count="1" role="normal">unsc_veh_warthog_01</Unit>
    </Units>
    <Birth anim0="Train" trainerAnim="TrainVehicles" ...>Trained</Birth>
</Squad>
```

### 3. Tactics — `data\tactics\*.tactics.xmb`

In `root.era`. Referenced by base name from `objects.xml` (no path prefix).

### 4. Physics — `physics\*.{physics,blueprint,shp}.xmb`

In `root.era` under the `physics\` directory. `<PhysicsInfo>warthog</PhysicsInfo>`
resolves to three files by appending extensions to the bare name:

| File                            | Purpose                                                                          |
| ------------------------------- | -------------------------------------------------------------------------------- |
| `physics\warthog.physics.xmb`   | Physics config: blueprint ref, center offset, vehicle type, terrain effects path |
| `physics\warthog.blueprint.xmb` | Physical properties: mass (150), friction (2.0), restitution (0.5), shape ref    |
| `physics\warthog.shp.xmb`       | Havok shape definition (box with half-extents 1.5×1.0×3.0)                       |

```xml
<!-- warthog.physics.xmb -->
<physics>
    <blueprint>warthog</blueprint>
    <ThrownByProjectiles>true</ThrownByProjectiles>
    <Vehicle>warthog</Vehicle>
    <CenterOffset>0,2.28,0</CenterOffset>
    <TerrainEffects>effects\terraineffects\warthog_physics</TerrainEffects>
</physics>
```

The shape file (`.shp.xmb`) uses a serialized Havok format (`hke version="V_20200_B_20031014"`)
with `hkBoxShape` or similar primitives. Not `.hkt` files — HW1 uses XML-serialized Havok shapes.

`<PhysicsReplacementInfo>` works the same way (e.g., `aircraft_default` → `physics\aircraft_default.*`).

### 5. LCE (Legendary Collector's Edition) Variants

LCE assets (e.g., `warthoglce_01.vis.xmb`, `mg_gunlce.ugx`) are alternate visual
definitions for units unlocked via the Legendary edition.

The unlock logic is in `sub_140392F90` (a 17-case switch processing unlockable content):

- **Case 16**: `LCE_warthog_unlock` — looks up string hash in `BStringTable`,
  checks unlock state via `sub_140416980`, applies unlock via `sub_1404165C0`
- **Case 17**: `LCE_wraith_unlock` — same pattern

The LCE variants are separate proto objects in `objects.xml` that reference the
`*lce*.vis.xmb` files. When unlocked, the engine swaps the proto object's visual
reference to the LCE variant. The unlock state is persisted via the string table system.

## The `.vis.xmb` File — The Asset Manifest

Located in `root.era` under `art\{faction}\{category}\{unit}\{unit}.vis.xmb`.

### Structure

```xml
<visual defaultmodel="Default">
    <model name="Default">
        <component>
            <!-- Tech-based model swapping (upgrades) -->
            <logic type="Tech">
                <logicdata value="Unsc_warthog_upgrade3" modelref="" weight="1">
                    <asset type="Model">
                        <file>unsc\vehicle\warthog_01\warthog_damage_01</file>
                        <damagefile>unsc\vehicle\warthog_01\warthog_01</damagefile>
                    </asset>
                </logicdata>
                <!-- more upgrade tiers... -->
            </logic>
            <!-- Default (no upgrade) model -->
            <asset type="Model">
                <file>unsc\vehicle\warthog_01\warthog_damage_01</file>
                <damagefile>unsc\vehicle\warthog_01\warthog_01</damagefile>
            </asset>
            <!-- Sub-model attachments -->
            <attach type="ModelRef" name="Turret" tobone="bone_turret_01" .../>
            <attach type="ModelRef" name="Wheel" tobone="bone_wheel_01" .../>
            <attach type="ModelRef" name="Wheel" tobone="bone_wheel_02" .../>
            <!-- ... -->
        </component>
        <anim type="Idle">
            <asset type="Anim">
                <file>unsc\vehicle\warthog_01\idle_01</file>
            </asset>
        </anim>
    </model>

    <!-- Sub-models (matched by name to ModelRef attachments) -->
    <model name="Turret">...</model>
    <model name="Turretgun">...</model>
    <model name="Gunner">...</model>
    <model name="Wheel">...</model>
</visual>
```

### Key Differences from HW2

- **`<logic type="Tech">`** — HW1 uses inline tech-upgrade logic to swap models
  based on upgrade state. Each upgrade tier can specify a different mesh.
- **`<damagefile>` is inline** — appears directly in the `<asset>` block, not as a
  separate element
- **`<uvOffset>`** — HW1 vis files include UV offset data per channel
- **Same-name sub-models** — e.g., "Wheel" appears 4 times as `ModelRef` with
  different `tobone` values, but resolves to a single `<model name="Wheel">` section
- **`defaultmodel="Default"`** — HW1 uses "Default" as the convention; HW2 uses
  the unit name

### Path Resolution (confirmed via IDA — `xgameFinal.exe`)

`<file>` paths in the `.vis.xmb` are **relative, extensionless**:

- `unsc\vehicle\warthog_01\warthog_damage_01` → `art\unsc\vehicle\warthog_01\warthog_damage_01.ugx`
- `unsc\vehicle\warthog_01\idle_01` → `art\unsc\vehicle\warthog_01\idle_01.uax`
- `<damagefile>` path → append `.dmg.xmb`

**The `art\` prefix is hardcoded in the engine.** Verified in `BDatabase::preloadVisFiles`
(at `0x1401F1AA0`), which reads vis names from `preloadVisFileList.txt` and explicitly
calls `BString_Set(…, "art\\", …)` before concatenating the vis file path.

For mesh/animation loading, `BGrannyModel::loadFromFile` (`0x14073D6F0`) constructs the
full path via `sprintf("%s%s", basePath, fileName)` where:

- `basePath` = `"art\\"` (stored at the model's offset 80, set from a global)
- `fileName` = the extensionless path from the `.vis.xmb` `<file>` element (offset 96)

The function then strips any existing extension (via `strrchr(path, '.')`) and tries `.ugx`.

This means:

- The `<Visual>` path in `objects.xml` has **no `art\` prefix** — it's just
  `unsc\vehicle\warthog_01\warthog_01.vis`
- The engine prepends `art\` and appends `.vis.xmb` when looking up the vis file in ERA
- For meshes/anims referenced within the vis, the engine again prepends the stored
  `art\` base path and appends `.ugx` (for `<asset type="Model">`) or `.uax`
  (for `<asset type="Anim">`)

## The `.dmg.xmb` File — Damage Model

Located in `scenarioshared.era` alongside the `.ugx` files.
HW1 damage models use two systems:

### Percentage-Based Damage

Progressive damage at health thresholds:

- `multiframeTextureIndex` — swap damage texture frame
- `attachparticle` — spawn fire/smoke VFX
- `throwpart` — detach geometry with physics

### Impact-Point-Based Damage

Localized damage at specific bones (`bone_impact_01` through `bone_impact_05`):

- `swappart` — swap intact mesh part for damaged version (e.g., `fender2,damaged_fender2`)
- `throwpart` — detach the damaged part with streamer effects

```xml
<damagetemplate>
    <percentagebased>
        <event>
            <action type="multiframeTextureIndex" fromindex="0" toindex="1"/>
        </event>
        <event>
            <action type="throwpart" streamereffect="effects\fire\flaming_debris_02">warthog_02</action>
        </event>
    </percentagebased>
    <impactpointbased>
        <impactpoint name="bone_impact_01" bone="bone_impact_01">
            <event>
                <action type="swappart">fender2,damaged_fender2</action>
            </event>
            <event>
                <action type="throwpart" ...>damaged_fender2</action>
            </event>
        </impactpoint>
    </impactpointbased>
</damagetemplate>
```

## Directory Layout

Assets are split across two archives:

### `root.era` — Manifests & Metadata

```
art\
├── unsc\vehicle\warthog_01\
│   ├── warthog_01.vis.xmb          # visual manifest
│   └── warthoglce_01.vis.xmb       # LCE (legendary) variant
├── covenant\vehicle\...
├── campaign\...
└── cinematic\...
data\
├── objects.xml.xmb                  # master object database
├── squads.xml.xmb                   # squad compositions
└── tactics\
    └── unsc_veh_warthog_01.tactics.xmb
physics\
├── warthog.physics.xmb              # physics config
├── warthog.blueprint.xmb            # mass/friction/shape ref
└── warthog.shp.xmb                  # Havok shape definition
```

### `scenarioshared.era` — Art Assets

```
art\unsc\vehicle\warthog_01\
├── warthog_damage_01.ugx           # main body mesh (295 KB)
├── mg_turret.ugx                   # turret base
├── mg_gun.ugx                      # turret gun barrel
├── warthogmarine_01.ugx            # gunner character
├── warthogSniperLegs_01.ugx        # passenger legs
├── wheel_01.ugx                    # wheel (shared ×4)
├── warthog_gauss_01.ugx            # gauss cannon upgrade
├── warthog_01.dmg.xmb              # damage model
├── idle_01.uax, idle_02.uax        # animations
├── walk_01.uax, walk_03.uax
├── attack_01.uax ... attack_03.uax
├── death_01.uax, death_02.uax
├── train_01.uax
├── spc_warthog_01_df.ddx           # diffuse texture
├── spc_warthog_01_sp.ddx           # specular texture
├── warthog_01_nm.ddx               # normal map
└── wheel_01_df.ddx, wheel_01_nm.ddx, wheel_01_sp.ddx
```

## Resolving "Give Me All Assets for Warthog"

1. **Parse `objects.xml.xmb`** — find `unsc_veh_warthog_01`, extract `<Visual>` path

2. **Find the `.vis.xmb` in `root.era`** — prepend `art\`, append `.xmb`:
   `art\unsc\vehicle\warthog_01\warthog_01.vis.xmb`

3. **Parse the `.vis.xmb`** — walk every `<model>` section:
   - Collect `<file>` from `<asset type="Model">` → prepend `art\`, append `.ugx`
   - Collect `<file>` from `<asset type="Anim">` → prepend `art\`, append `.uax`
   - Collect `<damagefile>` paths → prepend `art\`, append `.dmg.xmb`
   - Also collect from `<logic type="Tech">` blocks (upgrade variants)

4. **Find all collected files in `scenarioshared.era`** (or other ERAs)

5. **Parse `.dmg.xmb`** — extract `throwpart` and `swappart` references
   (these reference mesh parts baked into the `.ugx`, not separate files)

6. **Textures** are referenced inside the `.ugx` material data, not in the XML chain.
   They live alongside the `.ugx` in `scenarioshared.era` with `_df`/`_sp`/`_nm`/`_em` suffixes.

### File Type Summary

| Extension        | Format      | Description                                       |
| ---------------- | ----------- | ------------------------------------------------- |
| `.ugx`           | ECF         | Geometry (vertices, indices, skeleton, materials) |
| `.uax`           | ECF         | Animation clips                                   |
| `.vis.xmb`       | XMB         | Visual manifest (models, anims, attachments)      |
| `.dmg.xmb`       | XMB         | Damage model (destruction sequence)               |
| `.tactics.xmb`   | XMB         | Combat tactics (weapons, abilities)               |
| `.physics.xmb`   | XMB         | Physics config (vehicle type, center offset)      |
| `.blueprint.xmb` | XMB         | Physical properties (mass, friction, shape ref)   |
| `.shp.xmb`       | XMB (Havok) | Collision shape (hkBoxShape, hkConvexShape, etc.) |
| `.ddx`           | DDS variant | Textures (diffuse, specular, normal, emissive)    |
| `.pfx`           | Particle    | Particle effect definitions                       |
| `.tfx`           | Terrain FX  | Terrain effect definitions (dust, tracks)         |
| `.era`           | ERA         | Archive container for all file types              |
