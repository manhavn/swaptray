//! Renders `assets/logo.svg` into StatusNotifierItem icon pixmaps (ARGB32,
//! network byte order) and a window icon. Pure Rust via resvg, no system
//! librsvg or image tools required.

use std::sync::LazyLock;

use ksni::Icon;

const LOGO_SVG: &str = include_str!("../assets/logo.svg");

/// Sizes offered to tray hosts; they pick what fits.
const TRAY_SIZES: [u32; 3] = [32, 64, 128];

static PIXMAPS: LazyLock<Vec<Icon>> = LazyLock::new(|| {
    TRAY_SIZES
        .iter()
        .filter_map(|&size| {
            let (w, h, rgba) = render_rgba(size)?;
            let mut data = Vec::with_capacity(rgba.len());
            for px in rgba.chunks_exact(4) {
                // straight RGBA -> ARGB, network byte order
                data.extend_from_slice(&[px[3], px[0], px[1], px[2]]);
            }
            Some(Icon {
                width: w,
                height: h,
                data,
            })
        })
        .collect()
});

pub fn pixmaps() -> Vec<Icon> {
    PIXMAPS.clone()
}

/// Window icon (straight RGBA) for the egui dialog.
pub fn window_icon(size: u32) -> Option<egui::viewport::IconData> {
    let (w, h, rgba) = render_rgba(size)?;
    Some(egui::viewport::IconData {
        width: w as u32,
        height: h as u32,
        rgba,
    })
}

/// Rasterize the logo at `size` x `size`, returning straight (non-premultiplied)
/// RGBA bytes.
fn render_rgba(size: u32) -> Option<(i32, i32, Vec<u8>)> {
    let options = usvg::Options::default();
    let tree = usvg::Tree::from_str(LOGO_SVG, &options).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(size, size)?;
    let scale = size as f32 / 128.0;
    resvg::render(&tree, usvg::Transform::from_scale(scale, scale), &mut pixmap.as_mut());

    let src = pixmap.data(); // premultiplied RGBA
    let mut out = Vec::with_capacity(src.len());
    for px in src.chunks_exact(4) {
        let [r, g, b, a] = [px[0], px[1], px[2], px[3]];
        let (r, g, b) = if a == 0 {
            (0, 0, 0)
        } else if a == 255 {
            (r, g, b)
        } else {
            (
                ((u32::from(r) * 255) / u32::from(a)).min(255) as u8,
                ((u32::from(g) * 255) / u32::from(a)).min(255) as u8,
                ((u32::from(b) * 255) / u32::from(a)).min(255) as u8,
            )
        };
        out.extend_from_slice(&[r, g, b, a]);
    }
    Some((size as i32, size as i32, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_requested_sizes() {
        for size in TRAY_SIZES {
            let (w, h, rgba) = render_rgba(size).expect("logo should render");
            assert_eq!((w, h), (size as i32, size as i32));
            assert_eq!(rgba.len(), (size * size * 4) as usize);
            // The logo has an opaque background; corners are rounded so at
            // least one pixel must be fully opaque and one transparent.
            assert!(rgba.chunks_exact(4).any(|p| p[3] == 255));
            assert!(rgba.chunks_exact(4).any(|p| p[3] == 0));
        }
    }

}
