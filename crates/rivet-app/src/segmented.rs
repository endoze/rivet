//! The segmented control every choice between a few options uses, so the
//! selected segment looks the same wherever one appears.

use crate::palette::Palette;
use crate::theme::{Typography as _, spec};
use gpui_kit::accesskit::Toggled;
use gpui_kit::component::{ActiveTheme as _, ThemeStyled as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  AnyElement, App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, MouseButton,
  ParentElement as _, RenderOnce, Role, StatefulInteractiveElement as _, Styled as _,
  TestSupportExt as _, Window,
};

/// The height of a segment, in spec pixels.
const SEGMENT_HEIGHT: f32 = 24.0;
/// The space between the control's edge and its segments, in spec pixels.
const INSET: f32 = 2.0;

type OnClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A row of segments, one of them selected. The selected one is filled,
/// underlined, and labelled in the full text color; the rest stay muted.
/// Each segment is a tab stop that shows a focus ring while focused.
#[derive(IntoElement)]
pub struct Segmented {
  id: ElementId,
  segments: Vec<Segment>,
}

/// One option in a [`Segmented`] control.
pub struct Segment {
  id: ElementId,
  label: AnyElement,
  selected: bool,
  on_click: OnClick,
}

impl Segmented {
  /// An empty control with the element id `id`.
  pub fn new(id: impl Into<ElementId>) -> Self {
    Self {
      id: id.into(),
      segments: Vec::new(),
    }
  }

  /// Adds `segment` after the ones already added.
  pub fn child(mut self, segment: Segment) -> Self {
    self.segments.push(segment);

    self
  }
}

impl Segment {
  /// A segment with the element id `id` showing `label`, which calls
  /// `on_click` when clicked, or when Enter or Space is pressed while it has
  /// focus.
  pub fn new(
    id: impl Into<ElementId>,
    label: impl IntoElement,
    selected: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
  ) -> Self {
    Self {
      id: id.into(),
      label: label.into_any_element(),
      selected,
      on_click: Box::new(on_click),
    }
  }
}

impl RenderOnce for Segmented {
  fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
    let colors = Palette::new(cx).segments;
    let radius = cx.theme().radius;

    h_flex()
      .id(self.id)
      .role(Role::RadioGroup)
      .flex_none()
      .p(spec(INSET))
      .gap(spec(INSET))
      .rounded(radius)
      .bg(colors.unselected)
      .text_small()
      .children(self.segments.into_iter().map(|segment| {
        let selected = segment.selected;
        let on_click = segment.on_click;
        let focus_handle = window
          .use_keyed_state(segment.id.clone(), cx, |_, cx| {
            cx.focus_handle().tab_stop(true)
          })
          .read(cx)
          .clone();
        let focused = focus_handle.is_focused(window);

        h_flex()
          .id(segment.id)
          .test_support()
          .track_focus(&focus_handle)
          .role(Role::RadioButton)
          .aria_toggled(if selected {
            Toggled::True
          } else {
            Toggled::False
          })
          .h(spec(SEGMENT_HEIGHT))
          .px_2p5()
          .items_center()
          .rounded(radius - spec(INSET).to_pixels(window.rem_size()))
          .border_b_2()
          .cursor_pointer()
          .map(|this| {
            if selected {
              this
                .bg(colors.selected)
                .border_color(colors.indicator)
                .text_color(colors.selected_text)
            } else {
              this
                .border_color(colors.unselected)
                .text_color(colors.unselected_text)
                .hover(|this| this.text_color(colors.selected_text))
            }
          })
          .when(focused, |this| this.focus_ring_style(window, cx))
          // A click leaves focus where it was, as gpui-component's buttons do.
          .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
          .on_click(move |event, window, cx| on_click(event, window, cx))
          .child(segment.label)
      }))
  }
}
