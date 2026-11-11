//! A boot probe for the Bevy + egui stack.
//!
//! This is not the application. It exists to answer one question with a number:
//! can this machine build and start a Bevy window that hosts an `egui` frame in
//! a reasonable time and within its memory? The answer decides whether the plan
//! moves the whole interface to Bevy or keeps `eframe` and paints 3D through a
//! `wgpu` callback.
//!
//! It prints the time from process start to the first painted `egui` frame, and
//! then keeps running so an external timer can measure steady-state frame time.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass};

/// Counts painted frames, and remembers when the first one happened.
#[derive(Resource)]
struct Boot {
    started: std::time::Instant,
    frames: u64,
    first_frame_at: Option<std::time::Duration>,
    missing_context: u64,
}

impl Default for Boot {
    fn default() -> Self {
        Self {
            started: std::time::Instant::now(),
            frames: 0,
            first_frame_at: None,
            missing_context: 0,
        }
    }
}

/// `bevy_egui`'s primary context is attached to a camera, so there has to be one.
fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn probe_ui(mut contexts: EguiContexts, mut boot: ResMut<Boot>) {
    let Ok(ctx) = contexts.ctx_mut() else {
        // The primary egui context lives on a camera. With no camera spawned
        // there is no context to draw into, and the failure is silent unless it
        // is reported: this is the whole point of a probe.
        boot.missing_context += 1;
        if boot.missing_context == 1 {
            eprintln!("canlab-boot: no primary egui context yet (no camera?)");
        }
        return;
    };

    boot.frames += 1;
    let elapsed = boot.started.elapsed();
    if boot.first_frame_at.is_none() {
        boot.first_frame_at = Some(elapsed);
        // stderr on purpose: a redirected stdout is fully buffered, and an
        // external timer may kill this process before the buffer is flushed.
        eprintln!("canlab-boot: first egui frame after {elapsed:?}");
    }

    // egui 0.36 moved the panels onto a `Ui`; a context-level window is the
    // documented way in through `bevy_egui`, and it is enough to prove the pass.
    egui::Window::new("CanLab — boot probe").show(ctx, |ui| {
        ui.heading("CanLab — boot probe");
        ui.label(format!(
            "frame {} at {:.0} ms since start",
            boot.frames,
            elapsed.as_secs_f64() * 1000.0
        ));
        ui.separator();
        ui.label("If this panel is readable, Bevy can host the editor's egui frame.");
    });

    // Ask for the next frame: without a reason to repaint, egui idles and the
    // probe measures one interval instead of a loop.
    ctx.request_repaint();
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(EguiPlugin::default())
        .init_resource::<Boot>()
        .add_systems(Startup, setup_camera)
        .add_systems(EguiPrimaryContextPass, probe_ui)
        .run();
}
