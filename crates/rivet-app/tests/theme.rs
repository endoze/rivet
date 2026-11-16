use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode};
use gpui_kit::{App, Hsla, TestAppContext, WindowAppearance, rgb, rgba};
use rivet_app::palette::Palette;
use rivet_app::theme;
use rivet_core::settings::Appearance;

/// One appearance's row of the spec's design token table.
struct Spec {
  background: u32,
  panel: u32,
  raised: u32,
  line: u32,
  text: u32,
  muted: u32,
  brass: u32,
  brass_ink: u32,
  /// Info at the spec's opacity, as `0xrrggbbaa`.
  selection: u32,
  success: u32,
  warning: u32,
  danger: u32,
  info: u32,
}

const DARK: Spec = Spec {
  background: 0x1a1f27,
  panel: 0x212833,
  raised: 0x2a3240,
  line: 0x364052,
  text: 0xe6e9ef,
  muted: 0x9aa4b5,
  brass: 0xd4a84a,
  brass_ink: 0x1a1f27,
  selection: 0x6fa8e824,
  success: 0x6cc08b,
  warning: 0xe8925a,
  danger: 0xe86a6a,
  info: 0x6fa8e8,
};

const LIGHT: Spec = Spec {
  background: 0xeef1f5,
  panel: 0xffffff,
  raised: 0xf5f7fa,
  line: 0xd8dee7,
  text: 0x1c2330,
  muted: 0x5b6577,
  brass: 0x946f1c,
  brass_ink: 0xffffff,
  selection: 0x2f6db51a,
  success: 0x2e8a52,
  warning: 0xb85a1e,
  danger: 0xc23b3b,
  info: 0x2f6db5,
};

#[gpui_kit::test]
fn the_dark_theme_paints_the_spec_tokens(cx: &mut TestAppContext) {
  assert_spec_tokens(cx, ThemeMode::Dark, &DARK);
}

#[gpui_kit::test]
fn the_light_theme_paints_the_spec_tokens(cx: &mut TestAppContext) {
  assert_spec_tokens(cx, ThemeMode::Light, &LIGHT);
}

#[gpui_kit::test]
fn only_the_primary_action_is_brass_in_the_dark_theme(cx: &mut TestAppContext) {
  assert_brass_only_on_primary(cx, ThemeMode::Dark, &DARK);
}

#[gpui_kit::test]
fn only_the_primary_action_is_brass_in_the_light_theme(cx: &mut TestAppContext) {
  assert_brass_only_on_primary(cx, ThemeMode::Light, &LIGHT);
}

#[gpui_kit::test]
fn both_themes_set_text_in_ibm_plex(cx: &mut TestAppContext) {
  for mode in [ThemeMode::Light, ThemeMode::Dark] {
    with_rivet_theme(cx, mode, |cx| {
      let theme = cx.theme();

      assert_eq!(theme.font_family, theme::SANS, "{mode:?}");
      assert_eq!(theme.mono_font_family, theme::MONO, "{mode:?}");
    });
  }
}

#[test]
fn match_system_follows_the_os_and_light_or_dark_ignores_it() {
  let cases = [
    (
      Appearance::System,
      WindowAppearance::Light,
      ThemeMode::Light,
    ),
    (
      Appearance::System,
      WindowAppearance::VibrantLight,
      ThemeMode::Light,
    ),
    (Appearance::System, WindowAppearance::Dark, ThemeMode::Dark),
    (
      Appearance::System,
      WindowAppearance::VibrantDark,
      ThemeMode::Dark,
    ),
    (Appearance::Light, WindowAppearance::Dark, ThemeMode::Light),
    (Appearance::Dark, WindowAppearance::Light, ThemeMode::Dark),
  ];

  for (appearance, system, expected) in cases {
    assert_eq!(
      theme::mode(appearance, system),
      expected,
      "{appearance:?} on a {system:?} system"
    );
  }
}

#[test]
fn small_text_uses_the_spec_size_not_the_rem_scale() {
  let sources = [
    ("lib.rs", include_str!("../src/lib.rs")),
    ("modal.rs", include_str!("../src/modal.rs")),
    ("text.rs", include_str!("../src/text.rs")),
  ];

  for (file, source) in sources {
    for scale in [".text_xs()", ".text_sm()"] {
      assert!(
        !source.contains(scale),
        "{file} sizes text with {scale}, a fraction of the 14px body; use text_small for the spec's 13px"
      );
    }
  }
}

#[cfg(target_os = "linux")]
#[test]
fn the_bundled_fonts_load_without_any_system_font() {
  use gpui_kit::TestDispatcher;
  use gpui_wgpu::CosmicTextSystem;
  use std::sync::Arc;

  let no_system_fonts = Arc::new(CosmicTextSystem::new_without_system_fonts("no fallback"));
  let cx = TestAppContext::build_with_text_system(TestDispatcher::new(0), None, no_system_fonts);

  cx.update(gpui_kit::init);
  cx.update(theme::init);

  let families = cx.update(|cx| cx.text_system().all_font_names());

  assert_eq!(families, [theme::MONO, theme::SANS]);

  cx.update(|cx| cx.quit());
}

fn assert_spec_tokens(cx: &mut TestAppContext, mode: ThemeMode, spec: &Spec) {
  with_rivet_theme(cx, mode, |cx| {
    let theme = cx.theme();
    let palette = Palette::new(cx);
    let tokens = [
      ("background", theme.background, spec.background),
      ("palette background", palette.background, spec.background),
      ("sidebar", theme.sidebar, spec.panel),
      ("status bar", theme.status_bar, spec.panel),
      ("popover", theme.popover, spec.panel),
      ("palette panel", palette.panel, spec.panel),
      ("palette toast", palette.toast, spec.panel),
      ("secondary button", theme.secondary, spec.raised),
      ("button", theme.button, spec.raised),
      ("list hover", theme.list_hover, spec.raised),
      ("palette row hover", palette.row_hover, spec.raised),
      ("palette code", palette.code, spec.raised),
      ("border", theme.border, spec.line),
      ("input border", theme.input, spec.line),
      ("palette border", palette.border, spec.line),
      ("text", theme.foreground, spec.text),
      ("palette text", palette.text, spec.text),
      ("muted text", theme.muted_foreground, spec.muted),
      ("palette muted text", palette.muted_text, spec.muted),
      ("primary", theme.primary, spec.brass),
      ("primary text", theme.primary_foreground, spec.brass_ink),
      ("success", theme.success, spec.success),
      ("palette success", palette.success, spec.success),
      ("warning", theme.warning, spec.warning),
      ("palette warning", palette.warning, spec.warning),
      ("danger", theme.danger, spec.danger),
      ("palette error", palette.error, spec.danger),
      ("palette danger text", palette.danger_text, spec.danger),
      ("info", theme.info, spec.info),
      ("palette info", palette.info, spec.info),
    ];

    for (name, actual, expected) in tokens {
      assert_close(mode, name, actual, rgb(expected).into());
    }

    let selections = [
      ("selection", theme.selection),
      ("selected list item", theme.list_active),
    ];

    for (name, actual) in selections {
      assert_close(mode, name, actual, rgba(spec.selection).into());
    }

    let selected_row = Hsla::from(rgb(spec.background)).blend(rgba(spec.selection).into());

    assert_close(
      mode,
      "palette selected row",
      palette.row_selected,
      selected_row,
    );
  });
}

fn assert_brass_only_on_primary(cx: &mut TestAppContext, mode: ThemeMode, spec: &Spec) {
  with_rivet_theme(cx, mode, |cx| {
    let theme = cx.theme();
    let brass = Hsla::from(rgb(spec.brass));
    let others = [
      ("link", theme.link),
      ("focus ring", theme.ring),
      ("caret", theme.caret),
      ("selection", theme.selection),
      ("selected list item", theme.list_active),
      ("selected list item border", theme.list_active_border),
      ("menu hover", theme.accent),
      ("secondary button", theme.secondary),
      ("button", theme.button),
      ("drag border", theme.drag_border),
      ("drop target", theme.drop_target),
      ("sidebar hover", theme.sidebar_accent),
    ];

    for (name, color) in others {
      assert!(
        distance(color.opacity(1.), brass) > 0.1,
        "{mode:?}: {name} is brass, which only the primary action may be"
      );
    }
  });
}

fn with_rivet_theme(cx: &mut TestAppContext, mode: ThemeMode, check: impl FnOnce(&App)) {
  cx.update(gpui_kit::init);
  cx.update(theme::init);
  cx.update(|cx| Theme::change(mode, None, cx));
  cx.update(|cx| check(cx));
}

fn assert_close(mode: ThemeMode, name: &str, actual: Hsla, expected: Hsla) {
  assert!(
    distance(actual, expected) < 1.0 / 255.0 && (actual.a - expected.a).abs() < 1.0 / 255.0,
    "{mode:?}: {name} is {:?}, the spec says {:?}",
    actual.to_rgb(),
    expected.to_rgb()
  );
}

/// The largest difference between two colors on any of red, green, and blue.
fn distance(a: Hsla, b: Hsla) -> f32 {
  let (a, b) = (a.to_rgb(), b.to_rgb());

  [(a.r - b.r), (a.g - b.g), (a.b - b.b)]
    .into_iter()
    .map(f32::abs)
    .fold(0.0, f32::max)
}
