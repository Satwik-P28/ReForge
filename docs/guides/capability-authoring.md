# Capability authoring guide

A Capability Package is a directory with `manifest.json`, usually `twin.json`, and optional `evidence/`.

## External developer checklist

1. Declare commands, queries, events, and errors in `manifest.interfaces`.
2. Provide `contract_inline` invariants and permitted side effects.
3. Implement executable behavior in `twin.json` using the constrained expression language (`map_put`, `map_get`, `map_remove`, `keys`, `list_append`, arithmetic, guards).
4. Avoid `HostBuiltin` implementations for portable packages.
5. Add evidence entries that support key claims.
6. Validate with `reforge package validate <path>` and an e2e test that only uses public `load_package` / `TwinEngine` APIs.

The Labels example (`examples/labels`) was added without modifying compiler domain logic — treat it as the extensibility template.
