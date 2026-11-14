# Rivet

A desktop app for installing and launching Thunderstore mods, across every game Thunderstore supports.

## Language

**Package**:
A versioned unit published on Thunderstore. It may be a mod, a mod loader, or a modpack that only lists dependencies. Interface text shown to users says "mod".
_Avoid_: Mod (outside interface text), plugin, addon

**Profile**:
A named, self-contained set of installed packages for one game. A game has many profiles and exactly one is selected at a time.
_Avoid_: Modpack, loadout, instance

**Dependency**:
A package that another package requires in order to work.

**Dependant**:
An installed package that requires a given package. The reverse of a dependency.
_Avoid_: Dependent, child, consumer

**Pin**:
A user's choice to hold a package at a specific version in one profile. A pinned package is skipped by "update all" until it is unpinned.
_Avoid_: Lock, freeze

**Operation**:
One user-requested change to a profile: an install, update, uninstall, enable, disable, or import. A profile runs one operation at a time and queues the rest.
_Avoid_: Task, job, action
