# Architecture overview (v0.1.0)

```
Capability Packages
        │
        ▼
 Package Loader (reforge-capability)
        │
        ▼
 Dependency / Contract checks
        │
        ▼
 Composition Compiler (reforge-compose) → Application IR
        │
        ▼
 App Runtime (reforge-runtime) ── SQLite persist
        │
   ┌────┴────┐
   ▼         ▼
 Verify    Export (vendored crates + runner)
   │
 Studio Lite / CLI
```

## Crates

| Crate | Role |
| --- | --- |
| `reforge-types` | Shared models |
| `reforge-canon` | Canonical JSON + hashing |
| `reforge-expr` | Safe expression eval |
| `reforge-behavior` | Twin engine |
| `reforge-capability` | Package load/inspect/export |
| `reforge-evidence` | Provenance + CAS |
| `reforge-dependency` | Graphs / compatibility |
| `reforge-schema` | Migrations |
| `reforge-compose` | Manifest → Application IR |
| `reforge-runtime` | Multi-cap execution |
| `reforge-verify` | Composition expectations |
| `reforge-export` | Standalone project export |
| `reforge-studio` | Local UI |
| `reforge-cli` | `reforge` binary |
| `reforge` | Public facade |

Phase 1 crates remain the semantic foundation; Phase 2 adds composition without inventing missing contracts.
