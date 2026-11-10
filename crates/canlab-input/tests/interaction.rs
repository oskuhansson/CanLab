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
    // Offsets count *characters*, not nodes: `2x+1` is four of them.
    assert_eq!(f.cursor().offset, 4);
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

#[test]
fn shift_right_selects_a_run_and_slash_promotes_it_to_a_numerator() {
    // The idiom this exists for: select something, press `/`, and it becomes the
    // numerator. Until now the model could do it but the GUI had no way to make a
    // selection.
    let mut f = MathField::new();
    f.type_str("x+1");
    while f.move_left() {}
    f.extend_right();
    f.extend_right();
    assert_eq!(
        f.selection().map(|s| (s.start.offset, s.end.offset)),
        Some((0, 2))
    );
    f.slash();
    // `x+` is what was selected; the `1` was not.
    assert_eq!(f.to_latex(), "\\frac{x+}{ }1");
}

#[test]
fn select_all_then_slash_makes_the_whole_line_the_numerator() {
    let mut f = typed("x+1");
    f.select_all();
    f.slash();
    assert_eq!(f.to_latex(), "\\frac{x+1}{ }");
}

#[test]
fn clearing_the_selection_keeps_the_tree() {
    let mut f = typed("1/2");
    f.select_all();
    assert!(f.selection().is_some());
    f.clear_selection();
    assert!(f.selection().is_none());
    assert_eq!(f.to_latex(), "\\frac{1}{2}");
}

#[test]
fn plain_movement_drops_the_selection() {
    let mut f = typed("x+1");
    f.select_all();
    assert!(f.selection().is_some());
    f.move_left();
    assert!(f.selection().is_none());
}

#[test]
fn extending_across_slots_leaves_no_selection() {
    // A selection is a range inside one row; the model says so rather than
    // pretending it can span a fraction and a denominator.
    let mut f = MathField::new();
    f.type_str("1/");
    assert!(f.cursor().path.len() == 1);
    f.extend_up();
    assert!(f.selection().is_none());
}

#[test]
fn select_all_then_typing_replaces_the_line() {
    let mut f = typed("2x+1");
    f.select_all();
    f.insert_char('9');
    assert_eq!(f.to_latex(), "9");
}

#[test]
fn a_system_keeps_each_line_to_itself() {
    let mut f = MathField::new();
    f.type_str("x=1");
    f.new_line();
    f.type_str("y=2");
    assert_eq!(f.to_latex(), "\\begin{cases} x=1 \\\\ y=2 \\end{cases}");
    assert_eq!(f.line_count(), 2);
    assert!(f.is_system());
    assert!(matches!(
        f.build_expr(),
        Err(canlab_input::ToExprError::System)
    ));
    // An equation line carries a relation, so it is read as two sides.
    let equations = f.build_equations().expect("both lines are equations");
    assert_eq!(equations.len(), 2);
    assert!(equations.iter().all(|(_, rhs)| rhs.is_some()));
}

#[test]
fn backspace_at_the_start_of_an_empty_line_folds_it_away() {
    let mut f = typed("x=1");
    f.new_line();
    assert_eq!(f.line_count(), 2);
    f.backspace();
    assert_eq!(f.line_count(), 1);
    assert_eq!(f.to_latex(), "x=1");
}

#[test]
fn up_and_down_cross_between_equations() {
    let mut f = typed("x=1");
    f.new_line();
    assert_eq!(f.line(), 1);
    assert!(f.move_up());
    assert_eq!(f.line(), 0);
    assert!(f.move_down());
    assert_eq!(f.line(), 1);
}

#[test]
fn a_run_of_letters_and_digits_is_a_product() {
    // The field glues `2x` into one run (which is what `/` needs), so the reader
    // has to split it again. This is how everyone writes a product.
    assert_eq!(typed("2x").to_expr_latex().unwrap(), "2x");
    // canlab-tex separates adjacent symbols so `xy` cannot be read as one name.
    assert_eq!(typed("3xy").to_expr_latex().unwrap(), "3x y");
    assert_eq!(typed("2.5x").to_expr_latex().unwrap(), "2.5x");
    assert_eq!(typed("x2").to_expr_latex().unwrap(), "2x");
    assert!(typed("2x+1").to_expr().is_some());
}

#[test]
fn a_leading_minus_is_a_sign_and_not_an_empty_term() {
    // A sign with nothing before it is unary. This used to make the whole
    // expression unreadable ("the field has an empty slot"), which is why
    // `e^{-x^2}` could be typed but never evaluated.
    assert!(typed("-x").to_expr().is_some());
    assert!(typed("-1").to_expr().is_some());
    assert!(typed("e^{-x^2}").to_expr().is_some());
    assert!(typed("e^(-x^2").to_expr().is_some());
    // Canonical form sorts the terms, so the constant comes first.
    assert_eq!(typed("x+-1").to_expr().unwrap().to_string(), "-1 + x");
    assert_eq!(typed("x--y").to_expr().unwrap().to_string(), "x + y");
    // A bare sign on its own is still incomplete.
    assert_eq!(typed("-").build_expr(), Err(canlab_input::ToExprError::EmptySlot));
}

#[test]
fn a_logarithm_with_a_base_is_a_ratio_of_natural_logs() {
    // `\log_2(10)` — the subscript is the base, not a decoration on a variable.
    let field = typed("\\log_2(10)");
    let expression = field.to_expr().expect("log with a base is an expression");
    let program = canlab_core::Program::compile(&expression, &[]).expect("evaluable");
    let value = program.eval(&[]);
    assert!((value - 10f64.log2()).abs() < 1e-9, "got {value}");
}

#[test]
fn a_unit_marker_is_notation_and_the_number_still_reads() {
    // `80°` is a perfectly good thing to write in an exam answer. The degree
    // sign is not an operator, so it must not make the whole field unreadable.
    assert_eq!(typed("80°").to_expr().unwrap().to_string(), "80");
    assert_eq!(typed("80°").to_latex(), "80°", "the sign stays in the LaTeX");
    assert!(typed("5%").to_expr().is_some());
    assert!(typed("1/2 + 30°").to_expr().is_some());
}

#[test]
fn enter_commits_a_half_typed_command_instead_of_breaking_the_line() {
    // `\bar` then Enter must be the accent, with the cursor in its argument —
    // not an accent *and* a new line.
    let mut f = MathField::new();
    f.type_str("\\bar");
    assert_eq!(f.pending_command(), Some("bar"));
    assert!(f.commit_command());
    assert_eq!(f.line_count(), 1);
    assert_eq!(f.system_count(), 1);
    assert_eq!(f.to_latex(), "\\bar{ }");
    assert_eq!(f.cursor().path.len(), 1, "the cursor is inside the argument");
    f.type_str("x");
    assert_eq!(f.to_latex(), "\\bar{x}");
}

#[test]
fn committing_with_nothing_pending_reports_so() {
    let mut f = typed("x+1");
    assert!(!f.commit_command());
    assert_eq!(f.to_latex(), "x+1");
}

#[test]
fn a_document_holds_several_separate_systems() {
    // Two questions on one page: they must not merge into one long system.
    let mut f = MathField::new();
    f.type_str("x+y=3");
    f.new_line();
    f.type_str("2x-y=0");
    assert_eq!(f.system_count(), 1);
    assert_eq!(f.line_count(), 2);

    f.new_system();
    assert_eq!(f.system_count(), 2);
    assert_eq!(f.line_count(), 1);
    f.type_str("a+b=1");

    assert_eq!(
        f.to_latex(),
        "\\begin{cases} x+y=3 \\\\ 2x-y=0 \\end{cases}\\quad a+b=1"
    );
    // Equations come out in document order, across systems.
    assert_eq!(f.build_equations().map(|e| e.len()), Ok(3));
    // And `build_expr` still refuses: a page is not a value.
    assert!(matches!(
        f.build_expr(),
        Err(canlab_input::ToExprError::System)
    ));
}

#[test]
fn a_single_line_system_folds_back_when_it_is_left_empty() {
    let mut f = typed("x=1");
    f.new_system();
    assert_eq!(f.system_count(), 2);
    f.backspace();
    assert_eq!(f.system_count(), 1);
    assert_eq!(f.to_latex(), "x=1");
}

#[test]
fn moving_down_crosses_into_the_next_system() {
    let mut f = typed("x=1");
    f.new_system();
    f.type_str("y=2");
    // Back to the first system's only line, then down again into the second.
    assert_eq!((f.system(), f.line()), (1, 0));
    assert!(f.move_up());
    assert_eq!((f.system(), f.line()), (0, 0));
    assert!(f.move_down());
    assert_eq!((f.system(), f.line()), (1, 0));
}

#[test]
fn the_cursor_can_be_sent_to_any_equation_of_any_system() {
    let mut f = typed("a=1");
    f.new_line();
    f.type_str("b=2");
    f.new_system();
    f.type_str("c=3");
    assert!(f.set_position(0, 1));
    assert_eq!((f.system(), f.line()), (0, 1));
    assert!(!f.set_position(9, 0), "there is no such system");
}

// ------------------------------------------------------------ esc and enter

#[test]
fn escape_leaves_every_structure_and_reports_it() {
    let mut f = typed("1/2");
    assert_eq!(f.escape(), EscapeOutcome::LeftStructure);
    assert!(f.cursor().path.is_empty());
    assert_eq!(f.cursor().offset, 1);
    assert_eq!(f.escape(), EscapeOutcome::AtRoot);
}

#[test]
fn commit_returns_the_latex_including_empty_slots_and_resets() {
    let mut f = typed("1/");
    assert_eq!(f.to_latex(), "\\frac{1}{ }");
    assert_eq!(f.commit(), "\\frac{1}{ }");
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
