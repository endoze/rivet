//! Rivet's light and dark themes and the IBM Plex fonts they set text in,
//! both bundled into the binary.

use gpui_kit::component::{ActiveTheme as _, Theme, ThemeConfig, ThemeMode};
use gpui_kit::{App, FontFeatures, Styled, Window, WindowAppearance, px};
use rivet_core::settings::Appearance;
use std::borrow::Cow;
use std::rc::Rc;
use std::sync::Arc;

/// The family every screen sets its text in.
pub const SANS: &str = "IBM Plex Sans";
/// The family for the Steam launch line and code in a readme.
pub const MONO: &str = "IBM Plex Mono";
/// Body text, in pixels. The theme's rem, so gpui's relative sizes scale
/// from it.
pub const BODY_TEXT_SIZE: f32 = 14.0;
/// Small text, in pixels: hints, bylines, and the status bar.
pub const SMALL_TEXT_SIZE: f32 = 13.0;

/// The spec's type scale and figure style on any element that sets text.
pub trait Typography: Styled + Sized {
  /// Sets text at [`SMALL_TEXT_SIZE`]. gpui's `text_sm` is 0.875rem, which
  /// is 12.25px on a 14px body.
  fn text_small(self) -> Self {
    self.text_size(px(SMALL_TEXT_SIZE))
  }

  /// Sets figures at one width, so counts, versions, and download totals
  /// line up and keep their width as their digits change.
  fn tabular(self) -> Self {
    self.font_features(FontFeatures(Arc::new(vec![("tnum".into(), 1)])))
  }
}

impl<E: Styled> Typography for E {}

/// IBM Plex Sans at 400, 500, 600, and 700, and IBM Plex Mono at 400.
const FONTS: [&[u8]; 5] = [
  include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf"),
  include_bytes!("../assets/fonts/IBMPlexSans-Medium.ttf"),
  include_bytes!("../assets/fonts/IBMPlexSans-SemiBold.ttf"),
  include_bytes!("../assets/fonts/IBMPlexSans-Bold.ttf"),
  include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf"),
];

/// One appearance's design tokens, as the hex colors a theme config reads.
struct Tokens {
  name: &'static str,
  mode: ThemeMode,
  /// The window, under the list and the banners.
  background: &'static str,
  /// The sidebar, the detail pane, the status bar, menus, and toasts.
  panel: &'static str,
  /// Hover, inputs, and secondary buttons.
  raised: &'static str,
  /// Borders and dividers.
  line: &'static str,
  text: &'static str,
  muted: &'static str,
  /// The primary action, the one brass thing on a screen.
  brass: &'static str,
  /// Text on brass.
  brass_ink: &'static str,
  /// The selected row: info, mostly transparent.
  selection: &'static str,
  success: &'static str,
  warning: &'static str,
  danger: &'static str,
  info: &'static str,
}

const LIGHT: Tokens = Tokens {
  name: "Rivet Light",
  mode: ThemeMode::Light,
  background: "#eef1f5",
  panel: "#ffffff",
  raised: "#f5f7fa",
  line: "#d8dee7",
  text: "#1c2330",
  muted: "#5b6577",
  brass: "#946f1c",
  brass_ink: "#ffffff",
  selection: "#2f6db51a",
  success: "#2e8a52",
  warning: "#b85a1e",
  danger: "#c23b3b",
  info: "#2f6db5",
};

const DARK: Tokens = Tokens {
  name: "Rivet Dark",
  mode: ThemeMode::Dark,
  background: "#1a1f27",
  panel: "#212833",
  raised: "#2a3240",
  line: "#364052",
  text: "#e6e9ef",
  muted: "#9aa4b5",
  brass: "#d4a84a",
  brass_ink: "#1a1f27",
  selection: "#6fa8e824",
  success: "#6cc08b",
  warning: "#e8925a",
  danger: "#e86a6a",
  info: "#6fa8e8",
};

/// Registers the bundled fonts and makes Rivet's themes the ones the light
/// and dark appearances load, then reloads the current appearance with its
/// theme. Call it after `gpui_kit::init`.
///
/// # Panics
///
/// If the platform rejects a bundled font.
pub fn init(cx: &mut App) {
  let fonts = FONTS.iter().map(|font| Cow::Borrowed(*font)).collect();

  cx.text_system()
    .add_fonts(fonts)
    .expect("the bundled fonts load");
  Theme::update(cx, |theme| {
    theme.light_theme = Rc::new(config(&LIGHT));
    theme.dark_theme = Rc::new(config(&DARK));
  });

  let mode = cx.theme().mode;

  Theme::change(mode, None, cx);
}

/// Whether the light or dark theme applies for `appearance` while the system
/// looks like `system`.
///
/// ```
/// use gpui_kit::WindowAppearance;
/// use gpui_kit::component::ThemeMode;
/// use rivet_app::theme;
/// use rivet_core::settings::Appearance;
///
/// assert_eq!(theme::mode(Appearance::System, WindowAppearance::Dark), ThemeMode::Dark);
/// assert_eq!(theme::mode(Appearance::Light, WindowAppearance::Dark), ThemeMode::Light);
/// ```
pub fn mode(appearance: Appearance, system: WindowAppearance) -> ThemeMode {
  match appearance {
    Appearance::System => system.into(),
    Appearance::Light => ThemeMode::Light,
    Appearance::Dark => ThemeMode::Dark,
  }
}

/// Loads the theme `appearance` calls for, reading the system's appearance
/// from `window`. Does nothing when that theme is already loaded, so it can
/// run on every render.
pub fn apply(appearance: Appearance, window: &Window, cx: &mut App) {
  let mode = mode(appearance, window.appearance());

  if cx.theme().mode != mode {
    Theme::change(mode, None, cx);
  }
}

/// The theme config for one appearance. Every color the component library
/// would otherwise take from the primary color is named here, so brass stays
/// on the primary action.
fn config(tokens: &Tokens) -> ThemeConfig {
  let color = |hex: &str| Some(hex.into());
  let mut config = ThemeConfig::default();

  config.name = tokens.name.into();
  config.mode = tokens.mode;
  config.font_family = Some(SANS.into());
  config.mono_font_family = Some(MONO.into());
  config.font_size = Some(BODY_TEXT_SIZE);
  config.radius = Some(6);
  config.radius_lg = Some(10);

  let colors = &mut config.colors;

  colors.background = color(tokens.background);
  colors.foreground = color(tokens.text);
  colors.muted_foreground = color(tokens.muted);
  colors.caret = color(tokens.text);
  colors.border = color(tokens.line);
  colors.input = color(tokens.line);
  colors.window_border = color(tokens.line);
  colors.scrollbar_thumb = color(tokens.line);
  colors.popover = color(tokens.panel);
  colors.popover_foreground = color(tokens.text);
  colors.sidebar = color(tokens.panel);
  colors.sidebar_foreground = color(tokens.text);
  colors.sidebar_border = color(tokens.line);
  colors.sidebar_accent = color(tokens.raised);
  colors.sidebar_accent_foreground = color(tokens.text);
  colors.sidebar_primary = color(tokens.brass);
  colors.sidebar_primary_foreground = color(tokens.brass_ink);
  colors.title_bar = color(tokens.panel);
  colors.title_bar_border = color(tokens.line);
  colors.status_bar = color(tokens.panel);
  colors.status_bar_border = color(tokens.line);
  colors.muted = color(tokens.raised);
  colors.secondary = color(tokens.raised);
  colors.secondary_foreground = color(tokens.text);
  colors.button = color(tokens.raised);
  colors.button_foreground = color(tokens.text);
  colors.accent = color(tokens.raised);
  colors.accent_foreground = color(tokens.text);
  colors.list_hover = color(tokens.raised);
  colors.list_active = color(tokens.selection);
  colors.list_active_border = color(tokens.info);
  colors.selection = color(tokens.selection);
  colors.drop_target = color(tokens.selection);
  colors.drag_border = color(tokens.info);
  colors.ring = color(tokens.info);
  colors.link = color(tokens.info);
  colors.primary = color(tokens.brass);
  colors.primary_foreground = color(tokens.brass_ink);
  colors.success = color(tokens.success);
  colors.warning = color(tokens.warning);
  colors.danger = color(tokens.danger);
  colors.info = color(tokens.info);

  config
}
