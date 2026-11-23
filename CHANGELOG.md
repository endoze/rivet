# Changelog

All notable changes to this project will be documented in this file.

## [0.5.0](https://github.com/endoze/rivet/compare/v0.4.0...v0.5.0) - 2026-11-23

### Added

- *(picker)* let users star favorite games

### Other

- update Cargo.toml dependencies

## [0.4.0](https://github.com/endoze/rivet/compare/v0.3.0...v0.4.0) - 2026-11-18

### Added

- open thunderstore install links in rivet
- open .r2z profile exports in rivet
- hand a second launch to the running rivet
- publish a Linux AppImage that updates itself
- add rivet to the windows start menu on start
- *(platform)* hide the linux menu entry once rivet is deleted

## [0.3.0] - 2026-11-17

### Features

- Give rivet an app icon on windows and linux


## [0.2.0] - 2026-11-16

### Features

- Ask before downloading a rivet update


## [0.1.0] - 2026-11-16

### Bug Fixes

- Make the mod lists readable
- Show the steam launch option banner only when it needs attention
- Run on windows from a linux-built exe
- Open no console window on windows
- Lead with pinned mods only when the search is empty

### Documentation

- Add glossary and first two ADRs
- License rivet under MIT and add a readme for players
- Show what rivet looks like in the readme

### Features

- Add the four-crate workspace skeleton
- Add the game picker and remembered game folder
- Browse a game's packages
- Install a package with its dependencies
- Launch a native game modded or vanilla
- Find a Steam game's folder by itself
- Search, sort, and filter the package list
- Manage a game's profiles from an always-visible switcher
- Manage installed packages in an installed tab
- Preview a package's readme beside the list
- Run profile changes through an operation queue
- Rank package search by best match
- Cancel and track progress for every queued operation
- Show notices as toast notifications
- Organize profiles in a header switcher
- Confirm installs in the shared modal
- Stop changes from shifting the layout
- Launch proton games on linux
- Badge and update outdated mods
- Add a settings screen with launch arguments
- Export and import profiles
- Let a game be forced to run natively or under proton
- Drop the missing loader warning from the layout
- Build and test windows in ci and label it experimental
- Remove the loader files launches copied into a game folder
- Choose a mod's version and pin it
- Offer to import r2modman profiles
- Drop the game folder line from the game screen
- Give rivet its own theme and bundled IBM Plex fonts
- Move the game, launch and profiles into a sidebar
- Put the list's controls in a toolbar with category chips
- Give mod rows icons, state tags and an on/off switch
- Turn the readme panel into a resizable detail pane
- Find r2modman profiles in every folder r2modman uses
- Clicking a profile shows its mods
- Choose light or dark in settings
- Replace category chips with a filter menu
- Make the selected segment stand out
- Move profile actions to a right-click menu
- Set new and import apart from the profile list
- Add One Light and One Dark themes
- Pick a readable interface size for the display
- Show every profile's mod count without opening it
- Show game covers as a list or a grid
- Open a game on its profile's mods
- Reopen each game on the view it was left on
- Show every find mods row's install button
- Offer update all at the top of the profile's mods
- Uninstall a mod from its row's right-click menu
- Say plainly when rivet can't install a game's mods
- Let rivet update itself from github releases
- Keep the toolbar's sort and refresh beside filter

### Miscellaneous Tasks

- Release with release-plz, git-cliff and dist

### Refactor

- Name the project Rivet and prefix its crates

### Build

- Trim dev debug info to cut build size
- Add a windows cross-compile dev shell with wine

