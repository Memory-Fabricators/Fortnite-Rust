//! The real island terrain from `fn-export terrain` (`terrain/terrain.json`, `heightmap.png`, `coverage.png`), turned into
//! the renderer's mesh and the movement scene's height field.
//!
//! Conventions: `terrain.json` describes a sample grid in UE centimetres (`quad_cm`, `grid_origin`, `origin_z_cm`,
//! `z_scale_cm`: `z = origin_z + (value - 32768) / 128 * z_scale`). The game world is engine metres with Y up and the
//! ground plane at `Y = 0` (props and the pawn both stand on it), so a sample becomes
//! `engine = ((uex - centre_x) / 100, z / 100 - spawn_z, (uey - centre_y) / 100)` — centred on the island so the sandbox
//! spawn lands on it, and shifted vertically so the terrain height at the spawn is exactly the flat plane (the sandbox
//! props around the spawn keep standing on the ground).
use crate::assets::AssetRoots;
use fn_movement::terrain::HeightField;
use std::sync::Arc;

/// Everything the game needs from the terrain export.
pub struct Terrain {
    /// Renderer mesh (engine space, ground-texture UVs).
    pub mesh: fn_render::TerrainMesh,
    /// Movement ground (scene centimetres, `Z = 0` = the flat plane).
    pub field: Arc<HeightField>,
    /// Height of the island centre above the flat plane (m), for logs.
    pub spawn_offset_m: f32,
}

/// Reads the export below the asset roots; `None` when there is none (the flat baseplate stays).
pub fn load(roots: &AssetRoots) -> Option<Terrain> {
    let meta_path = roots.find("terrain.json")?;
    let dir = meta_path.parent()?;
    let meta: serde_json::Value = serde_json::from_slice(&std::fs::read(meta_path).ok()?).ok()?;
    let quad = [
        meta["quad_cm"][0].as_f64()? as f32,
        meta["quad_cm"][1].as_f64()? as f32,
    ];
    let origin = [
        meta["grid_origin"][0].as_f64()? as f32,
        meta["grid_origin"][1].as_f64()? as f32,
    ];
    let (w, h) = (
        meta["width"].as_u64()? as usize,
        meta["height"].as_u64()? as usize,
    );
    let base_z = meta["origin_z_cm"].as_f64()? as f32;
    let z_scale = meta["z_scale_cm"].as_f64()? as f32;
    if w < 2 || h < 2 {
        return None;
    }
    let heights = image::open(dir.join("heightmap.png")).ok()?.to_luma16();
    let coverage = image::open(dir.join("coverage.png")).ok()?.to_luma8();
    if heights.width() as usize != w || heights.height() as usize != h {
        eprintln!(
            "[terrain] heightmap.png is {}x{}, terrain.json says {w}x{h}: ignored",
            heights.width(),
            heights.height()
        );
        return None;
    }
    // UE world centimetres of a sample, minus the grid centre, in engine metres.
    let centre = [
        (origin[0] + (w - 1) as f32 * 0.5) * quad[0],
        (origin[1] + (h - 1) as f32 * 0.5) * quad[1],
    ];
    let covered_at = |i: usize, j: usize| coverage.get_pixel(i as u32, j as u32).0[0] > 127;
    // `z_scale` is already centimetres per height unit (the export resolved UE's 1/128 `LANDSCAPE_ZSCALE` and the landscape's Z scale)
    let height_cm = |i: usize, j: usize| {
        base_z + (heights.get_pixel(i as u32, j as u32).0[0] as f32 - 32768.) * z_scale
    };
    let centre_i = w / 2;
    let centre_j = h / 2;
    let spawn_cm = height_cm(centre_i, centre_j); // the terrain height under the spawn (engine 0, 0)
    let mut mesh = fn_render::TerrainMesh::default();
    let mut index: Vec<u32> = vec![u32::MAX; w * h];
    let mut push_vertex =
        |mesh: &mut fn_render::TerrainMesh, index: &mut Vec<u32>, i: usize, j: usize| -> u32 {
            let k = j * w + i;
            if index[k] == u32::MAX {
                let x = (origin[0] + i as f32 * quad[0]) * 0.01 - centre[0] * 0.01;
                let y = (height_cm(i, j) - spawn_cm) * 0.01;
                let z = (origin[1] + j as f32 * quad[1]) * 0.01 - centre[1] * 0.01;
                mesh.positions.push([x, y, z]);
                mesh.uvs.push([x * 0.5, z * 0.5]);
                index[k] = (mesh.positions.len() - 1) as u32;
            }
            index[k]
        };
    let mut covered_cells = 0usize;
    for j in 0..h - 1 {
        for i in 0..w - 1 {
            if !(covered_at(i, j)
                && covered_at(i + 1, j)
                && covered_at(i, j + 1)
                && covered_at(i + 1, j + 1))
            {
                continue;
            }
            covered_cells += 1;
            let (a, b, c, d) = (
                push_vertex(&mut mesh, &mut index, i, j),
                push_vertex(&mut mesh, &mut index, i + 1, j),
                push_vertex(&mut mesh, &mut index, i + 1, j + 1),
                push_vertex(&mut mesh, &mut index, i, j + 1),
            );
            // same winding as the baseplate's `ground_triangles` quads (i ~ engine +X, j ~ engine +Z)
            mesh.indices.extend_from_slice(&[a, d, c, a, c, b]);
        }
    }
    if mesh.indices.is_empty() {
        eprintln!("[terrain] the export covers no full cells: ignored");
        return None;
    }
    // movement field: the same samples in scene centimetres (the scene's Z = 0 is the flat plane)
    let field = HeightField {
        width: w,
        height: h,
        quad_cm: quad,
        origin_cm: [origin[0] - centre[0], origin[1] - centre[1]],
        z_cm: (0..w * h)
            .map(|k| height_cm(k % w, k / w) - spawn_cm)
            .collect(),
        covered: (0..w * h)
            .map(|k| {
                if coverage.get_pixel((k % w) as u32, (k / w) as u32).0[0] > 127 {
                    255
                } else {
                    0
                }
            })
            .collect(),
    };
    println!(
        "[terrain] {w}x{h} samples, {covered_cells} quads ({} vertices), {:.0} cm/quad, spawn offset {:.1} m",
        mesh.positions.len(),
        quad[0],
        spawn_cm * 0.01
    );
    Some(Terrain {
        mesh,
        field: Arc::new(field),
        spawn_offset_m: spawn_cm * 0.01,
    })
}
