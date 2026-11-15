# Phase 11 untouched target selection

This document was written before the suite was frozen and before any Phase 11
reconstruction run reached either application. Neither application or version
appears in `benchmarks/`, `fixtures/` or any earlier Phase 6-10 suite.

## Target registry audit

`TARGET_EXCLUSION.json` records every family already used by ReForge development
or tuning: final2-final8, Phase 6.3, the causal holdout, the Phase 6.4 external
tools (hledger, topydo, SQLite), Phase 6.5 notecli, Phase 6.6 parcel/vault,
Phase 7.1 json-server/todo.txt-cli, Phase 7.4 Taskbook, and the Phase 8 pair
(npm-cli 11.20.0, buku 5.1). A version change inside an excluded family is not
an independent target.

## Selected targets

1. **changesets 2.27.9 (`@changesets/cli`)** — JavaScript/Node release planner.
   The harness invokes its public CLI against a single-package workspace. State
   is the package manifest version, the pending changeset release plan, the
   prerelease mode file and the generated changelog. It has generated identities
   (changeset files, release entries), derived state (the version implied by the
   strongest pending release type, the changelog sections), ordering (release
   consumption), validation (prerelease transitions outside their mode) and
   restart-visible file state.
2. **dbmate 2.20.0** — a Go schema-migration CLI over SQLite. The harness runs
   its public CLI over a pristine migration directory. State is the SQLite
   database: the `schema_migrations` ledger, the applied/pending ordering, the
   produced schema, the seeded row with its generated integer identity, and the
   presence of the database file itself. It has create/apply/rollback/drop
   operations, a rejection when nothing has been applied, derived counts, and
   restart persistence.

The pair differs in implementation language (JavaScript vs Go), distribution
model (npm package vs static binary), persistence model (JSON/Markdown files and
a manifest vs an SQLite database) and domain (release planning vs schema
migration). Neither is a clone of a previously used application.

## Access boundary

The adapters invoke only installed public executables and observe persisted
state. They import no target source. Hidden workflows are never visible to the
investigator. Action schemas are parameter-free and were written by hand from
public CLI behavior before the freeze.

## Known normalization decisions

- changeset file names are randomly generated words. The adapter exposes the
  release plan and cardinality, not the generated names; names carry no
  semantic content. Summaries, release types, versions and changelog sections
  are reported verbatim.
- the migration ledger's `applied_at` timestamp is excluded from the normalized
  state; migration versions, the produced schema and seeded rows are reported.
- dbmate opens (and therefore creates) the SQLite file before validating a
  rollback. That observed file-creation side effect is recorded by the preflight
  rather than treated as a state mutation.

## Risk note

Both applications are developer-workflow tools, so this pair is narrower than a
cross-domain pair would be. It was chosen for deterministic observations and a
reproducible offline reference lifecycle, and the limitation is carried into the
Phase 11 report rather than hidden.
