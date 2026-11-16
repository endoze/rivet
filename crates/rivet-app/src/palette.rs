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
  /// The background of the window and of rows at rest.
  pub background: Hsla,
  /// The background of the sidebar, the detail pane, and the status bar.
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
  /// The text and outline of a mod's update tag.
  pub tag_update: Hsla,
  /// The text and outline of a deprecated mod's tag.
  pub tag_deprecated: Hsla,
  /// The text and outline of the tag on a mod missing a dependency.
  pub tag_missing: Hsla,
  /// The text and outline of the mod loader's tag.
  pub tag_loader: Hsla,
  /// The colors of a segmented control.
  pub segments: Segments,
}

/// The colors of a segmented control, so the selected segment stands out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segments {
  /// The background of the control and of every unselected segment.
  pub unselected: Hsla,
  /// The label of an unselected segment.
  pub unselected_text: Hsla,
  /// The fill of the selected segment.
  pub selected: Hsla,
  /// The label of the selected segment: the full text color.
  pub selected_text: Hsla,
  /// The bar under the selected segment.
  pub indicator: Hsla,
}

impl Palette {
  /// The palette of the active theme. `muted_text` is the theme's muted
  /// foreground, moved toward `text` only as far as it takes to reach WCAG AA
  /// on every background here. The toast accents are the theme's info,
  /// success, warning, and danger colors, moved the same way only as far as
  /// it takes to reach the WCAG AA contrast for icons on a toast.
  /// `danger_text` is the theme's danger color moved as far as it takes to
  /// reach WCAG AA on a menu. The tag colors are the theme's info, warning,
  /// danger, and primary colors, each moved as far as it takes to reach WCAG
  /// AA on a row at rest, hovered, or selected, and on the panel. A
  /// segmented control sits on the launch option line's color, its selected
  /// segment tinted like the selected row and underlined in the theme's info
  /// color, moved as far as it takes to reach the WCAG AA contrast for
  /// outlines on both.
  pub fn new(cx: &App) -> Self {
    let theme = cx.theme();
    let background = theme.background;
    let panel = background.blend(theme.sidebar);
    let row_hover = background.blend(theme.list_hover);
    let row_selected = background.blend(theme.list_active);
    let code = background.blend(theme.muted);
    let toast = background.blend(theme.popover);
    let text = theme.foreground;
    let muted_text = reaching(
      AA,
      background.blend(theme.muted_foreground),
      text,
      &[background, panel, row_hover, row_selected, code],
    );
    let accent = |color: Hsla| reaching(NON_TEXT, toast.blend(color), text, &[toast]);
    let tag = |color: Hsla| {
      reaching(
        AA,
        background.blend(color),
        text,
        &[background, panel, row_hover, row_selected],
      )
    };
    let segment = code.blend(theme.list_active);
    let segments = Segments {
      unselected: code,
      unselected_text: muted_text,
      selected: segment,
      selected_text: text,
      indicator: reaching(NON_TEXT, code.blend(theme.info), text, &[code, segment]),
    };

    Self {
      text,
      muted_text,
      background,
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
      tag_update: tag(theme.info),
      tag_deprecated: tag(theme.warning),
      tag_missing: tag(theme.danger),
      tag_loader: tag(theme.primary),
      segments,
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
