# Use gpui for the UI, through an exact gpui-kit version

The app's UI is built on gpui, taken as the single dependency `gpui-kit` at an exact version (`=0.9.1` at the time of writing). gpui-kit bundles a weekly snapshot of gpui (`gpui-pre`) with the `gpui-component` widget library, which supplies the text input, markdown, tabs, dialogs and menus that gpui itself lacks. We chose it because gpui lets all app state live in a plain struct updated by a pure function, with views as a thin shell, and a spike confirmed that on Linux with 14,656 mods.

The snapshot only changes when we raise the version, so upgrades are deliberate.

## Considered options

- **A pinned git revision of Zed.** This is what Zed staff recommend, but `gpui-component` depends on the snapshot crate under a different name, so we would have to write every widget ourselves.
- **The official `gpui` crate on crates.io.** It has been frozen at 0.2.2 since October 2025 and predates the current Linux renderer.
- **iced, egui, Slint, Dioxus, Tauri.** Not evaluated beyond their READMEs. Tauri is what Gale, the existing Rust mod manager, uses.

## Consequences

- Both the gpui snapshot and the widget library are published by one third party (Longbridge) and are pre-1.0 with breaking changes. If they stop, the fallback is a pinned Zed revision plus our own widgets.
- gpui has no CPU renderer. Without a working GPU the app is slow on Linux and may not run on Windows. A GPU is a requirement.
- Zed's own `ui` and `markdown` crates are GPL-3.0 and must not be used or copied.
- gpui does not register URL schemes on Windows or Linux and has no system tray or installer bundler. Those are ours to build.
- Each OS needs its own build machine. Windows was not tested by the spike.

The spike is saved on the `prototype/gpui-spike` bookmark.
