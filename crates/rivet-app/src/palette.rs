//! The colors the shell paints with. Every one comes from the active
//! gpui-component theme, so text and the backgrounds under it change
//! together between light and dark.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{App, Hsla};

/// WCAG 2 level AA for normal-size text.
const AA: f32 = 4.5;
/// WCAG 2 level AA for icons and outlines.
const NON_TEXT: f32 = 3.0;

/// The active theme's colors as the shell uses them, each opaque.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
  /// Text on any panel, row, toast, or the launch option line.
  pub text: Hsla,
  /// Secondary text: bylines, descriptions, and status lines.
  pub muted_text: Hsla,
  /// The background of panels and of rows at rest.
  pub panel: Hsla,
  /// The background of a row under the pointer.
  pub row_hover: Hsla,
  /// The background of the selected row.
  pub row_selected: Hsla,
  /// The background of the launch option line.
  pub code: Hsla,
  /// The outline of a panel.
  pub border: Hsla,
  /// The outline of a panel reporting a failure or asking about one.
  pub danger_border: Hsla,
  /// The background of a toast or a menu.
  pub toast: Hsla,
  /// The text of a menu item that destroys something.
  pub danger_text: Hsla,
  /// The icon and outline of an info toast.
  pub info: Hsla,
  /// The icon and outline of a success toast.
  pub success: Hsla,
  /// The icon and outline of a warning toast.
  pub warning: Hsla,
  /// The icon and outline of an error toast.
  pub error: Hsla,
}

impl Palette {
  /// The palette of the active theme. `muted_text` is the theme's muted
  /// foreground, moved toward `text` only as far as it takes to reach WCAG AA
  /// on every background here. The toast accents are the theme's info,
  /// success, warning, and danger colors, moved the same way only as far as
  /// it takes to reach the WCAG AA contrast for icons on a toast.
  /// `danger_text` is the theme's danger color moved as far as it takes to
  /// reach WCAG AA on a menu.
  pub fn new(cx: &App) -> Self {
    let theme = cx.theme();
    let panel = theme.background;
    let row_hover = panel.blend(theme.list_hover);
    let row_selected = panel.blend(theme.secondary);
    let code = panel.blend(theme.muted);
    let toast = panel.blend(theme.popover);
    let text = theme.foreground;
    let muted_text = reaching(
      AA,
      panel.blend(theme.muted_foreground),
      text,
      &[panel, row_hover, row_selected, code],
    );
    let accent = |color: Hsla| reaching(NON_TEXT, toast.blend(color), text, &[toast]);

    Self {
      text,
      muted_text,
      panel,
      row_hover,
      row_selected,
      code,
      border: theme.border,
      danger_border: theme.danger,
      toast,
      danger_text: reaching(AA, toast.blend(theme.danger), text, &[toast]),
      info: accent(theme.info),
      success: accent(theme.success),
      warning: accent(theme.warning),
      error: accent(theme.danger),
    }
  }
}

/// The WCAG 2 contrast ratio of two opaque colors, from 1 to 21.
///
/// ```
/// use gpui_kit::rgb;
/// use rivet_app::palette::contrast;
///
/// let ratio = contrast(rgb(0x000000).into(), rgb(0xffffff).into());
///
/// assert!((ratio - 21.0).abs() < 0.01);
/// ```
pub fn contrast(a: Hsla, b: Hsla) -> f32 {
  let (a, b) = (luminance(a), luminance(b));

  (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// WCAG 2 relative luminance.
fn luminance(color: Hsla) -> f32 {
  let rgb = color.to_rgb();
  let linear = |channel: f32| {
    if channel <= 0.04045 {
      channel / 12.92
    } else {
      ((channel + 0.055) / 1.055).powf(2.4)
    }
  };

  0.2126 * linear(rgb.r) + 0.7152 * linear(rgb.g) + 0.0722 * linear(rgb.b)
}

/// `color` blended toward `text` in tenths until it reaches `ratio` on all of
/// `backgrounds`, or `text` when no blend does.
fn reaching(ratio: f32, color: Hsla, text: Hsla, backgrounds: &[Hsla]) -> Hsla {
  (0..=10)
    .map(|tenths| color.blend(text.opacity(tenths as f32 / 10.0)))
    .find(|candidate| {
      backgrounds
        .iter()
        .all(|background| contrast(*candidate, *background) >= ratio)
    })
    .unwrap_or(text)
}
