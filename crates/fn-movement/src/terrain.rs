//! Sampled ground surface for the movement scene: the real Athena landscape exported by `fn-export terrain`, sampled on
//! the scene's own grid (UE units: centimetres, X/Y horizontal, Z up, `Z = 0` = the old flat baseplate).
//!
//! The scene's only previous floor was the infinite plane `Z = 0`. A [`HeightField`] replaces it wherever the landscape
//! covers a sample; everywhere else the plane stays, so a capsule that walks off the island still finds the baseplate
//! instead of falling forever. Only fully covered cells are sampled (a cell that straddles the coverage edge is the
//! plane), so the surface never mixes the two heights inside one bilinear patch.
use glam::Vec3;

/// A bilinear height grid in scene centimetres (`z_cm[y * width + x]`).
#[derive(Clone, Debug)]
pub struct HeightField {
    pub width: usize,
    pub height: usize,
    /// Sample spacing on X and Y (cm).
    pub quad_cm: [f32; 2],
    /// Scene position of sample `(0, 0)` (cm).
    pub origin_cm: [f32; 2],
    /// Ground Z per sample (cm).
    pub z_cm: Vec<f32>,
    /// 255 where a landscape component provides the sample, 0 where it does not.
    pub covered: Vec<u8>,
}

impl HeightField {
    pub fn is_empty(&self) -> bool {
        self.width < 2 || self.height < 2 || self.z_cm.len() < self.width * self.height
    }
    /// Bilinear ground height (cm) under `(x, y)`, `None` outside the grid or where the cell is not fully covered.
    pub fn height_cm(&self, x: f32, y: f32) -> Option<f32> {
        if self.is_empty() {
            return None;
        }
        let fx = (x - self.origin_cm[0]) / self.quad_cm[0];
        let fy = (y - self.origin_cm[1]) / self.quad_cm[1];
        if fx < 0. || fy < 0. {
            return None;
        }
        let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
        if x0 + 1 >= self.width || y0 + 1 >= self.height {
            return None;
        }
        let at = |x: usize, y: usize| {
            (
                self.z_cm[y * self.width + x],
                self.covered[y * self.width + x] != 0,
            )
        };
        let (za, ca) = at(x0, y0);
        let (zb, cb) = at(x0 + 1, y0);
        let (zc, cc) = at(x0, y0 + 1);
        let (zd, cd) = at(x0 + 1, y0 + 1);
        if !(ca && cb && cc && cd) {
            return None;
        }
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        Some((za * (1. - tx) + zb * tx) * (1. - ty) + (zc * (1. - tx) + zd * tx) * ty)
    }
    /// Surface normal at `(x, y)` from the local height gradient (unit, Z up); `+Z` where the ground is flat or unknown.
    pub fn normal(&self, x: f32, y: f32) -> Vec3 {
        let e = self.quad_cm[0].max(1.);
        let (Some(zx), Some(zy), Some(z)) = (
            self.height_cm(x + e, y),
            self.height_cm(x, y + e),
            self.height_cm(x, y),
        ) else {
            return Vec3::Z;
        };
        Vec3::new((z - zx) / e, (z - zy) / e, 1.).normalize_or_zero()
    }
    /// Grid point `(x, y)` of sample `(i, j)` in scene cm.
    pub fn sample_pos(&self, i: usize, j: usize) -> [f32; 2] {
        [
            self.origin_cm[0] + i as f32 * self.quad_cm[0],
            self.origin_cm[1] + j as f32 * self.quad_cm[1],
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field() -> HeightField {
        // 3x3 samples, 100 cm apart, at x 0..200, y 0..200: z = 10 * i + j (a plane of slope 0.1)
        let (w, h) = (3usize, 3usize);
        let mut z = vec![];
        for j in 0..h {
            for i in 0..w {
                z.push(10. * i as f32 + j as f32);
            }
        }
        HeightField {
            width: w,
            height: h,
            quad_cm: [100., 100.],
            origin_cm: [0., 0.],
            z_cm: z,
            covered: vec![255; w * h],
        }
    }

    #[test]
    fn bilinear_sample_interpolates_the_grid() {
        let f = field();
        assert_eq!(f.height_cm(0., 0.), Some(0.));
        assert_eq!(f.height_cm(100., 100.), Some(11.));
        assert_eq!(f.height_cm(50., 50.), Some(5.5));
        assert_eq!(
            f.normal(50., 50.).z > 0.98,
            true,
            "{:?}",
            f.normal(50., 50.)
        );
    }

    #[test]
    fn outside_the_grid_or_a_partial_cell_there_is_no_height() {
        let mut f = field();
        assert_eq!(f.height_cm(-1., 0.), None);
        assert_eq!(f.height_cm(250., 0.), None);
        f.covered[4] = 0; // the middle sample
        assert_eq!(
            f.height_cm(50., 50.),
            None,
            "a cell that straddles the coverage edge is not sampled"
        );
        assert_eq!(f.height_cm(0., 0.), None);
    }

    #[test]
    fn normal_follows_the_slope() {
        let f = field();
        let n = f.normal(50., 50.);
        // z grows with x and y at 0.1 -> the surface tilts towards -x / -y
        assert!(n.x < 0. && n.y < 0. && n.z > 0.98, "{n:?}");
    }
}
