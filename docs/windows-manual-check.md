# Windows manual check

The Windows build says "Experimental Windows build" in its status bar until someone has gone through this list by hand on real Windows. CI builds and tests Windows on every change but cannot start the app or a game.

Use a Windows 10 or 11 machine with Steam signed in, a game Thunderstore supports installed (Valheim or Lethal Company), and a second Steam library on another drive if you can. Build the exe with `nix run .#windows-vm`, or with `cargo xwin build -p rivet-app --release --target x86_64-pc-windows-msvc` in `nix develop .#windows`.

## The list

- [ ] Double-clicking `rivet.exe` opens Rivet with no console window beside it.
- [ ] Picking the game finds its folder with no prompt, including a game in the second library.
- [ ] With Steam installed outside `C:\Program Files (x86)\Steam`, Rivet still finds it.
- [ ] Settings are saved to `%APPDATA%\rivet\settings.toml`, the package index to `%LOCALAPPDATA%\rivet\cache`, and profiles to `%LOCALAPPDATA%\rivet\data`. Closing and reopening Rivet keeps the game, profile, and settings.
- [ ] Installing a mod pulls in its dependencies and the loader. Enabling, disabling, updating, and uninstalling it work.
- [ ] Launching modded copies the loader into the game folder and the game starts with the mod loaded.
- [ ] Launching vanilla afterwards starts the game without mods.
- [ ] Exporting a profile as a code and as a file, then importing each, gives the same mods.
- [ ] Closing Rivet while a change runs asks first.

## When every item passes

Remove the label: drop the `experimental` element from `render_status_bar` in `crates/rivet-app/src/lib.rs`, `text::EXPERIMENTAL`, and the test that checks for it. Record the Windows version and the date in the commit message.
