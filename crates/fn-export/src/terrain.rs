//! Terrain stage: the real Athena island (`Athena_Terrain` + `Maps/Landscape/Athena_Terrain_LS_00..05`) -> an
//! assembled height grid the runtime can draw and walk on.
//!
//! UE 4.16 stores a landscape as `ALandscape`/`ALandscapeStreamingProxy` actors (with their root transform) and one
//! `ULandscapeComponent` per `ComponentSizeQuads` x `ComponentSizeQuads` tile. Each component references its heightmap
//! `Texture2D` (`ComponentSizeQuads + 1` square) inside the same package. The components are laid out on a global quad
//! grid: `world_x = (SectionBaseX + root_quad_x) * Scale3D.x`, where `root_quad_x = root_location.x / Scale3D.x`.
//!
//! Output (relative to `--out`):
//! * `terrain/heightmap.png` — one sample per landscape vertex, 16-bit greyscale (the raw biased G16 value), row 0 = the
//!   smallest world Y; vertices no component covers are 0.
//! * `terrain/coverage.png` — 8-bit mask (255 = a landscape component covers that vertex).
//! * `terrain/terrain.json` — `{quad_cm, grid_origin, width, height, origin_z_cm, z_scale_cm, min_z_cm, max_z_cm,
//!   components, packages}`. World position of sample `(i, j)`:
//!   `x = (grid_origin[0] + i) * quad_cm[0]`, `y = (grid_origin[1] + j) * quad_cm[1]`,
//!   `z = origin_z_cm + (value - 32768) / 128 * z_scale_cm`.
use crate::source::Source;
use crate::write_file;
use fn_texture::Texture2D;
use fn_uasset::{ObjectRef, Package as UePackage};
use fn_ueio::Package as TexPackage;
use serde_json::{json, Value as Json};
use std::time::Instant;

/// The level packages that carry landscape components (the persistent level plus the six streamed sections).
const MAPS: &[&str] = &[
    "Game/Athena/Maps/Athena_Terrain",
    "Game/Athena/Maps/Landscape/Athena_Terrain_LS_00",
    "Game/Athena/Maps/Landscape/Athena_Terrain_LS_01",
    "Game/Athena/Maps/Landscape/Athena_Terrain_LS_02",
    "Game/Athena/Maps/Landscape/Athena_Terrain_LS_03",
    "Game/Athena/Maps/Landscape/Athena_Terrain_LS_04",
    "Game/Athena/Maps/Landscape/Athena_Terrain_LS_05",
];

/// `LANDSCAPE_ZSCALE` of UE 4.16: the height sample is divided by 128 to get the local height.
const LANDSCAPE_ZSCALE: f32 = 1.0 / 128.0;

/// The raw `.umap` / `.uexp` / `.ubulk` of one level package.
struct Raw {
    header: Vec<u8>,
    uexp: Vec<u8>,
    ubulk: Vec<u8>,
}

/// Level packages are stored as `.umap` (+ `.uexp` / `.ubulk`), not `.uasset`.
fn load_bytes(src: &Source, map: &str) -> Result<Raw, String> {
    let long = format!("/{map}");
    let read = |ext: &str| {
        src.set
            .read_game(&long, ext)
            .map(|c| c.into_owned())
            .unwrap_or_default()
    };
    let header = src
        .set
        .read_game(&long, "umap")
        .map_err(|e| format!("{long}.umap: {e}"))?
        .into_owned();
    Ok(Raw {
        header,
        uexp: read("uexp"),
        ubulk: read("ubulk"),
    })
}

struct Tile {
    /// Global vertex coordinate of the tile's first sample.
    gx: i64,
    gy: i64,
    /// Width/height in vertices (`ComponentSizeQuads + 1`).
    w: usize,
    h: usize,
    heights: Vec<u16>,
    /// World height in cm of the sample value 32768, and cm per height unit.
    base_z_cm: f32,
    z_cm: f32,
}

fn prop_vec3(props: &fn_uasset::PropertyList, name: &str) -> Option<[f32; 3]> {
    props.get(name)?.as_vec3()
}
fn prop_i64(props: &fn_uasset::PropertyList, name: &str) -> Option<i64> {
    props.get(name)?.as_i64()
}

#[derive(Clone, Copy)]
/// The landscape transform of a level package: the world height of the plane the height samples are measured from
/// (`RelativeLocation.z`) and the landscape scale (cm per quad on X/Y, cm per height unit on Z).
struct Landscape {
    z_cm: f32,
    scale: [f32; 3],
}

/// The landscape actor's root transform in a level package: the scene component with a non-unit `RelativeScale3D`
/// (a proxy's plain `RootComponent0` may belong to an unrelated actor and carry no transform).
fn landscape_root(pkg: &UePackage) -> Result<Option<Landscape>, String> {
    let mut found = None;
    for i in 0..pkg.exports.len() {
        if pkg.export_class_name(i) != "SceneComponent" {
            continue;
        }
        let e = pkg
            .read_export(i)
            .map_err(|err| format!("export {i}: {err}"))?;
        let Some(scale) = prop_vec3(&e.props, "RelativeScale3D") else {
            continue;
        };
        if scale[0] <= 1.0 {
            continue;
        }
        let loc = prop_vec3(&e.props, "RelativeLocation").unwrap_or([0.; 3]);
        found = Some(Landscape {
            z_cm: loc[2],
            scale,
        }); // the landscape root is the last such component in these packages
    }
    Ok(found)
}

/// The component's serialized local Z bounds (`CachedLocalBox.Min/Max.z`), in landscape height units.
fn local_box_z(props: &fn_uasset::PropertyList) -> Option<(f32, f32)> {
    let b = props.get("CachedLocalBox")?.as_struct()?;
    let (a, c) = (b.get("Min")?.as_vec3()?, b.get("Max")?.as_vec3()?);
    Some((a[2], c[2]))
}

/// One landscape tile: its placement and its heightmap samples.
fn read_tile(
    map: &str,
    pkg: &UePackage,
    tex_pkg: &TexPackage,
    comp: usize,
    root: Landscape,
) -> Result<Tile, String> {
    let e = pkg
        .read_export(comp)
        .map_err(|err| format!("{map}: export {comp}: {err}"))?;
    let name = &e.name;
    let Landscape { z_cm, scale } = root;
    let quads = prop_i64(&e.props, "ComponentSizeQuads").unwrap_or(127);
    // absent = default 0 (UE omits default-valued tagged properties)
    let bx = prop_i64(&e.props, "SectionBaseX").unwrap_or(0);
    let by = prop_i64(&e.props, "SectionBaseY").unwrap_or(0);
    let tex_idx = match e
        .props
        .get("HeightmapTexture")
        .map(|v| pkg.resolve(v.as_object().unwrap_or_default()))
    {
        Some(ObjectRef::Export { index, .. }) => index,
        _ => return Err(format!("{map}: {name} has no heightmap in this package")),
    };
    let tex =
        Texture2D::parse_export(tex_pkg, tex_idx).map_err(|err| format!("{map}: {name}: {err}"))?;
    if tex.size_x != (quads + 1) as usize || tex.size_y != (quads + 1) as usize {
        return Err(format!(
            "{map}: {name} heightmap is {}x{}, expected {}x{}",
            tex.size_x,
            tex.size_y,
            quads + 1,
            quads + 1
        ));
    }
    let mip = tex
        .mips
        .first()
        .ok_or_else(|| format!("{map}: {name} has no mips"))?;
    let data = mip.data().ok_or_else(|| {
        format!(
            "{map}: {name} heightmap payload unavailable ({})",
            mip.missing_reason().unwrap_or("?")
        )
    })?;
    let box_z =
        local_box_z(&e.props).ok_or_else(|| format!("{map}: {name} has no CachedLocalBox"))?;
    let heights =
        decode_heights(&tex, data, box_z).map_err(|err| format!("{map}: {name}: {err}"))?;
    // `SectionBase` is the component's coordinate on the landscape's one global quad grid: every level package tiles the
    // same grid, and the actor origins are the same landscape seen from different streamed sections (they are not an
    // additional offset — adding them scatters the sections). The world position follows from the landscape scale alone.
    Ok(Tile {
        gx: bx,
        gy: by,
        w: tex.size_x,
        h: tex.size_y,
        heights,
        base_z_cm: z_cm,
        z_cm: scale[2] * LANDSCAPE_ZSCALE,
    })
}

/// One sample per vertex (`ComponentSizeQuads + 1` square), as the biased 16-bit value UE stores.
///
/// The 1.8 heightmaps cook as `PF_G16` in the persistent level but as a 4-byte format in the streamed sections, where the
/// 16-bit value is split over two of the four bytes. Which bytes is not guessable from the header, so the decoder takes the
/// component's serialized `CachedLocalBox` as the independent truth: a candidate pairing is accepted only when every
/// decoded height, `(value - 32768) / 128`, lies inside that box. (The box's X/Y are quad counts, its Z is local height.)
fn decode_heights(tex: &Texture2D, data: &[u8], box_z: (f32, f32)) -> Result<Vec<u16>, String> {
    let (w, h) = (tex.size_x, tex.size_y);
    let n = w * h;
    let tol = 1.0; // one height unit of slack for the box's rounding
    let fits = |v: &[u16]| {
        let (mut lo, mut hi) = (u16::MAX, 0u16);
        for &x in v {
            lo = lo.min(x);
            hi = hi.max(x);
        }
        let (a, b) = ((lo as f32 - 32768.0) / 128.0, (hi as f32 - 32768.0) / 128.0);
        (a >= box_z.0 - tol) && (b <= box_z.1 + tol) && a < b
    };
    match &tex.format {
        fn_texture::PixelFormat::G16 => {
            if data.len() < n * 2 {
                return Err(format!(
                    "payload is {} bytes, expected {}",
                    data.len(),
                    n * 2
                ));
            }
            let v: Vec<u16> = data[..n * 2]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            if !fits(&v) {
                return Err(format!("G16 heights do not fit CachedLocalBox Z {box_z:?}"));
            }
            Ok(v)
        }
        f => {
            if data.len() < n * 4 {
                return Err(format!(
                    "payload is {} bytes, {f:?} {w}x{h} needs {}",
                    data.len(),
                    n * 4
                ));
            }
            // (low byte, high byte) of the 16-bit sample; the first pairing whose range fits the component's box wins
            for (lo, hi) in [
                (0usize, 2usize),
                (0, 1),
                (2, 3),
                (1, 3),
                (1, 2),
                (0, 3),
                (2, 1),
            ] {
                let v: Vec<u16> = data
                    .chunks_exact(4)
                    .take(n)
                    .map(|c| c[lo] as u16 | (c[hi] as u16) << 8)
                    .collect();
                if fits(&v) {
                    return Ok(v);
                }
            }
            Err(format!(
                "no byte pairing of the {f:?} payload fits CachedLocalBox Z {box_z:?}"
            ))
        }
    }
}

fn write_png16(path: &std::path::Path, w: usize, h: usize, px: &[u16]) -> Result<(), String> {
    use image::ImageEncoder;
    let mut bytes = Vec::with_capacity(px.len() * 2);
    for &v in px {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    image::codecs::png::PngEncoder::new(std::io::BufWriter::new(file))
        .write_image(&bytes, w as u32, h as u32, image::ExtendedColorType::L16)
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn write_png8(path: &std::path::Path, w: usize, h: usize, px: &[u8]) -> Result<(), String> {
    use image::ImageEncoder;
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    image::codecs::png::PngEncoder::new(std::io::BufWriter::new(file))
        .write_image(px, w as u32, h as u32, image::ExtendedColorType::L8)
        .map_err(|e| format!("{}: {e}", path.display()))
}

pub fn run(src: &Source, out: &std::path::Path) -> Result<Json, String> {
    let t0 = Instant::now();
    let mut tiles: Vec<Tile> = Vec::new();
    let mut used_maps: Vec<&str> = Vec::new();
    let mut scale = None;
    for map in MAPS {
        let raw = match load_bytes(src, map) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("terrain: skipping {map}: {e}");
                continue;
            }
        };
        let (pkg, tex_pkg) = match (
            UePackage::parse(&raw.header, &raw.uexp),
            TexPackage::parse(&raw.header, &raw.uexp, &raw.ubulk),
        ) {
            (Ok(a), Ok(b)) => (a, b),
            (a, b) => {
                let e = match (a, b) {
                    (Err(e), _) => e.to_string(),
                    (_, Err(e)) => e.to_string(),
                    _ => unreachable!(),
                };
                eprintln!("terrain: skipping {map}: cannot parse: {e}");
                continue;
            }
        };
        if pkg.exports.len() != tex_pkg.exports.len() {
            eprintln!("terrain: {map}: the two package readers disagree on the export count ({} vs {}), skipped", pkg.exports.len(), tex_pkg.exports.len());
            continue;
        }
        let root = match landscape_root(&pkg)? {
            Some(r) => r,
            None => {
                eprintln!(
                    "terrain: {map} has no landscape actor below a scaled scene component, skipped"
                );
                continue;
            }
        };
        let mut n = 0;
        for i in 0..pkg.exports.len() {
            if pkg.export_class_name(i) != "LandscapeComponent" {
                continue;
            }
            match read_tile(map, &pkg, &tex_pkg, i, root) {
                Ok(t) => {
                    tiles.push(t);
                    n += 1;
                }
                Err(e) => eprintln!("terrain: {e}"),
            }
        }
        println!(
            "terrain: {map}: {n} components ({} cm/quad, base z {} cm)",
            root.scale[0], root.z_cm
        );
        used_maps.push(map);
        scale = Some((root.scale[0] as f64, root.scale[1] as f64));
    }
    if tiles.is_empty() {
        return Err("no landscape components could be read".into());
    }
    let (scale_x, scale_y) = scale.ok_or("no landscape scale")?;
    let gx0 = tiles.iter().map(|t| t.gx).min().unwrap();
    let gy0 = tiles.iter().map(|t| t.gy).min().unwrap();
    let gx1 = tiles.iter().map(|t| t.gx + t.w as i64 - 1).max().unwrap();
    let gy1 = tiles.iter().map(|t| t.gy + t.h as i64 - 1).max().unwrap();
    let (gw, gh) = ((gx1 - gx0 + 1) as usize, (gy1 - gy0 + 1) as usize);
    if gw.checked_mul(gh).map_or(true, |n| n > 64 * 1024 * 1024) {
        return Err(format!("landscape grid {gw}x{gh} is implausibly large"));
    }
    let mut heights = vec![0u16; gw * gh];
    let mut coverage = vec![0u8; gw * gh];
    let (mut min_z, mut max_z) = (f32::INFINITY, f32::NEG_INFINITY);
    let (mut base_z, mut z_cm) = (0f32, 0f32);
    for t in &tiles {
        base_z = t.base_z_cm;
        z_cm = t.z_cm;
        for ty in 0..t.h {
            for tx in 0..t.w {
                let v = t.heights[ty * t.w + tx];
                let gx = (t.gx - gx0) as usize + tx;
                let gy = (t.gy - gy0) as usize + ty;
                heights[gy * gw + gx] = v;
                coverage[gy * gw + gx] = 255;
                let z = t.base_z_cm + (v as f32 - 32768.0) * t.z_cm;
                min_z = min_z.min(z);
                max_z = max_z.max(z);
            }
        }
    }
    let dir = out.join("terrain");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    write_png16(&dir.join("heightmap.png"), gw, gh, &heights)?;
    write_png8(&dir.join("coverage.png"), gw, gh, &coverage)?;
    let meta = json!({
        "quad_cm": [scale_x, scale_y],
        "grid_origin": [gx0, gy0],
        "width": gw,
        "height": gh,
        "origin_z_cm": base_z,
        "z_scale_cm": z_cm,
        "min_z_cm": min_z,
        "max_z_cm": max_z,
        "components": tiles.len(),
        "packages": used_maps,
    });
    write_file(
        &dir.join("terrain.json"),
        (serde_json::to_string_pretty(&meta).unwrap() + "\n").as_bytes(),
    )?;
    println!(
        "terrain: {} components -> {}x{} vertices, X {:.0}..{:.0} m, Y {:.0}..{:.0} m, Z {:.0}..{:.0} m ({:.1}s)",
        tiles.len(),
        gw,
        gh,
        gx0 as f64 * scale_x / 100.,
        gx1 as f64 * scale_x / 100.,
        gy0 as f64 * scale_y / 100.,
        gy1 as f64 * scale_y / 100.,
        min_z / 100.,
        max_z / 100.,
        t0.elapsed().as_secs_f32()
    );
    Ok(meta)
}
