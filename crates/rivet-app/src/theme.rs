//! Rivet's themes, a light and a dark member of each family, and the IBM
//! Plex fonts they set text in, both bundled into the binary.

use gpui_kit::component::{ActiveTheme as _, Theme, ThemeConfig, ThemeMode};
use gpui_kit::{App, FontFeatures, Pixels, Rems, Size, Styled, Window, WindowAppearance, px, rems};
use rivet_core::settings::{Appearance, INTERFACE_SIZES, ThemeFamily};
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
/// Monospaced text, in pixels: the Steam launch line and code in a readme.
const MONO_TEXT_SIZE: f32 = 13.0;

/// The spec's type scale and figure style on any element that sets text.
pub trait Typography: Styled + Sized {
  /// Sets text at [`SMALL_TEXT_SIZE`], scaled with the interface size.
  /// gpui's `text_sm` is 0.875rem, which is 12.25px on a 14px body.
  fn text_small(self) -> Self {
    self.text_size(spec(SMALL_TEXT_SIZE))
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

const DEFAULT_LIGHT: Tokens = Tokens {
  name: "Default Light",
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

const DEFAULT_DARK: Tokens = Tokens {
  name: "Default Dark",
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

/// Atom's One Light. Text, accents, and the background are the
/// one-light-syntax palette, and the other surfaces are the One Light UI
/// levels built on that background:
/// <https://github.com/atom/atom/blob/master/packages/one-light-syntax/styles/colors.less>
/// and
/// <https://github.com/atom/atom/blob/master/packages/one-light-ui/styles/ui-variables-custom.less>.
/// One's accent is the primary action, with One Light UI's dark accent text
/// on it, since white on the accent is under WCAG AA. The selection is the
/// info hue at the Default theme's opacity.
const ONE_LIGHT: Tokens = Tokens {
  name: "One Light",
  mode: ThemeMode::Light,
  background: "#fafafa",
  panel: "#eaeaeb",
  raised: "#ffffff",
  line: "#dbdbdc",
  text: "#383a42",
  muted: "#696c77",
  brass: "#526fff",
  brass_ink: "#000933",
  selection: "#0184bc1a",
  success: "#50a14f",
  warning: "#986801",
  danger: "#e45649",
  info: "#0184bc",
};

/// Atom's One Dark. Text, accents, and the background are the
/// one-dark-syntax palette, and the other surfaces are One Dark UI's:
/// <https://github.com/atom/atom/blob/master/packages/one-dark-syntax/styles/colors.less>
/// and <https://github.com/atom/atom/tree/master/packages/one-dark-ui>.
/// One's accent is the primary action, with One Dark UI's dark accent text
/// on it. The selection is the info hue at the Default theme's opacity.
const ONE_DARK: Tokens = Tokens {
  name: "One Dark",
  mode: ThemeMode::Dark,
  background: "#282c34",
  panel: "#21252b",
  raised: "#2c313a",
  line: "#181a1f",
  text: "#abb2bf",
  muted: "#828997",
  brass: "#528bff",
  brass_ink: "#001133",
  selection: "#56b6c224",
  success: "#98c379",
  warning: "#d19a66",
  danger: "#e06c75",
  info: "#56b6c2",
};

/// The light and dark members of `family`.
fn members(family: ThemeFamily) -> (&'static Tokens, &'static Tokens) {
  match family {
    ThemeFamily::Default => (&DEFAULT_LIGHT, &DEFAULT_DARK),
    ThemeFamily::One => (&ONE_LIGHT, &ONE_DARK),
  }
}

/// Registers the bundled fonts and makes the Default themes the ones the
/// light and dark appearances load, then reloads the current appearance with
/// its theme. Call it after `gpui_kit::init`.
///
/// # Panics
///
/// If the platform rejects a bundled font.
pub fn init(cx: &mut App) {
  let fonts = FONTS.iter().map(|font| Cow::Borrowed(*font)).collect();

  cx.text_system()
    .add_fonts(fonts)
    .expect("the bundled fonts load");

  let mode = cx.theme().mode;

  change(ThemeFamily::Default, mode, cx);
}

/// Makes `family`'s themes the ones the light and dark appearances load, then
/// loads the one for `mode`.
pub fn change(family: ThemeFamily, mode: ThemeMode, cx: &mut App) {
  let (light, dark) = members(family);

  Theme::update(cx, |theme| {
    theme.light_theme = Rc::new(config(light));
    theme.dark_theme = Rc::new(config(dark));
  });
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

/// Loads the member of `family` that `appearance` calls for, reading the
/// system's appearance from `window`. Does nothing when that theme is already
/// loaded, so it can run on every render.
pub fn apply(family: ThemeFamily, appearance: Appearance, window: &Window, cx: &mut App) {
  let mode = mode(appearance, window.appearance());
  let (light, dark) = members(family);
  let wanted = if mode.is_dark() { dark } else { light };
  let theme = cx.theme();

  if theme.mode != mode || theme.theme_name() != wanted.name {
    change(family, mode, cx);
  }
}

/// A size the spec gives in pixels, scaled with the interface size. Spec
/// pixels assume 14px body text, the rem, so this is that many fourteenths
/// of a rem.
///
/// ```
/// use gpui_kit::{px, rems};
/// use rivet_app::theme;
///
/// assert_eq!(theme::spec(21.), rems(1.5));
/// assert_eq!(theme::spec(21.).to_pixels(px(28.)), px(42.));
/// ```
pub fn spec(pixels: f32) -> Rems {
  rems(pixels / BODY_TEXT_SIZE)
}

/// The factor an interface size of `percent` scales the spec's sizes by,
/// clamped to the sizes settings offer.
///
/// ```
/// use rivet_app::theme;
///
/// assert_eq!(theme::scale(150), 1.5);
/// assert_eq!(theme::scale(400), 1.5);
/// ```
pub fn scale(percent: u16) -> f32 {
  let (smallest, largest) = (
    INTERFACE_SIZES[0],
    INTERFACE_SIZES[INTERFACE_SIZES.len() - 1],
  );

  f32::from(percent.clamp(smallest, largest)) / 100.0
}

/// Draws the interface at `percent` of the spec's sizes, clamped to the
/// sizes settings offer, by setting the rem that [`spec`] sizes and gpui's
/// relative sizes are measured in. Does nothing when it already is, so it can
/// run on every render.
pub fn apply_size(percent: u16, window: &mut Window, cx: &mut App) {
  let scale = scale(percent);
  let body = px(BODY_TEXT_SIZE * scale);

  if cx.theme().font_size != body {
    Theme::update(cx, |theme| {
      theme.font_size = body;
      theme.mono_font_size = px(MONO_TEXT_SIZE * scale);
    });
  }

  window.set_rem_size(body);
}

/// The interface size, as a percentage, for a display `display` big at
/// `scale_factor`. A display the OS scales gets 100%, since the OS already
/// made it readable; an unscaled one gets more the more pixels its shorter
/// side has.
///
/// ```
/// use gpui_kit::{px, size};
/// use rivet_app::theme;
///
/// assert_eq!(theme::automatic_size(1.0, size(px(1920.), px(1080.))), 100);
/// assert_eq!(theme::automatic_size(1.0, size(px(3840.), px(2160.))), 150);
/// ```
pub fn automatic_size(scale_factor: f32, display: Size<Pixels>) -> u16 {
  let shorter = f32::from(display.width.min(display.height));

  match shorter {
    _ if scale_factor > 1.0 => 100,
    2160.0.. => 150,
    1800.0.. => 130,
    1440.0.. => 115,
    _ => 100,
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
