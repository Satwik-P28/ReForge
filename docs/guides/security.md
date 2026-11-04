# Security model

- Packages and manifests are untrusted.
- Inspect/validate do not execute package `impl/` trees.
- Studio refuses non-loopback binds.
- Event propagation is depth-limited.
- Expression evaluation is depth-limited.
- Export path copies must not follow `..` escapes inside walked trees.
- Prefer reviewing vendored crate licenses before redistributing exports.

See also `SECURITY.md` and `docs/adr/0008-security-boundaries.md`.
