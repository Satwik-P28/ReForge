# taskboard.py — CLI reference application (v0.2 investigation fixture)

A small, deterministic, stateful task manager used as an *investigation target*.
It contains no ReForge code and no test hooks: ReForge must discover its
behavior the same way it would discover any authorised CLI program.

## Why it exists

It gives the investigation engine a target with:

- **Real state**: a `state.json` file that persists between invocations.
- **Prerequisite-gated actions**: `archive` only becomes available after two
  tasks are completed; `purge` only appears in the archive view. These are the
  "hidden behaviors" active exploration has to find.
- **Guarded rejection semantics**: `note` requires the detail view, `delete`
  is rejected while notes are attached, `done` is rejected twice.
- **An honest affordance report**: each response includes
  `available_actions`, which the adapter treats as a discovery signal rather
  than trusting a fixed action list.
- **Deterministic output**: no timestamps, randomness, or environment reads.

## Invocation

```bash
python3 taskboard.py --state <dir> <command> [args...]
```

Commands: `add TITLE`, `list`, `detail ID`, `note ID TEXT`, `done ID`,
`delete ID`, `archive`, `purge`, `stats`, `--help`.

Every run prints one JSON envelope with `outcome`, `view`, counts, tasks,
`available_actions`, and any `events`.

## Reset

There is no reset command. Reset is achieved by the adapter copying this
pristine directory to a fresh working directory, which is exactly how ReForge
models "equivalent initial conditions" for a filesystem-backed target.

## What it deliberately does not do

- No network access.
- No secrets, personal data, or third-party content.
- No attempt to simulate a real commercial application.
