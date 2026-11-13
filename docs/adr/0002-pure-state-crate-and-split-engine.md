# The pure crate cannot do IO, and thunderstore-engine is split to allow it

Functional core, imperative shell is the one absolute rule of this project, and we enforce it with crate boundaries. All app state and every decision live in a crate that has no dependency on gpui, Tokio, the filesystem, or the network. To let that crate use the engine's data models, dependency resolver, game definitions, and queries, thunderstore-engine is split into two published crates: a pure one named `thunderstore-engine-core`, and the existing one that keeps everything touching disk or network and re-exports the pure one.

Gaps in the engine that any Thunderstore client would hit are fixed in the engine and released. This workspace does not work around them.

## Considered options

- **One engine crate with an optional IO feature.** Less restructuring, but Cargo merges features across a workspace, so the guarantee would rest on a CI check.
- **The pure crate owns its own copies of the engine's types.** No engine change, but every type is mapped twice and the engine's pure queries cannot be called from the core.
