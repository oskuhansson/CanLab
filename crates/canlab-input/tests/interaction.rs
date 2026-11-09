//! Behavioural tests for the structural input model.
//!
//! Each test is named after the keystrokes or palette click it reproduces, and the
//! expected values are the ones measured in the live Abitti editor (see
//! `docs/mathquill-interaction.md`, which tags every claim [OBSERVED] / [SOURCE] /
//! [INFERRED]).

use canlab_input::{EscapeOutcome, MathField, Slot, Template, ToExprError};

/// A field with the given keys typed into it.
fn typed(keys: &str) -> MathField {
    let mut f = MathField::new();
    f.type_str(keys);
    f
}

/// The slot path the cursor is in, e.g. `[Num]` for "inside the numerator".
fn slot_path(f: &MathField) -> Vec<Slot> {
    f.cursor().path.iter().map(|s| s.slot).collect()
}

/// Move left until the cursor sits at the start of the given slot.
fn goto_slot_start(f: &mut MathField, slot: Slot) {
    for _ in 0..64 {
        if f.cursor().slot() == Some(slot) && f.cursor().offset == 0 {
            return;
        }
        if !f.move_left() {
            return;
        }
    }
    panic!("never reached the start of {slot:?}");
}

// ---------------------------------------------------------------- plain typing

#[test]
fn typing_builds_a_plain_run_with_no_structure() {
    let f = typed("2x+1");
    assert_eq!(f.to_latex(), "2x+1");
    // Every operator is its own node; word characters run together.
    assert_eq!(f.root().len(), 3);
    assert!(f.cursor().path.is_empty());
    assert_eq!(f.cursor().offset, 3);
}

#[test]
fn space_is_a_latex_space() {
    let f = typed("pi ");
    assert_eq!(f.to_latex(), "pi\\ ");
}

// ------------------------------------------------------------------- fractions

#[test]
fn x_slash_2_puts_the_cursor_in_the_denominator() {
    let f = typed("x/2");
    assert_eq!(f.to_latex(), "\\frac{x}{2}");
    assert_eq!(slot_path(&f), vec![Slot::Den]);
    assert_eq!(f.cursor().offset, 1);
}

#[test]
fn division_on_an_empty_field_leaves_both_slots_empty() {
    let f = typed("/");
    assert_eq!(f.to_latex(), "\\frac{ }{ }");
    assert_eq!(slot_path(&f), vec![Slot::Num]);
    assert_eq!(f.cursor().offset, 0);
    assert!(f.is_incomplete());
}

#[test]
fn one_half_x_is_one_over_two_x() {
    // The acid test: the denominator keeps absorbing what you type.
    let f = typed("1/2x");
    assert_eq!(f.to_latex(), "\\frac{1}{2x}");
}

#[test]
fn one_half_x_plus_one_stays_in_the_denominator() {
    let f = typed("1/2x+1");
    assert_eq!(f.to_latex(), "\\frac{1}{2x+1}");
}

#[test]
fn slash_takes_the_whole_word_run() {
    assert_eq!(typed("xy/").to_latex(), "\\frac{xy}{ }");
    assert_eq!(typed("12/").to_latex(), "\\frac{12}{ }");
}

#[test]
fn slash_stops_at_an_operator() {
    assert_eq!(typed("1+2/").to_latex(), "1+\\frac{2}{ }");
}

#[test]
fn slash_inside_a_denominator_nests() {
    let f = typed("1/2/");
    assert_eq!(f.to_latex(), "\\frac{1}{\\frac{2}{ }}");
    assert_eq!(slot_path(&f), vec![Slot::Den, Slot::Den]);
}

#[test]
fn slash_on_a_selection_wraps_it() {
    let mut f = typed("x+1");
    f.select_all();
    f.slash();
    assert_eq!(f.to_latex(), "\\frac{x+1}{ }");
    assert_eq!(slot_path(&f), vec![Slot::Den]);
}

// ------------------------------------------------------------------ superscripts

#[test]
fn x_caret_2_is_x_squared() {
    let f = typed("x^2");
    assert_eq!(f.to_latex(), "x^2");
    assert_eq!(slot_path(&f), vec![Slot::Sup]);
    assert!(f.to_expr().is_some());
}

#[test]
fn a_superscript_absorbs_following_characters() {
    assert_eq!(typed("x^2y").to_latex(), "x^{2y}");
}

#[test]
fn x_sub_1_sup_2_subscripts_the_one() {
    // After `_` the cursor is inside the subscript, so `^` decorates the `1`.
    let f = typed("x_1^2");
    assert_eq!(f.to_latex(), "x_{1^2}");
}

#[test]
fn power_on_an_empty_field_makes_a_baseless_superscript() {
    let f = typed("^");
    assert_eq!(f.to_latex(), "^{ }");
    assert_eq!(slot_path(&f), vec![Slot::Sup]);
    assert_eq!(f.to_expr(), None);
}

#[test]
fn a_second_caret_after_an_existing_superscript_re_enters_it() {
    // [INFERRED]: MathQuill was not observed pressing `^` twice.
    let mut f = typed("x^2");
    while f.move_right() {}
    assert!(f.cursor().path.is_empty());
    f.power();
    assert_eq!(f.to_latex(), "x^2");
    assert_eq!(slot_path(&f), vec![Slot::Sup]);
}

#[test]
fn caret_over_a_selection_makes_a_baseless_superscript_of_it() {
    let mut f = typed("x+1");
    f.select_all();
    f.power();
    assert_eq!(f.to_latex(), "^{x+1}");
    // Nothing is left empty, so the cursor sits at the end of the superscript.
    assert_eq!(slot_path(&f), vec![Slot::Sup]);
    assert_eq!(f.cursor().offset, 3);
}

// ----------------------------------------------------------------- navigation

#[test]
fn left_right_walk_the_flattened_position_list() {
    // Measured on the live editor: root → num[0] → num[1] → den[0] → den[1] → root.
    let mut f = typed("1/2");
    let order = |f: &MathField| (slot_path(f), f.cursor().offset);

    assert_eq!(order(&f), (vec![Slot::Den], 1));
    assert!(f.move_left());
    assert_eq!(order(&f), (vec![Slot::Den], 0));
    assert!(f.move_left());
    assert_eq!(order(&f), (vec![Slot::Num], 1));
    assert!(f.move_left());
    assert_eq!(order(&f), (vec![Slot::Num], 0));
    assert!(f.move_left());
    assert_eq!(order(&f), (vec![], 0));
    assert!(f.move_right());
    assert_eq!(order(&f), (vec![Slot::Num], 0));
}

#[test]
fn leaving_a_fraction_is_just_walking_past_it() {
    let mut f = typed("1/2");
    for _ in 0..5 {
        f.move_right();
    }
    assert!(f.cursor().path.is_empty());
    assert_eq!(f.cursor().offset, 1);
    // One more does nothing: the root has a single child.
    assert!(!f.move_right());
}

#[test]
fn there_is_no_position_between_an_atom_and_its_superscript() {
    let mut f = typed("x^2");
    assert_eq!(slot_path(&f), vec![Slot::Sup]);
    assert!(f.move_left());
    assert_eq!(slot_path(&f), vec![Slot::Sup]);
    assert_eq!(f.cursor().offset, 0);
    // Left from the start of the superscript skips the base and exits.
    assert!(f.move_left());
    assert!(f.cursor().path.is_empty());
    assert_eq!(f.cursor().offset, 0);
}

#[test]
fn up_and_down_swap_numerator_and_denominator() {
    let mut f = typed("1/2");
    assert_eq!(slot_path(&f), vec![Slot::Den]);
    assert!(f.move_up());
    assert_eq!(slot_path(&f), vec![Slot::Num]);
    // Nothing above the numerator.
    assert!(!f.move_up());
    assert!(f.move_down());
    assert_eq!(slot_path(&f), vec![Slot::Den]);
}

#[test]
fn vertical_movement_clamps_because_there_is_no_layout() {
    // [OBSERVED] that `↑`/`↓` are geometric; [INFERRED] the clamp. See
    // `MathField::move_up`.
    let mut f = typed("123456/7");
    goto_slot_start(&mut f, Slot::Den);
    assert!(f.move_up());
    assert_eq!(slot_path(&f), vec![Slot::Num]);
    assert_eq!(f.cursor().offset, 0);
}

// ------------------------------------------------------------------- backspace

#[test]
fn backspace_in_the_denominator_empties_it() {
    let mut f = typed("1/2");
    f.backspace();
    assert_eq!(f.to_latex(), "\\frac{1}{ }");
    assert_eq!(slot_path(&f), vec![Slot::Den]);
    assert!(f.is_incomplete());
}

#[test]
fn backspace_at_the_start_of_an_empty_denominator_unwraps_the_fraction() {
    let mut f = typed("1/2");
    f.backspace(); // `2` gone, denominator empty
    f.backspace(); // fraction unwrapped
    assert_eq!(f.to_latex(), "1");
    assert!(f.cursor().path.is_empty());
    assert_eq!(f.cursor().offset, 1);
}

#[test]
fn backspace_at_a_denominator_start_merges_and_keeps_the_junction() {
    let mut f = typed("x+1/2");
    // Cursor is in the denominator after `2`; step back to its start.
    f.move_left();
    assert_eq!(f.cursor().offset, 0);
    f.backspace();
    assert_eq!(f.to_latex(), "x+12");
    // The logical position is preserved: between the two pieces of content,
    // i.e. after `x`, `+` and the numerator's `1`.
    assert!(f.cursor().path.is_empty());
    assert_eq!(f.cursor().offset, 3);
}

#[test]
fn backspace_at_a_numerator_start_unwraps_to_the_left() {
    let mut f = typed("1/2");
    goto_slot_start(&mut f, Slot::Num);
    f.backspace();
    assert_eq!(f.to_latex(), "12");
    assert_eq!(f.cursor().offset, 0);
}

#[test]
fn backspace_at_a_root_body_start_unwraps_the_root() {
    let mut f = MathField::new();
    f.insert_template(Template::Sqrt).unwrap();
    f.type_str("y");
    f.move_left();
    assert_eq!(f.cursor().offset, 0);
    f.backspace();
    assert_eq!(f.to_latex(), "y");
    assert_eq!(f.cursor().offset, 0);
}

#[test]
fn backspace_at_a_bracket_body_start_unwraps_the_brackets() {
    let mut f = typed("(x)");
    f.move_left(); // into the body, at its end
    f.move_left(); // start of the body
    f.backspace();
    assert_eq!(f.to_latex(), "x");
}

#[test]
fn backspace_in_an_empty_superscript_removes_the_superscript() {
    let mut f = typed("x^2");
    f.backspace(); // delete `2`
    assert_eq!(f.to_latex(), "x^{ }");
    f.backspace(); // delete the empty superscript
    assert_eq!(f.to_latex(), "x");
    assert!(f.cursor().path.is_empty());
}

#[test]
fn backspace_at_a_non_empty_superscript_start_deletes_the_base() {
    let mut f = typed("x^2");
    f.move_left();
    assert_eq!(slot_path(&f), vec![Slot::Sup]);
    assert_eq!(f.cursor().offset, 0);
    f.backspace();
    assert_eq!(f.to_latex(), "^2");
}

#[test]
fn backspace_with_a_selection_deletes_it() {
    let mut f = typed("1/2");
    f.select_all();
    f.backspace();
    assert_eq!(f.to_latex(), "");
    assert!(f.is_blank());
}

#[test]
fn typing_over_a_selection_replaces_it() {
    let mut f = typed("1/2");
    f.select_all();
    f.insert_char('9');
    assert_eq!(f.to_latex(), "9");
}

// --------------------------------------------------------------- forward delete

#[test]
fn delete_forward_removes_one_character() {
    let mut f = typed("123");
    while f.move_left() {}
    f.delete_forward();
    assert_eq!(f.to_latex(), "23");
}

#[test]
fn delete_forward_at_a_numerator_start_empties_the_slot() {
    let mut f = typed("1/2");
    goto_slot_start(&mut f, Slot::Num);
    f.delete_forward();
    assert_eq!(f.to_latex(), "\\frac{ }{2}");
    assert_eq!(slot_path(&f), vec![Slot::Num]);
}

#[test]
fn delete_forward_at_the_field_start_removes_a_whole_node() {
    let mut f = typed("1/2");
    f.select_all();
    f.clear_selection();
    while f.move_left() {}
    f.delete_forward();
    assert_eq!(f.to_latex(), "");
}

// -------------------------------------------------------------------- palettes

#[test]
fn palette_frac_lands_the_cursor_in_the_numerator() {
    let mut f = MathField::new();
    f.insert_template(Template::Frac).unwrap();
    assert_eq!(f.to_latex(), "\\frac{ }{ }");
    assert_eq!(slot_path(&f), vec![Slot::Num]);
}

#[test]
fn palette_frac_on_a_selection_behaves_like_slash() {
    let mut f = typed("x+1");
    f.select_all();
    f.insert_template(Template::Frac).unwrap();
    assert_eq!(f.to_latex(), "\\frac{x+1}{ }");
    assert_eq!(slot_path(&f), vec![Slot::Den]);
}

#[test]
fn palette_sqrt_lands_the_cursor_in_the_body() {
    let mut f = MathField::new();
    f.insert_template(Template::Sqrt).unwrap();
    assert_eq!(f.to_latex(), "\\sqrt{ }");
    assert_eq!(slot_path(&f), vec![Slot::Body]);
    f.type_str("y");
    assert_eq!(f.to_latex(), "\\sqrt{y}");
}

#[test]
fn palette_nthroot_lands_the_cursor_in_the_index() {
    let mut f = MathField::new();
    f.insert_template(Template::NthRoot).unwrap();
    assert_eq!(f.to_latex(), "\\sqrt[]{ }");
    assert_eq!(slot_path(&f), vec![Slot::Index]);
    f.type_str("y");
    assert_eq!(f.to_latex(), "\\sqrt[y]{ }");
}

#[test]
fn palette_int_lands_the_cursor_in_the_lower_limit() {
    let mut f = MathField::new();
    f.insert_template(Template::Int).unwrap();
    assert_eq!(f.to_latex(), "\\int_{ }^{ }");
    assert_eq!(slot_path(&f), vec![Slot::Sub]);
}

#[test]
fn palette_sin_inserts_a_bare_operator_with_no_argument_slot() {
    let mut f = MathField::new();
    f.insert_template(Template::Sin).unwrap();
    assert_eq!(f.to_latex(), "\\sin");
    assert!(f.cursor().path.is_empty());
    assert_eq!(f.cursor().offset, 1);
}

#[test]
fn palette_paren_lands_the_cursor_inside() {
    let mut f = MathField::new();
    f.insert_template(Template::Paren).unwrap();
    assert_eq!(f.to_latex(), "\\left(\\right)");
    assert_eq!(slot_path(&f), vec![Slot::Body]);
}

#[test]
fn every_palette_action_is_classified() {
    for action in [
        "\\frac", "\\binom", "\\sqrt", "\\nthroot", "^", "_", "(", "[", "|", "\\overrightarrow",
        "\\overline", "\\underrightarrow", "\\int", "\\sum", "\\lim_", "\\sin", "\\cos", "\\tan",
    ] {
        assert!(
            Template::from_palette_action(action).is_ok(),
            "{action} should map to a template"
        );
    }
    for action in ["\\cases", "\\matrix", "\\array", "\\intsub", "]x]", "\\mathrm"] {
        assert!(
            Template::from_palette_action(action).is_err(),
            "{action} is deliberately not modelled"
        );
    }
}

// ------------------------------------------------------------ command entry

#[test]
fn backslash_alpha_commits_on_a_non_letter_and_swallows_the_space() {
    let f = typed("\\alpha ");
    assert_eq!(f.to_latex(), "\\alpha");
}

#[test]
fn a_digit_terminates_a_command_and_is_inserted_normally() {
    assert_eq!(typed("\\alpha2").to_latex(), "\\alpha2");
    assert_eq!(typed("\\sqrt2").to_latex(), "\\sqrt{2}");
}

#[test]
fn an_unterminated_invalid_command_stays_raw() {
    assert_eq!(typed("\\alphax").to_latex(), "\\alphax");
}

#[test]
fn an_invalid_command_terminated_by_space_becomes_upright_text() {
    assert_eq!(typed("\\alp ").to_latex(), "\\text{alp}");
}

#[test]
fn backspace_edits_a_pending_command() {
    let mut f = typed("\\alp");
    assert_eq!(f.pending_command(), Some("alp"));
    f.backspace();
    assert_eq!(f.pending_command(), Some("al"));
    assert_eq!(f.to_latex(), "\\al");
}

#[test]
fn sqrt_needs_a_non_letter_terminator() {
    // `\sqrt` then space commits the root, so `x` becomes its body.
    assert_eq!(typed("\\sqrt x").to_latex(), "\\sqrt{x}");
}

// ------------------------------------------------------ automatic conversions

#[test]
fn bare_operator_names_become_commands() {
    assert_eq!(typed("sin").to_latex(), "\\sin");
    assert_eq!(typed("sinx").to_latex(), "\\sin x");
    assert_eq!(typed("sin2").to_latex(), "\\sin2");
    assert_eq!(typed("xsin").to_latex(), "x\\sin");
    assert_eq!(typed("arcsin").to_latex(), "\\arcsin");
}

#[test]
fn pi_and_sqrt_are_not_commands_without_a_backslash() {
    assert_eq!(typed("pi").to_latex(), "pi");
    assert_eq!(typed("sqrt").to_latex(), "sqrt");
}

#[test]
fn star_is_a_centered_dot() {
    assert_eq!(typed("2*3").to_latex(), "2\\cdot3");
}

#[test]
fn less_equal_and_greater_equal_collapse() {
    assert_eq!(typed("2<=3").to_latex(), "2\\le3");
    // A space appears only because the next character is a letter.
    assert_eq!(typed("x<=y").to_latex(), "x\\le y");
    assert_eq!(typed("x>=y").to_latex(), "x\\ge y");
}

#[test]
fn other_shortcuts_are_not_shortcuts() {
    assert_eq!(typed("x+-1").to_latex(), "x+-1");
    assert_eq!(typed("a!=b").to_latex(), "a!=b");
}

#[test]
fn comma_is_delimited() {
    assert_eq!(typed("2,3").to_latex(), "2{,}3");
}

// ---------------------------------------------------------------- brackets

#[test]
fn parentheses_open_a_left_right_group() {
    let f = typed("(x+1)^2");
    assert_eq!(f.to_latex(), "\\left(x+1\\right)^2");
}

#[test]
fn a_closing_paren_leaves_the_group() {
    let mut f = typed("(x)");
    assert!(f.cursor().path.is_empty());
    assert_eq!(f.cursor().offset, 1);
    f.type_str("+1");
    assert_eq!(f.to_latex(), "\\left(x\\right)+1");
}

#[test]
fn literal_braces_become_visible_delimiters() {
    assert_eq!(typed("x^{2}").to_latex(), "x^{\\left\\{2\\right\\}}");
}

// ------------------------------------------------------------- esc and enter

#[test]
fn escape_leaves_every_structure_and_reports_it() {
    let mut f = typed("1/2");
    assert_eq!(f.escape(), EscapeOutcome::LeftStructure);
    assert!(f.cursor().path.is_empty());
    assert_eq!(f.cursor().offset, 1);
    assert_eq!(f.escape(), EscapeOutcome::AtRoot);
}

#[test]
fn enter_commits_the_latex_including_empty_slots_and_resets() {
    let mut f = typed("1/");
    assert_eq!(f.to_latex(), "\\frac{1}{ }");
    assert_eq!(f.enter(), "\\frac{1}{ }");
    assert!(f.is_blank());
    assert_eq!(f.to_latex(), "");
}

// ------------------------------------------------------------------ to_latex

#[test]
fn latex_of_nested_structures() {
    let cases: &[(&str, &str)] = &[
        ("1/2x", "\\frac{1}{2x}"),
        ("(x+1)^2", "\\left(x+1\\right)^2"),
        ("\\sqrt x", "\\sqrt{x}"),
        ("x_1^2", "x_{1^2}"),
        ("x^2", "x^2"),
        ("x^{2y}", "x^{\\left\\{2y\\right\\}}"),
        ("1+2/", "1+\\frac{2}{ }"),
        ("/", "\\frac{ }{ }"),
        ("2*3", "2\\cdot3"),
    ];
    for (keys, expected) in cases {
        assert_eq!(&typed(keys).to_latex(), expected, "typing {keys:?}");
    }
}

#[test]
fn empty_field_prints_nothing() {
    assert_eq!(MathField::new().to_latex(), "");
}

// ------------------------------------------------------------------- to_expr

#[test]
fn to_expr_reads_a_complete_field() {
    let f = typed("1/2");
    let e = f.to_expr().expect("1/2 is complete");
    assert_eq!(canlab_tex::to_latex(&e), "\\frac{1}{2}");
}

#[test]
fn to_expr_reads_a_sum_and_a_power() {
    // `canlab-core` canonicalises, so the sum comes back in its own order.
    assert_eq!(typed("x+1").to_expr_latex().unwrap(), "1 + x");
    assert_eq!(typed("x^2").to_expr_latex().unwrap(), "x^{2}");
}

#[test]
fn to_expr_reads_a_root_as_a_half_power() {
    let e = typed("\\sqrt x").to_expr().unwrap();
    assert_eq!(canlab_tex::to_latex(&e), "\\sqrt{x}");
}

#[test]
fn to_expr_reads_a_function_application() {
    let e = typed("sin x").to_expr().unwrap();
    assert_eq!(canlab_tex::to_latex(&e), "\\sin\\left(x\\right)");
}

#[test]
fn to_expr_refuses_a_half_typed_field() {
    let f = typed("1/");
    assert_eq!(f.to_latex(), "\\frac{1}{ }");
    assert_eq!(f.build_expr(), Err(ToExprError::EmptySlot));
    assert_eq!(f.to_expr(), None);
}

#[test]
fn to_expr_refuses_a_half_typed_denominator_before_it_is_emptied() {
    let mut f = typed("1/2");
    f.backspace(); // denominator now empty
    assert_eq!(f.to_expr(), None);
    f.type_str("3");
    assert!(f.to_expr().is_some());
}

#[test]
fn to_expr_refuses_a_subscript() {
    assert_eq!(typed("x_1").build_expr(), Err(ToExprError::Subscript));
    // `x_1^2` is `x_{1^2}`, so it is a subscript too.
    assert_eq!(typed("x_1^2").build_expr(), Err(ToExprError::Subscript));
}

#[test]
fn to_expr_refuses_a_dangling_superscript() {
    assert_eq!(typed("^").build_expr(), Err(ToExprError::DanglingSuperscript));
}

#[test]
fn to_expr_refuses_a_relation() {
    let err = typed("x<=y").build_expr().unwrap_err();
    assert!(matches!(err, ToExprError::UnsupportedCommand(_)), "{err:?}");
}

#[test]
fn to_expr_refuses_an_empty_field() {
    assert_eq!(MathField::new().build_expr(), Err(ToExprError::EmptySlot));
}

// ------------------------------------------------------------------ invariants

#[test]
fn every_cursor_position_is_reachable_by_walking_right() {
    let mut f = typed("1/2");
    while f.move_left() {}
    let total = f.positions().len();
    let mut seen = 1;
    while f.move_right() {
        seen += 1;
    }
    assert_eq!(seen, total);
}

#[test]
fn a_renderer_can_place_the_cursor_from_the_position_list() {
    let mut f = typed("1/2");
    let denominator = f
        .positions()
        .into_iter()
        .find(|c| c.slot() == Some(Slot::Den) && c.offset == 0)
        .expect("the denominator start is a position");
    assert!(f.set_cursor(denominator));
    assert_eq!(slot_path(&f), vec![Slot::Den]);
    assert_eq!(f.cursor().offset, 0);
}

#[test]
fn placing_the_cursor_at_a_path_that_does_not_exist_is_refused() {
    let mut f = typed("1");
    let mut c = canlab_input::Cursor::root();
    c.path.push(canlab_input::PathStep::new(7, Slot::Den));
    assert!(!f.set_cursor(c));
    // The field is untouched.
    assert_eq!(f.to_latex(), "1");
    assert_eq!(f.cursor().offset, 1);
}

#[test]
fn inserting_a_template_into_a_denominator_stays_in_the_denominator() {
    let mut f = typed("1/2");
    f.insert_template(Template::Sqrt).unwrap();
    assert_eq!(f.to_latex(), "\\frac{1}{2\\sqrt{ }}");
    assert_eq!(slot_path(&f), vec![Slot::Den, Slot::Body]);
}
