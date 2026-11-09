//! The Abitti formula palette, as data.
//!
//! Copied deliberately from `digabi/rich-text-editor`
//! (`src/app/components/toolbar/math-char-data.ts` and `src/latexCommands.ts`),
//! which is MIT, Copyright 2017 Matriculation Examination Board, Finland. See
//! `NOTICE` at the repository root.
//!
//! Why copy it: Finnish upper-secondary students spend two years learning
//! exactly this palette in the matriculation examination. Matching the
//! vocabulary is worth more than matching the pixels.
//!
//! A [`Button`] carries what the user sees and what gets inserted. For
//! structural entries the inserted text contains `X`, the position the cursor
//! should land in — the same convention the original data uses.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Button {
    /// What is drawn on the key.
    pub label: &'static str,
    /// What is typed into the formula editor.
    pub insert: &'static str,
}

const fn glyph(label: &'static str, insert: &'static str) -> Button {
    Button { label, insert }
}

/// Row 1: the always-visible quick symbols.
pub const BASIC: &[Button] = &[
    glyph("°", "°"),
    glyph("·", r"\cdot"),
    glyph("×", r"\times"),
    glyph("±", r"\pm"),
    glyph("∞", r"\infty"),
    glyph("²", "^2"),
    glyph("³", "^3"),
    glyph("½", r"\frac{1}{2}"),
    glyph("⅓", r"\frac{1}{3}"),
    glyph("π", r"\pi"),
    glyph("‰", "‰"),
    glyph("α", r"\alpha"),
    glyph("β", r"\beta"),
    // The remaining Greek letters live behind the expand button.
    glyph("Γ", r"\Gamma"),
    glyph("γ", r"\gamma"),
    glyph("Δ", r"\Delta"),
    glyph("δ", r"\delta"),
    glyph("ε", r"\varepsilon"),
    glyph("ζ", r"\zeta"),
    glyph("η", r"\eta"),
    glyph("θ", r"\theta"),
    glyph("ϑ", r"\vartheta"),
    glyph("ι", r"\iota"),
    glyph("κ", r"\kappa"),
    glyph("Λ", r"\Lambda"),
    glyph("λ", r"\lambda"),
    glyph("µ", r"\mu"),
    glyph("ν", r"\nu"),
    glyph("Ξ", r"\Xi"),
    glyph("ξ", r"\xi"),
    glyph("∏", r"\Pi"),
    glyph("ρ", r"\rho"),
    glyph("∑", r"\Sigma"),
    glyph("σ", r"\sigma"),
    glyph("τ", r"\tau"),
    glyph("Υ", r"\Upsilon"),
    glyph("υ", r"\upsilon"),
    glyph("Φ", r"\Phi"),
    glyph("ϕ", r"\phi"),
    glyph("χ", r"\chi"),
    glyph("Ψ", r"\Psi"),
    glyph("ψ", r"\psi"),
    glyph("Ω", r"\Omega"),
    glyph("ω", r"\omega"),
    glyph("∂", r"\partial"),
    glyph("φ", r"\varphi"),
];

pub const ALGEBRA: &[Button] = &[
    glyph("≠", r"\neq"),
    glyph("≈", r"\approx"),
    glyph("≤", r"\leq"),
    glyph("≥", r"\geq"),
    glyph("<", "<"),
    glyph(">", ">"),
    glyph("∼", r"\sim"),
    glyph("≡", r"\equiv"),
    glyph("≢", r"\not\equiv"),
    glyph("∘", r"\circ"),
    glyph("…", r"\ldots"),
    glyph("∝", r"\propto"),
];

pub const GEOMETRY: &[Button] = &[
    glyph("∢", r"\sphericalangle"),
    glyph("|", r"\mid"),
    glyph("‖", r"\parallel"),
    glyph("⇌", r"\xrightleftharpoons"),
    glyph("⇅", "⇅"),
    glyph("∠", r"\angle"),
    glyph("↑", r"\uparrow"),
    glyph("↗", r"\nearrow"),
    glyph("↘", r"\searrow"),
    glyph("↓", r"\downarrow"),
    glyph("↔", r"\leftrightarrow"),
    glyph("⊥", r"\perp"),
];

pub const SET_THEORY: &[Button] = &[
    glyph("→", r"\rightarrow"),
    glyph("⇒", r"\Rightarrow"),
    glyph("⇔", r"\Leftrightarrow"),
    glyph("∈", r"\in"),
    glyph("ℤ", r"\mathbb{Z}"),
    glyph("ℝ", r"\mathbb{R}"),
    glyph("∃", r"\exists"),
    glyph("∀", r"\forall"),
    glyph("ℕ", r"\mathbb{N}"),
    glyph("ℚ", r"\mathbb{Q}"),
    glyph("∩", r"\cap"),
    glyph("∪", r"\cup"),
    glyph("∖", r"\setminus"),
    glyph("⊂", r"\subset"),
    glyph("⊄", r"\not\subset"),
    glyph("∉", r"\notin"),
    glyph("∅", r"\varnothing"),
    glyph("∧", r"\wedge"),
    glyph("∨", r"\vee"),
    glyph("¬", r"\neg"),
    glyph("∇", r"\nabla"),
];

/// Rows 2–3: the structural entries. `X` marks the cursor target.
pub const STRUCTURES: &[Button] = &[
    glyph("√", r"\sqrt{X}"),
    glyph("xⁿ", "x^{X}"),
    glyph("a/b", r"\frac{X}{X}"),
    glyph("|x|", r"\left|X\right|"),
    glyph("( )", r"\left(X\right)"),
    glyph("[ ]", r"\left[X\right]"),
    glyph("xₙ", "x_X"),
    glyph("∫", r"\int_{X}^{X}"),
    glyph("∑", r"\sum_{X}^{X}"),
    glyph("lim", r"\lim_{X}"),
    glyph("lim x→∞", r"\lim_{x\rightarrow\infty}"),
    glyph("sin", r"\sin"),
    glyph("cos", r"\cos"),
    glyph("tan", r"\tan"),
    glyph("∛", r"\sqrt[X]{X}"),
    glyph("→v", r"\overrightarrow{X}"),
    glyph("←v", r"\overleftarrow{X}"),
    glyph("x̲", r"\underrightarrow{X}"),
    glyph("x̄", r"\overline{X}"),
    glyph("î", r"\bar{\text{i}}"),
    glyph("ĵ", r"\bar{\text{j}}"),
    glyph("k̂", r"\bar{\text{k}}"),
    glyph("C(n,k)", r"\binom{X}{X}"),
    glyph("cases", "\\begin{cases} X \\\\ X \\end{cases}"),
    glyph("matrix", "\\begin{matrix} X & X \\\\ X & X \\end{matrix}"),
    glyph("array", "\\begin{array}{l|l} X & X \\\\ \\hline X & X \\end{array}"),
    glyph("roman", r"\mathrm{T}"),
];

/// Every category, in the order the palette shows them.
pub const ALL: &[(&str, &[Button])] = &[
    ("basic", BASIC),
    ("algebra", ALGEBRA),
    ("geometry", GEOMETRY),
    ("set theory", SET_THEORY),
    ("structures", STRUCTURES),
];

/// Look up a button by its visible label, across every category.
pub fn find(label: &str) -> Option<&'static Button> {
    ALL.iter()
        .flat_map(|(_, buttons)| buttons.iter())
        .find(|button| button.label == label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greek_uses_the_exam_spellings() {
        // Not \epsilon / \phi: the palette emits the variant forms.
        assert_eq!(find("ε").unwrap().insert, r"\varepsilon");
        assert_eq!(find("ϑ").unwrap().insert, r"\vartheta");
        assert_eq!(find("φ").unwrap().insert, r"\varphi");
        assert_eq!(find("ϕ").unwrap().insert, r"\phi");
    }

    #[test]
    fn quick_fractions_are_precomposed() {
        assert_eq!(find("½").unwrap().insert, r"\frac{1}{2}");
        assert_eq!(find("⅓").unwrap().insert, r"\frac{1}{3}");
    }

    #[test]
    fn relations_use_the_palette_spelling() {
        assert_eq!(find("≤").unwrap().insert, r"\leq");
        assert_eq!(find("≥").unwrap().insert, r"\geq");
        assert_eq!(find("≠").unwrap().insert, r"\neq");
    }

    #[test]
    fn every_category_is_non_empty() {
        for (name, buttons) in ALL {
            assert!(!buttons.is_empty(), "category `{name}` is empty");
        }
    }

    #[test]
    fn no_duplicate_labels_within_a_category() {
        // `∑` legitimately appears twice across categories: once as the capital
        // Sigma *letter* (`\Sigma`) and once as the summation operator
        // (`\sum_{}^{}`). Abitti's own table does the same, so cross-category
        // duplicates are allowed; within one category they would be a defect.
        for (name, buttons) in ALL {
            let mut seen = std::collections::HashSet::new();
            for button in *buttons {
                assert!(
                    seen.insert(button.label),
                    "duplicate label `{}` inside category `{name}`",
                    button.label
                );
            }
        }
    }

    #[test]
    fn structural_entries_mark_where_the_cursor_goes() {
        // Entries that take arguments mark each slot with `X`. A few palette
        // keys (`\sin`, `\mathrm{T}`) insert a bare token and deliberately have
        // no marker, so only the argument-taking ones are asserted here.
        for label in ["√", "a/b", "xⁿ", "∫", "∑", "( )", "[ ]", "C(n,k)"] {
            let button = STRUCTURES
                .iter()
                .find(|button| button.label == label)
                .unwrap_or_else(|| panic!("no structural button `{label}`"));
            assert!(
                button.insert.contains('X'),
                "`{label}` has no cursor marker: {}",
                button.insert
            );
        }
    }
}
