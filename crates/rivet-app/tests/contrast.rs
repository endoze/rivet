use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode};
use gpui_kit::{TestAppContext, rgb};
use rivet_app::palette::{Palette, contrast};

/// WCAG 2 level AA for normal-size text.
const AA: f32 = 4.5;
/// WCAG 2 level AA for icons and outlines.
const NON_TEXT: f32 = 3.0;

#[test]
fn contrast_matches_the_wcag_reference_values() {
  let black = rgb(0x000000).into();
  let white = rgb(0xffffff).into();

  assert!((contrast(black, white) - 21.0).abs() < 0.01);
  assert!((contrast(white, black) - 21.0).abs() < 0.01);
  assert!((contrast(rgb(0x767676).into(), white) - 4.54).abs() < 0.01);
  assert!((contrast(rgb(0x777777).into(), white) - 4.48).abs() < 0.01);
}

#[gpui_kit::test]
fn every_text_is_readable_in_the_light_theme(cx: &mut TestAppContext) {
  assert_readable(cx, ThemeMode::Light);
}

#[gpui_kit::test]
fn every_text_is_readable_in_the_dark_theme(cx: &mut TestAppContext) {
  assert_readable(cx, ThemeMode::Dark);
}

#[test]
fn the_shell_paints_only_palette_colors() {
  let sources = [
    ("lib.rs", include_str!("../src/lib.rs")),
    ("icons.rs", include_str!("../src/icons.rs")),
    ("text.rs", include_str!("../src/text.rs")),
  ];

  for (file, source) in sources {
    for literal in ["rgb(", "rgba(", "hsla(", "hsl("] {
      assert!(
        !source.contains(literal),
        "{file} builds a color with {literal}), so it can leave the theme; add it to the palette"
      );
    }
  }
}

fn assert_readable(cx: &mut TestAppContext, mode: ThemeMode) {
  cx.update(gpui_kit::init);
  cx.update(|cx| Theme::change(mode, None, cx));

  let palette = cx.update(|cx| {
    assert_eq!(cx.theme().is_dark(), mode.is_dark());

    Palette::new(cx)
  });
  let texts = [("text", palette.text), ("muted text", palette.muted_text)];
  let backgrounds = [
    ("the panel", palette.panel),
    ("a hovered row", palette.row_hover),
    ("the selected row", palette.row_selected),
    ("the launch option line", palette.code),
  ];

  for (text_name, text) in texts {
    for (background_name, background) in backgrounds {
      let ratio = contrast(text, background);

      assert!(
        ratio >= AA,
        "{mode:?}: {text_name} on {background_name} is {ratio:.2}:1, under {AA}:1"
      );
    }
  }

  let toast_text = contrast(palette.text, palette.toast);

  assert!(
    toast_text >= AA,
    "{mode:?}: text on a toast is {toast_text:.2}:1, under {AA}:1"
  );

  let accents = [
    ("info", palette.info),
    ("success", palette.success),
    ("warning", palette.warning),
    ("error", palette.error),
  ];

  for (name, accent) in accents {
    let ratio = contrast(accent, palette.toast);

    assert!(
      ratio >= NON_TEXT,
      "{mode:?}: the {name} toast's icon is {ratio:.2}:1 on the toast, under {NON_TEXT}:1"
    );

    for (other_name, other) in accents {
      assert!(
        name == other_name || accent != other,
        "{mode:?}: the {name} and {other_name} toasts look alike"
      );
    }
  }

  assert_ne!(
    palette.row_selected, palette.row_hover,
    "{mode:?}: the selected row looks hovered"
  );
  assert_ne!(
    palette.row_selected, palette.panel,
    "{mode:?}: the selected row looks unselected"
  );
}
