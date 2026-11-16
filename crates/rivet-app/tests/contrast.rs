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

#[gpui_kit::test]
fn the_selected_segment_stands_out_in_the_light_theme(cx: &mut TestAppContext) {
  assert_selected_segment_stands_out(cx, ThemeMode::Light);
}

#[gpui_kit::test]
fn the_selected_segment_stands_out_in_the_dark_theme(cx: &mut TestAppContext) {
  assert_selected_segment_stands_out(cx, ThemeMode::Dark);
}

#[test]
fn the_shell_paints_only_palette_colors() {
  let sources = [
    ("lib.rs", include_str!("../src/lib.rs")),
    ("icons.rs", include_str!("../src/icons.rs")),
    ("segmented.rs", include_str!("../src/segmented.rs")),
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

/// Asserts the ticket's rule for a segmented control: the selected
/// segment's indicator is at least [`NON_TEXT`] on the unselected background
/// and on the selected fill, its label is the full text color, and every
/// label is readable on what it sits on.
fn assert_selected_segment_stands_out(cx: &mut TestAppContext, mode: ThemeMode) {
  cx.update(gpui_kit::init);
  cx.update(rivet_app::theme::init);
  cx.update(|cx| Theme::change(mode, None, cx));

  let palette = cx.update(|cx| Palette::new(cx));
  let segments = palette.segments;

  for (name, background) in [
    ("the unselected segments", segments.unselected),
    ("the selected segment", segments.selected),
  ] {
    let ratio = contrast(segments.indicator, background);

    assert!(
      ratio >= NON_TEXT,
      "{mode:?}: the selected segment's indicator is {ratio:.2}:1 on {name}, under {NON_TEXT}:1"
    );
  }

  assert_eq!(
    segments.selected_text, palette.text,
    "{mode:?}: the selected segment's label is not the full text color"
  );
  assert_ne!(
    segments.unselected_text, segments.selected_text,
    "{mode:?}: the unselected segments' labels look selected"
  );

  for (name, text, background) in [
    ("selected", segments.selected_text, segments.selected),
    ("unselected", segments.unselected_text, segments.unselected),
  ] {
    let ratio = contrast(text, background);

    assert!(
      ratio >= AA,
      "{mode:?}: a {name} segment's label is {ratio:.2}:1, under {AA}:1"
    );
  }
}

fn assert_readable(cx: &mut TestAppContext, mode: ThemeMode) {
  cx.update(gpui_kit::init);
  cx.update(rivet_app::theme::init);
  cx.update(|cx| Theme::change(mode, None, cx));

  let (palette, primary, primary_text) = cx.update(|cx| {
    let theme = cx.theme();

    assert_eq!(theme.is_dark(), mode.is_dark());

    (Palette::new(cx), theme.primary, theme.primary_foreground)
  });
  let primary_button = contrast(primary_text, primary);

  assert!(
    primary_button >= AA,
    "{mode:?}: text on a primary button is {primary_button:.2}:1, under {AA}:1"
  );

  let texts = [("text", palette.text), ("muted text", palette.muted_text)];
  let backgrounds = [
    ("the window", palette.background),
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

  let tags = [
    ("update", palette.tag_update),
    ("pinned", palette.muted_text),
    ("deprecated", palette.tag_deprecated),
    ("missing dependency", palette.tag_missing),
    ("mod loader", palette.tag_loader),
  ];

  for (name, tag) in tags {
    for (background_name, background) in &backgrounds[..4] {
      let ratio = contrast(tag, *background);

      assert!(
        ratio >= AA,
        "{mode:?}: the {name} tag on {background_name} is {ratio:.2}:1, under {AA}:1"
      );
    }

    for (other_name, other) in tags {
      assert!(
        name == other_name || tag != other,
        "{mode:?}: the {name} and {other_name} tags look alike"
      );
    }
  }

  let toast_text = contrast(palette.text, palette.toast);

  assert!(
    toast_text >= AA,
    "{mode:?}: text on a toast is {toast_text:.2}:1, under {AA}:1"
  );

  let danger_text = contrast(palette.danger_text, palette.toast);

  assert!(
    danger_text >= AA,
    "{mode:?}: a destructive menu item is {danger_text:.2}:1, under {AA}:1"
  );
  assert_ne!(
    palette.danger_text, palette.text,
    "{mode:?}: a destructive menu item looks like any other"
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
