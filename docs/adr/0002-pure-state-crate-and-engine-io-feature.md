# The pure crate cannot call engine IO, enforced by an engine feature flag

Functional core, imperative shell is the one absolute rule of this project. All app state and every decision live in a crate that has no dependency on gpui or Tokio. To let that crate use thunderstore-engine's data models, dependency resolver, game definitions, and queries without being able to call its network or disk code, the engine puts everything that does IO behind a default-on `io` feature, and the pure crate depends on the engine with default features off. CI tests the pure crate on its own, so a call to an engine IO function fails to compile.

Gaps in the engine that any Thunderstore client would hit are fixed in the engine and released. This workspace does not work around them.

## Considered options

- **Split the engine into a pure crate and an IO crate.** First chosen, then dropped. It costs a second crate to publish and version forever, and it moves public items: a method cannot live in a different crate from its type, and the error type wraps IO errors. The guarantee it buys is no stronger, because no crate layout stops Rust code from using `std::fs`.
- **The pure crate owns its own copies of the engine's types.** No engine change, but every type is mapped twice and the engine's pure queries cannot be called from the core.
- **Depend on the full engine and rely on review.** No engine change, but nothing fails when the rule is broken, and the pure crate's tests would build the HTTP and TLS stack.

## Consequences

- The guarantee holds only when the pure crate is built on its own, since Cargo merges features across everything built together. The CI step that tests it alone is what enforces the rule and must not be removed.
- The feature is on by default, so existing clients such as valheim-mod-manager are unaffected.
