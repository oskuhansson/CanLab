//! Focus behaviour of the math widget.
//!
//! The bug this file exists for: pressing an arrow key while editing moved
//! keyboard focus to the next widget, so the caret walked the expression *and*
//! the palette or a button quietly took the focus. egui decides that in
//! `Focus::begin_pass`, from the raw input, before any widget is drawn — so the
//! only place it can be stopped is a focus-lock filter claimed by the widget.

use canlab_input::MathField;
use canlab_mathview::{MathView, Style};

/// One frame with an optional key press. Returns the id of the math widget.
fn frame(
    ctx: &egui::Context,
    field: &mut MathField,
    events: Vec<egui::Event>,
    take_focus: bool,
) -> egui::Id {
    let style = Style::default();
    let mut id = egui::Id::NULL;
    let mut output = ctx.run_ui(
        egui::RawInput {
            events,
            ..Default::default()
        },
        |ui| {
            let response = MathView::show(ui, field, &style);
            id = response.id;
            if take_focus {
                response.request_focus();
            }
            // A second focusable widget, so focus has somewhere else to go.
            let _ = ui.button("elsewhere");
            let _ = ui.button("and here");
        },
    );
    // A real backend uploads these; epaint panics if they are dropped unhandled.
    output.textures_delta.clear();
    id
}

fn arrow(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

/// Focus the field and let one frame pass, so it counts as "focused last frame".
fn settle(ctx: &egui::Context, field: &mut MathField) -> egui::Id {
    let id = frame(ctx, field, Vec::new(), true);
    frame(ctx, field, Vec::new(), false);
    id
}

#[test]
fn the_field_can_hold_focus() {
    let ctx = egui::Context::default();
    let mut field = MathField::new();
    let id = settle(&ctx, &mut field);
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(id));
}

#[test]
fn arrow_keys_do_not_move_focus_out_of_the_field() {
    let ctx = egui::Context::default();
    let mut field = MathField::new();
    field.type_str("x+1");
    let id = settle(&ctx, &mut field);

    for key in [
        egui::Key::ArrowLeft,
        egui::Key::ArrowRight,
        egui::Key::ArrowUp,
        egui::Key::ArrowDown,
    ] {
        frame(&ctx, &mut field, vec![arrow(key)], false);
        assert_eq!(
            ctx.memory(|memory| memory.focused()),
            Some(id),
            "{key:?} moved the focus away from the math field"
        );
    }
}

#[test]
fn tab_still_moves_focus_away() {
    // The field claims the arrows, not Tab: leaving it with the keyboard has to
    // keep working.
    let ctx = egui::Context::default();
    let mut field = MathField::new();
    let id = settle(&ctx, &mut field);

    frame(&ctx, &mut field, vec![arrow(egui::Key::Tab)], false);
    assert_ne!(
        ctx.memory(|memory| memory.focused()),
        Some(id),
        "Tab should still leave the field"
    );
}

#[test]
fn the_arrow_keys_still_reach_the_caret() {
    // Claiming the keys must not swallow them: the widget reads them itself.
    let ctx = egui::Context::default();
    let mut field = MathField::new();
    field.type_str("x+1");
    let id = settle(&ctx, &mut field);

    let before = field.cursor().offset;
    frame(&ctx, &mut field, vec![arrow(egui::Key::ArrowLeft)], false);
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(id));
    // The widget itself does not move the caret (the caller does), but the event
    // must still be in the input this frame.
    let seen = ctx.input(|input| {
        input
            .events
            .iter()
            .any(|event| matches!(event, egui::Event::Key { key: egui::Key::ArrowLeft, .. }))
    });
    assert!(seen, "the arrow key was swallowed before the caller saw it");
    assert_eq!(field.cursor().offset, before);
}
