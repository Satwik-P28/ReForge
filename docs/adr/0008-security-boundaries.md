# ADR 0008: Security boundaries

## Status

Accepted (Phase 1)

## Context

Package directories may be untrusted. A semantic foundation that “helpfully” executes package code, follows `../` paths, or evaluates arbitrary expressions would become an attack surface before composition exists.

## Decision

Establish these Phase 1 boundaries:

1. **No package code execution** during `load` / `validate` / `inspect` / `export`. `impl/` paths are assets/metadata only.
2. **Path confinement**: reject `..` and absolute paths in implementation refs and payload refs; resolve loads under the package root.
3. **Constrained expressions only** — no `eval`, JS, shell, or callbacks (`reforge-expr`).
4. **Host builtins** (`notes_ref`, `tasks_ref`) are compiled host registrations, not downloaded plugins.
5. **CAS integrity** for evidence bytes; corruption/missing are explicit statuses.
6. **Authorization is explicit** on `SoftwareArtifact.permitted_use` (defaults unauthorized).
7. **Certainty / evidence hierarchy** must not be presented as cryptographic trust.

CLI and library share these rules; adversarial tests cover unsafe paths and unsupported expressions.

## Consequences

**Positive**

- Clear story for Composer authors: inspection is safe relative to package trees.
- Reduces accidental RCE class bugs in Phase 1.
- Forces honesty about what is modeled vs executed.

**Negative / tradeoffs**

- Cannot ship “bring your own interpreter” inside packages yet.
- Host builtins remain fully trusted — compromise of the host binary is still total.
- No sandbox, seccomp, or capability dropping for the CLI process itself.
- No package signatures / TUF / Sigstore in Phase 1.

## Alternatives considered

- WASM guest plugins per package: deferred; needs a stronger sandbox design.
- Allow relative escapes for convenience: rejected.
- Trust all JSON as executable config for host commands: rejected.
