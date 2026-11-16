# CLAUDE.md

@README.md
@CONTRIBUTING.md

## Clean room

- Work from the game's behaviour and data files, never its code: nothing from
  disassembly or decompilation, and nothing that mirrors its structure.
- Never commit game files, extracted assets or anything derived from the
  executables.

## Engine

- Keep the engine data-driven: game-specific behaviour comes from the data
  files, not from code that checks which game it is.
- Name things by the data files' own terms (organization, faction, kingdom),
  not the game UI's.
- File formats are documented once: as Kaitai specs in `docs/formats/`, or in
  the reading crate's module docs. Elsewhere, link to them.
- Intentional differences from the original go in `docs/divergences.md`.

## Docs

- Say "this project" rather than its name, so a rename stays cheap.

## Commits and PRs

- Subjects are one imperative line saying what changes in play, e.g.
  "Recruit K2 companies at settlements".
- Call the games `AA` and `K2`, not Axis & Allies and Kohan II.
