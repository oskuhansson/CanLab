//! Screenshots, for looking at the app without a compositor.
//!
//! `PrintWindow` returns a blank rectangle for a `wgpu` surface, and reading the
//! screen grabs whatever happens to be in front. Neither is a reliable way to
//! check that the interface draws — so this asks the *renderer* for the frame it
//! just produced: `Context::request_screenshot` hands back the pixels egui
//! painted, whatever the window manager is doing with them.
//!
//! It is a debug aid, gated on the environment:
//!
//! ```text
//! CANLAB_SHOT=shot.bmp CANLAB_OPEN="x^2+y^2=1" canlab-gui
//! ```
//!
//! The file is written as an uncompressed BMP, because a BMP is a header and the
//! pixels and needs no dependency. A test harness converts it to PNG afterwards.
//! Nothing is requested when the variable is unset, so a normal run costs one
//! `Option::is_none` per frame.

use std::path::PathBuf;

use eframe::egui;

/// How many frames to paint before the capture, so fonts, layout and the plot
/// have all settled. The first frame lays out the document; the app stops
/// counting at four, so the capture happens on the last of the startup frames.
const SETTLE_FRAMES: u8 = 3;

/// A pending capture.
pub struct Shot {
    path: Option<PathBuf>,
    taken: bool,
}

impl Shot {
    /// Read `CANLAB_SHOT`, if it is set.
    pub fn from_env() -> Shot {
        let path = std::env::var("CANLAB_SHOT")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        if path.is_some() && !cfg!(feature = "glow") {
            // The wgpu surface cannot be read back; eframe only implements the
            // screenshot command for the GL renderer.
            eprintln!(
                "canlab: CANLAB_SHOT needs a `--features glow` build; \
                 the wgpu backend cannot read its framebuffer"
            );
        }
        Shot { path, taken: false }
    }

    /// True while a capture is waiting.
    pub fn pending(&self) -> bool {
        self.path.is_some() && !self.taken
    }

    /// Ask for the frame, once the interface has settled.
    pub fn request(&mut self, ctx: &egui::Context, paints: u8) {
        let Some(path) = self.path.clone() else {
            return;
        };
        if self.taken || paints < SETTLE_FRAMES {
            // egui renders on demand; a capture needs a frame to happen.
            ctx.request_repaint();
            return;
        }
        self.taken = true;
        ctx.request_screenshot(move |image| {
            if let Err(error) = write_bmp(&path, &image) {
                eprintln!("canlab: could not write {}: {error}", path.display());
            } else {
                eprintln!("canlab: wrote {}", path.display());
            }
        });
    }
}

/// Write a [`egui::ColorImage`] as an uncompressed 24-bit BMP.
fn write_bmp(path: &std::path::Path, image: &egui::ColorImage) -> std::io::Result<()> {
    let [width, height] = image.size;
    if width == 0 || height == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "the frame is empty",
        ));
    }
    // BMP rows are padded to four bytes and stored bottom-up.
    let row = (width * 3).div_ceil(4) * 4;
    let pixels = row * height;
    let mut out = Vec::with_capacity(54 + pixels);

    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((54 + pixels) as u32).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());

    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(height as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(pixels as u32).to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());

    for y in (0..height).rev() {
        let start = out.len();
        for x in 0..width {
            let [r, g, b, _] = image.pixels[y * width + x].to_array();
            out.extend_from_slice(&[b, g, r]);
        }
        while out.len() - start < row {
            out.push(0);
        }
    }

    std::fs::write(path, out)
}
