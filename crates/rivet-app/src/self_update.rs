//! Rivet keeping itself current, as the settings screen and the status bar
//! show it.

use crate::palette::Palette;
use crate::theme::{Typography as _, spec};
use crate::{SETTINGS_LABEL_WIDTH, Shell, text};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::{
  AnyElement, Context, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _,
  Styled as _, TestSupportExt as _, div,
};
use rivet_core::Msg;
use rivet_core::self_update::Stage;

impl Shell {
  /// The settings every game shares about Rivet itself: the version running,
  /// updating automatically, checking for updates by hand with the result
  /// beside the button, which offers the restart or the release page when
  /// there is one, and whether Thunderstore's install links open in Rivet.
  pub(crate) fn render_rivet_settings(&self, cx: &mut Context<Self>) -> AnyElement {
    let palette = Palette::new(cx);
    let state = &self.state;
    let update = &state.self_update;
    let label = |label: &'static str| div().w(spec(SETTINGS_LABEL_WIDTH)).flex_none().child(label);
    let automatic = v_flex()
      .gap_1()
      .child(
        h_flex()
          .gap_3()
          .items_center()
          .child(label(text::UPDATE_AUTOMATICALLY))
          .child(
            Switch::new("update-automatically")
              .small()
              .checked(state.settings.update_automatically)
              .accessibility_label(text::UPDATE_AUTOMATICALLY)
              .on_click(cx.listener(|this, on: &bool, _, cx| {
                this.dispatch(Msg::UpdateAutomaticallyChosen(*on), cx);
              })),
          ),
      )
      .child(
        div()
          .text_small()
          .text_color(palette.muted_text)
          .child(text::UPDATE_AUTOMATICALLY_HINT),
      );
    let opens_links = state
      .install_links
      .unwrap_or(state.settings.install_links == Some(true));
    let links = v_flex()
      .gap_1()
      .child(
        h_flex()
          .gap_3()
          .items_center()
          .child(label(text::INSTALL_LINKS))
          .child(
            Switch::new("install-links")
              .small()
              .checked(opens_links)
              .accessibility_label(text::INSTALL_LINKS)
              .on_click(cx.listener(|this, on: &bool, _, cx| {
                this.dispatch(Msg::InstallLinksChosen(*on), cx);
              })),
          ),
      )
      .child(
        div()
          .text_small()
          .text_color(palette.muted_text)
          .child(text::INSTALL_LINKS_HINT),
      );
    let follow_up = match &update.stage {
      Stage::Ready { .. } => Some(
        Button::new("settings-restart-to-update")
          .label(text::RESTART_TO_UPDATE)
          .small()
          .disabled(!state.can_restart_to_update())
          .on_click(cx.listener(|this, _, _, cx| {
            this.dispatch(Msg::RestartToUpdatePressed, cx);
          })),
      ),
      Stage::Unwritable { page, .. } => {
        let page = page.clone();

        Some(
          Button::new("open-release")
            .label(text::OPEN_RELEASE)
            .small()
            .on_click(cx.listener(move |this, _, _, cx| {
              this.dispatch(Msg::LinkClicked(page.clone()), cx);
            })),
        )
      }
      _ => None,
    };
    let check = h_flex()
      .gap_3()
      .items_center()
      .child(
        div().w(spec(SETTINGS_LABEL_WIDTH)).flex_none().child(
          Button::new("check-for-updates")
            .label(text::CHECK_FOR_UPDATES)
            .small()
            .disabled(!update.can_check())
            .on_click(cx.listener(|this, _, _, cx| {
              this.dispatch(Msg::CheckForUpdatesPressed, cx);
            })),
        ),
      )
      .child(
        div()
          .id("update-result")
          .flex_1()
          .min_w_0()
          .text_small()
          .children(text::update_stage(&update.stage))
          .test_support(),
      )
      .children(follow_up);

    v_flex()
      .gap_3()
      .p_3()
      .border_1()
      .border_color(palette.border)
      .child(div().font_weight(FontWeight::SEMIBOLD).child(text::RIVET))
      .child(
        div()
          .id("rivet-version")
          .child(text::rivet_version(&update.running))
          .test_support(),
      )
      .child(automatic)
      .child(check)
      .child(links)
      .into_any_element()
  }

  /// A newer Rivet in the status bar: its download, with its progress and a
  /// cancel, or once downloaded the restart into it. `None` otherwise.
  pub(crate) fn render_rivet_update_status(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
    let palette = Palette::new(cx);
    let (said, action) = match &self.state.self_update.stage {
      Stage::Downloading {
        download,
        received,
        total,
        cancelling,
      } => (
        text::rivet_downloading(&download.found.version, *received, *total),
        Button::new("cancel-rivet-download")
          .label(text::CANCEL)
          .xsmall()
          .disabled(*cancelling)
          .on_click(cx.listener(|this, _, _, cx| {
            this.dispatch(Msg::RivetDownloadCancelPressed, cx);
          })),
      ),
      Stage::Ready { version, .. } => (
        text::rivet_ready(version),
        Button::new("restart-to-update")
          .label(text::RESTART_TO_UPDATE)
          .xsmall()
          .primary()
          .disabled(!self.state.can_restart_to_update())
          .on_click(cx.listener(|this, _, _, cx| {
            this.dispatch(Msg::RestartToUpdatePressed, cx);
          })),
      ),
      _ => return None,
    };

    let shown = h_flex()
      .id("rivet-update")
      .flex_none()
      .gap_2()
      .items_center()
      .text_small()
      .tabular()
      .child(div().text_color(palette.muted_text).child(said))
      .child(action)
      .test_support();

    Some(shown.into_any_element())
  }
}
