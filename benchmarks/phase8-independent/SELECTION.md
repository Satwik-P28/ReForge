# Phase 8 target selection

This selection was written before the suite was frozen or any reconstruction
run was made.

## Independent targets

1. **npm CLI 11.20.0** — a real JavaScript/Node application with a large
   analyzable source tree. The harness runs local-file package installation,
   uninstall, version mutation, pruning and dependency listing in an isolated
   workspace. State is observed from npm's own installed graph and lockfile.
2. **buku 5.1** — a separately developed Python/SQLite bookmark manager. The
   harness invokes its public CLI and observes the persisted SQLite rows:
   generated indices, tags, title updates, duplicate rejection and deletion.

The applications differ in language, storage model, identity allocation and
observable output family. Both have multi-step stateful behavior, generated or
persistent identities, relationships, validation behavior and restart behavior.

## Exclusion

All targets used by final2–final8, Phase 6.3, hledger, topydo, SQLite, notecli,
Taskbook, JSON-server, todo.txt-cli, Phase 6.6 parcel/vault and the causal
holdout are excluded. The exact family list and reasons are in
`TARGET_EXCLUSION.json`. A version change within an excluded family is not an
independent target.

## Access boundary

The adapters invoke only installed public executables and observe persisted
state. They do not import target source. The npm source tree is available only
to the explicit REA arm; the black-box arm receives no source path. Hidden
workflows are not visible to the investigator.
