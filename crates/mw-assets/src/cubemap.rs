use anyhow::{bail, ensure};

use crate::image_data::RgbaImageData;

/// Face order matches wgpu / WebGPU cube faces:
/// `+X, -X, +Y, -Y, +Z, -Z`.
pub const CUBE_FACE_COUNT: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CubemapLayout {
    /// 4×3 horizontal cross (SBS Cloudy Skyboxes, many engine packs):
    /// ```text
    ///       [+Y]
    /// [-X]  [+Z]  [+X]  [-Z]
    ///       [-Y]
    /// ```
    HorizontalCross,
}

/// Six RGBA8 cube faces of equal size.
#[derive(Debug, Clone)]
pub struct CubemapFaces {
    pub size: u32,
    /// Indexed by wgpu cube face order: +X -X +Y -Y +Z -Z.
    pub faces: [Vec<u8>; CUBE_FACE_COUNT],
}

impl CubemapFaces {
    pub fn from_cross(image: &RgbaImageData, layout: CubemapLayout) -> anyhow::Result<Self> {
        match layout {
            CubemapLayout::HorizontalCross => Self::from_horizontal_cross(image),
        }
    }

    fn from_horizontal_cross(image: &RgbaImageData) -> anyhow::Result<Self> {
        ensure!(
            image.width % 4 == 0 && image.height % 3 == 0,
            "horizontal cross expects width%4==0 and height%3==0, got {}x{}",
            image.width,
            image.height
        );

        let face_w = image.width / 4;
        let face_h = image.height / 3;
        ensure!(
            face_w == face_h,
            "cube faces must be square, got {face_w}x{face_h}"
        );
        let size = face_w;

        // (face_index, grid_x, grid_y) in the 4×3 cross.
        // wgpu order: +X -X +Y -Y +Z -Z
        let slots: [(usize, u32, u32); CUBE_FACE_COUNT] = [
            (0, 2, 1), // +X
            (1, 0, 1), // -X
            (2, 1, 0), // +Y
            (3, 1, 2), // -Y
            (4, 1, 1), // +Z
            (5, 3, 1), // -Z
        ];

        let mut faces: [Vec<u8>; CUBE_FACE_COUNT] = std::array::from_fn(|_| Vec::new());
        for (face_index, gx, gy) in slots {
            faces[face_index] = blit_face(image, gx * size, gy * size, size)?;
        }

        Ok(Self { size, faces })
    }
}

fn blit_face(image: &RgbaImageData, origin_x: u32, origin_y: u32, size: u32) -> anyhow::Result<Vec<u8>> {
    let mut out = vec![0u8; (size as usize) * (size as usize) * 4];
    for row in 0..size {
        let src_y = origin_y + row;
        if src_y >= image.height {
            bail!("cubemap face row out of bounds");
        }
        let src_start = ((src_y * image.width + origin_x) * 4) as usize;
        let src_end = src_start + (size as usize) * 4;
        let dst_start = (row as usize) * (size as usize) * 4;
        let dst_end = dst_start + (size as usize) * 4;
        out[dst_start..dst_end].copy_from_slice(&image.rgba[src_start..src_end]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_solid_horizontal_cross() {
        let size = 2u32;
        let width = size * 4;
        let height = size * 3;
        let mut rgba = vec![0u8; (width * height * 4) as usize];

        // Paint each face slot a unique R value.
        let paint = |rgba: &mut [u8], gx: u32, gy: u32, r: u8| {
            for y in 0..size {
                for x in 0..size {
                    let i = (((gy * size + y) * width + (gx * size + x)) * 4) as usize;
                    rgba[i] = r;
                    rgba[i + 3] = 255;
                }
            }
        };
        // +X -X +Y -Y +Z -Z  -> R = 1..6
        paint(&mut rgba, 2, 1, 1);
        paint(&mut rgba, 0, 1, 2);
        paint(&mut rgba, 1, 0, 3);
        paint(&mut rgba, 1, 2, 4);
        paint(&mut rgba, 1, 1, 5);
        paint(&mut rgba, 3, 1, 6);

        let image = RgbaImageData {
            width,
            height,
            rgba,
        };
        let faces = CubemapFaces::from_cross(&image, CubemapLayout::HorizontalCross).unwrap();
        assert_eq!(faces.size, size);
        for (i, expected) in [1u8, 2, 3, 4, 5, 6].into_iter().enumerate() {
            assert_eq!(faces.faces[i][0], expected, "face {i}");
            assert_eq!(faces.faces[i][3], 255);
        }
    }
}
