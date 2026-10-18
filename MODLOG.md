# Offline Fortnite 1.8 Runtime Journal

## 2026-10-03 - starter firearm grip

- Scope: offline Rust runtime using the user's local Fortnite 1.8 assets; no original game files are modified or redistributed.
- Verified the rifle item's `WeaponActorClass` (`B_Assault_Auto_Athena`) uses `WeaponMesh0` with no relative mesh transform. The held-model mapping therefore remains `SK_MachineGun_01` attached at the serialized player `RightHand` socket.
- Corrected the BR cosmetic-part runtime to use its native `CopyPoseFromMesh` master skeleton path by default. `FN_PART_COPY_MASTER=0` remains a diagnostic override.
- Built `target\\release\\fortnite-rs.exe` and captured the controlled AR pose in `verification\\weapons-master-pose-fixed\\ar-held-1280x720.png`.
- The visible support hand remains detached in that capture even with part animation disabled. The remaining fault is in the player AnimBlueprint hand-IK retarget evaluation, not in the weapon mesh actor transform or cosmetic part layer.
- Regression capture `verification\\pickaxe-hold-latest\\pickaxe-held-1280x720.png` confirms the native pickaxe attachment is intact.
- Tested the serialized ranged `CopyBone weapon_l -> weapon_r` node on held firearms only. It made the AR mesh disappear/collapse into the pawn in `verification\\weapons-firearm-copybone-latest\\ar-held-1280x720.png`; reverted that speculative route. It is not the correct standalone fix without the matching native hand-retarget evaluation.
- Found a second visible failure: `OrigGuns::is_drawing()` only means a translated-material frame was queued; the queued AR currently produces no visible pixels and suppresses the recovered original-mesh fallback.
- The fallback experiment was immediately reverted after real capture `verification\\weapons-mesh-fallback\\ar-held-1280x720.png` showed a full-screen green depth/image corruption. The original-material path remains the stable route while its visibility problem is diagnosed; no speculative fallback is left enabled.
- User clarified that the earlier runtime did present the gun and only the support hand was wrong. The temporary mesh-only and rigid-mesh oracle switches were removed; further work stays on the existing firearm route and the left-hand IK evaluation.
- Verified `verification\\weapons-original\\held-1280x720.png` and `verification\\weapons-current\\held-1280x720.png` show the recovered 1.8 rifle visibly. A later change incorrectly made the translated material pass the default even though its queued draw can be invisible. Default launch now uses the verified recovered-mesh route; `--original-gun-material` is explicitly diagnostic until its pixel output is recovered.
- 2026-10-03: Restored the verified recovered 1.8 gun-mesh route as the default. Capture `verification/weapons-restored-visible-final/ar-held-1280x720.png` confirms the AR is visible again. A firearm-only left-hand FK/IK-alpha test produced no visible grip improvement (`verification/weapons-left-fk/ar-held-1280x720.png`) and was reverted; the remaining support-hand defect is in the HandIK target/space mapping, not the mesh or a safe global animation toggle.
- 2026-10-03 attachment-chain verification (mounted user 1.8 assets; `cargo test --offline -p fn-pose loadout::tests::original_starter_resources -- --exact --nocapture` passed): the serialized player-skeleton `RightHand` socket is the sole weapon attachment: bone `weapon_r`, RelativeLocation `[0,0,0]`, RelativeRotation `[0,-90,0]` UE, RelativeScale `[1,1,1]`. The AR is therefore not attached to `hand_l`. Its support hand must be driven by the recovered firearm player clips (`M_AssaultRifle_NonTargeted` / `M_AssaultRifle_Targeted`) and the AnimBlueprint `HandIKRetargeting` chain (`ik_hand_gun` -> `ik_hand_l`); focus future work there, leaving the verified `RightHand` attachment intact.
- 2026-10-03 follow-up graph audit: the recovered AnimBlueprint state root for `LocomotionLowerBody/Idles` is exactly `M_Melee_TwoHanded` blended with cached `UpperBody`; a temporary direct rifle-pose substitution was removed. The original post-process order confirms `HandIKRetargeting` -> `CopyBone weapon_l -> weapon_r` in component space -> right-hand TwoBoneIK -> left-hand TwoBoneIK. The renderer currently uses a single final-skeleton pose for both the player and rigid weapon attachment, so applying that post-process CopyBone to its `RightHand` attachment frame turns the AR sideways. `apply_copy_bone` is intentionally disabled until attachment and final-skeleton poses are carried separately. The serialized node/order and disabled render-path invariant are covered by `graph::tests::weapon_hand_chain_matches_the_serialized_1_8_post_process_graph`; release capture `verification/weapons-precopy-ar-release/ar-held-1280x720.png` exits 0 with the forward attachment frame.

## 2026-10-03 - weapon source attachment and inventory audit

- Added a distinct `WeaponAim` socket from the serialized `weapon_l` source bone while retaining the serialized `RightHand` (`weapon_r`) socket for the final character pose. The runtime now runs the recovered `weapon_l -> weapon_r` CopyBone/hand-IK chain for the pawn, but attaches the weapon mesh to `WeaponAim`; this is intended to preserve the source-bone mesh basis while inheriting the authored aim-pitch motion. It is a renderer architecture correction, not a claim that the visual grip has been captured and approved yet.
- Routed aimed firing through the recovered `playerFireADS` and `weaponFireADS` montage bindings. The current starter definitions use the same named assets for hip and ADS fire, but the runtime now honors the distinct original bindings whenever they differ.
- Verification: `cargo test --offline -p fn-app arms::tests -- --nocapture` passed 23 starter-arms tests, covering idle/equip/fire/ADS montage selection, reload timing, pump reload loop/interruption, healing, and quickbar movement. `cargo test --offline -p fn-ui screens::inventory_panel::tests -- --nocapture` passed 10 inventory-panel tests, covering original layout, selection, drag threshold, slot drop, split/drop intent, scrolling, and rendering.
- Inventory audit: the visible panel uses original Athena catalog data, item definitions, inventory panel widget geometry, quickbar mapping, item icons and resource stacks. Its `DropItemOnQuickBar` path updates the live quickbar/HUD. The original native `FortInventoryContext` bodies for `RemoveItemFromQuickBar`, `EquipItem`, and world `DropItem` are not in the recovered client data, so they deliberately remain reported rather than fabricated; this standalone has no pickup actors or separate backpack storage.

## 2026-10-03 - original weapon attachment audit

- Verified from the mounted 1.8 BR weapon mesh packages that the starter AR
  (`SK_MachineGun_01`), pump (`SK_Crusader`), and sniper (`Sniper_01`) each
  have a `Handle` bone plus four mesh sockets: `Muzzle`, `MuzzleFalloff`,
  `FX_Idle_Start`, and `FX_Idle_End`.
- Those sockets are authored effect endpoints, not a pawn grip/attach socket.
  The weapon component therefore remains attached through the player mesh's
  serialized `RightHand` socket; its own `Handle` bone is animated internally.
- The renderer now samples two recovered player poses: the final
  CopyBone/hand-IK pose for the visible pawn and the pre-CopyBone `RightHand`
  pose for the weapon attachment. This keeps the gun basis from rotating
  sideways; support-hand fidelity remains under investigation.
- Tested and rejected a reversed `HandFKWeight` blend interpretation. The
  mounted 1.8 `vs_old_logs` pose oracle failed all six checks and showed the
  recorded right-hand FK/IK gap moved to the left hand (up to 0.374 m error).
  The established right-to-left retarget ordering was restored.
- Preserved the original sniper scope `Reticle` texture's black mask when it
  is decoded for the overlay. The previous loader made near-black pixels
  transparent, which precluded the original scope blackout.
- Reconciled the scoped camera timing with its recovered camera-mode
  `TransitionTime` (0.25 s). `TransitionParams.BlendTime` is retained as the
  ease-curve parameter rather than being incorrectly used as the camera
  duration. The full real-data sniper ADS test now passes.
- Built the release executable and captured the resulting scoped view at
  `verification/sniper-scope-timing-fixed/sniper-scope-1280x720.png`; the
  script report is `RESULT: ok`.

## 2026-10-03 - final-pose weapon attachment correction

- Static package/disassembly audit: the player `RightHand` socket is on
  `weapon_r` with zero translation and UE rotation `(0,-90,0)`. The three
  starter weapon actors use `WeaponMesh0` as their root component, and all
  three weapon AnimBPs are `LocalRefPose -> DefaultSlot -> Root`.
- Replaced the old renderer-only pre-CopyBone attachment pose. Guns, the
  pickaxe, and `weapon_origin` now all take the same final-pose `RightHand`
  socket after HandIKRetargeting, curve-gated CopyBone, and both hand IK nodes.
- Added decoded float-curve sampling to `FnClip` and weighted curve evaluation
  through the active pose tree. `SnapWeapon_LHand`, `BothHandFK`, and
  `LeftHandFK` now drive CopyBone and the two bytecode-derived hand IK formulas.
- Read-only scan result in `verification/snap-weapon-lhand-curves.txt`: all
  1,455 readable installed `AnimSequence` packages were scanned; none contains
  a serialized `SnapWeapon_LHand` curve in this local 1.8 install. The runtime
  consequently receives alpha zero rather than forcing CopyBone to one.
- The installed data exposes weapon `Handle` bones and muzzle/effect sockets,
  but no serialized `Grip` socket/marker. A hand-to-gun-grip distance cannot be
  truthfully defined from the packages without inventing a mesh point; this is
  recorded rather than tuned by eye.

## 2026-10-17 - pak-only start-up: manifest, Metal, UI fonts, trackpad look, real terrain

- Added `fn-manifest` (`crates/fn-manifest`): regenerates `tools/asset-manifest.json` from a Fortnite install, driven by
  the runtime's own lists — the UI texture table (`crates/fn-ui/data/ui-texture-map.tsv`, 202 entries), the sky pass'
  textures (`fn_render::sky::TEXTURES`, exported under `sky/`), the character packages of `fn_pose::loader` and
  `fn_legacy::prototype_clip_names()` (plus `M_Melee_TwoHanded`). Animation names resolve to packages the same way the
  runtime's clip loader does (`fortnitegame/content/**/<name>.uasset`, first hit in glob order); ambiguous names are
  reported. Generated against the 1.8 install: 202 textures, 4 meshes, 197 clips.
- Verified the manifest end to end: `fn-export --md5 all` with it exports 202/202 textures, 14/14 meshes (incl. the sky
  meshes) and 197/197 clips with 0 failures (~6 s), and `fortnite-rs --paks <Paks>` now reaches the world with
  `RESULT: ok` on the start-up capture.
- The app now finds the manifest at `tools/asset-manifest.json` (cwd) and `fn-export` in the sibling cargo profile
  directories (`FN_EXPORT` overrides); previously both had to sit beside the executable.
- `fn-render::gpu`: the default backend set is platform-aware (Metal on macOS, Vulkan + DX12 elsewhere), so enabling the
  `metal` wgpu feature is enough (`FN_BACKEND` = `vulkan|dx12|metal|gles` still forces one).
- The UI asset source layers the pak-backed `fn_ui::pak_assets::PakAssets` when the game runs from `--paks`: fonts
  (`NotoSans-*.ufont`) only exist in the paks, so the UI previously failed with "no usable UI font".
- `platform::recenter`: the hidden pointer is only warped when the OS can report its position (Windows). winit's macOS
  `set_cursor_position` re-associates the mouse with the cursor; the per-frame warp fought the trackpad and the view
  snapped back after every movement. Off Windows the `Locked` grab already pins the pointer.
- `fn-export terrain` (new stage): reads `Athena_Terrain` + `Maps/Landscape/Athena_Terrain_LS_00..05`, decodes each
  `LandscapeComponent`'s heightmap and stitches the global height grid (`terrain/heightmap.png`, `coverage.png`,
  `terrain.json`). 94 components, one global 1525 x 1525 grid (3060 m at 200.7874 cm/quad), heights -141..111 m.
  Tile placement: `SectionBase` is already the global quad coordinate (adding the actor origins scatters the sections —
  every level package tiles the same grid). The cooked streamed-section heightmaps are 4-byte textures, not `PF_G16`;
  the 16-bit sample's byte pair is chosen per component by fitting the decoded heights inside that component's serialized
  `CachedLocalBox` (all 94 components pass), never guessed. The stitched island silhouette matches the shipped
  `MiniMapAthena` texture.
- Wired the terrain into the game. `fn-movement::terrain::HeightField` samples it bilinearly in the movement scene's own
  units, and `Scene::probe_dir`/`Scene::line_down` use it in place of the baseplate plane wherever it covers the capsule
  (uncovered cells and partial cells stay the plane, so a capsule that walks off the island still finds a floor and the
  two surfaces never mix inside one bilinear patch); the slope normal matters for walkability, so it comes from the
  height gradient. `Controller.terrain` hands the field to the collision scene on every step (a flat `Scene::default()`
  stays the default, so the existing movement tests are untouched).
- `fn-render` draws the island: `RendererAssets::terrain` is a `TerrainMesh` (vertex + 32-bit index buffer, drawn with
  the ground pipeline, whose normals come from screen-space derivatives) and replaces the flat plate and its ring.
- `fn-app` loads `terrain/{terrain.json,heightmap.png,coverage.png}` from the asset roots, centres the island and sets its
  height under the spawn to the baseplate height. The renderer and `GameSim` use the same height field.
- Fixed camera clipping below the island: `GameSim::camera_world` now sphere-traces the sampled terrain and uses the flat
  baseplate only where the landscape has no covered cell. Before this, the controller stayed grounded but the camera boom
  collided against `Y = 0` instead of the lower terrain, snapping the view beneath the island.
- On `--paks`, missing optional world assets are exported into `assets-cache/props`: the placed tree/rock catalog and the
  map's baked `S_WavyGrass_01` instances. Existing `fn-app` loaders render nearby real props and cull the grass spatially.
  This is vegetation, not buildings: POI/static-mesh actors in the streamed map levels still need a world-geometry exporter
  and placement renderer.
- Verified: the terrain exporter reads all 94 components into 1525x1525 samples (1.5 M covered quads, -141..111 m); the
  1.8 prop export writes 85/85 kinds, 261 textures and 72 materials; foliage export records 555,419 instances. A real-props
  startup capture draws 308 nearby props and 35,141 visible grass instances (`RESULT: ok`). The 10-second walk smoke ends
  grounded (`air 0`); the terrain-camera regression tests pass, including a below-baseplate valley and a rising slope.
