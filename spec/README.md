# Behaviour specs

This folder is the boundary of a clean-room reimplementation. The games'
executables are studied separately, by disassembly and decompilation, and
the games are watched in play. What that finds is written here as what
the game does, in our own words. The code in this repo is written from
these specs, the data files and the running game, never from the
executables, so none of the original program's code reaches it.

Specs are written only by analysis sessions (`/spec`), which work in a
private repo and commit here nothing but this folder. Implementers only
read them, and bring their questions to an analysis session. The git
history of this folder is the record of everything that crossed.

One file per subject, named for the area of play (`combat.md`,
`buildings.md`), each laid out as:

```markdown
# <Subject>

<What the subject covers, in a sentence or two.>

Games: <which game's engine and data the rules were traced in, and what
of the other game is unchecked>

## <Topic>

- <Rule.> [code|game|data|guess]

## Open questions

- <Question still to settle.> (<analysis|implementer>, <date asked>)
```

Specs are moving to TOML (`supply.toml`), with the same parts as fields,
each paragraph or list item of a text on one line, and a text of one line
in a single `'''…'''`:

```toml
subject = "<Subject>"
summary = '''<What the subject covers.>'''
games = '''<Which game's engine and data the rules were traced in.>'''
intro = '''<Optional: what holds across the topics.>'''

[[topic]]
title = "<Topic>"

[[topic.rule]]
id = "<rule-id>"
tags = ["<code|game|data|guess>", ...]
text = '''<Rule, Markdown.>'''

[[topic.rule.example]]            # optional, any number
given = { <name> = <value>, ... }
then = { <name> = <value>, ... }
note = "<optional>"

[[open]]
q = '''<Question still to settle.>'''
by = "<analysis|implementer>"
asked = <date asked>
```

- A rule's `id` is stable: tests name the rule they check by it
  (`supply#relay-connection`), and its git history is the rule's.
- `tags` are the rule's, in that order, and hold for all of it: a rule
  that is partly a guess is two rules.
- Examples are worked cases of their rule, in the rule's own terms, for
  tests to load and check.
- A link's `#fragment` into a TOML spec names a topic (its title in
  kebab case) or a rule ID.

## Audits

`audits/<subject>.toml` says how far this repo's code follows a TOML
spec, rule by rule, as an analysis session found it (`/reconcile`). It's
the one place a spec's folder speaks of our code. Implementers fix what it
lists and the next audit re-checks it; nobody else edits it.

```toml
subject = "<subject>"
aaa = "<the commit last audited>"
audited = <date>

[[rule]]
id = "<rule-id>"
verdict = "<matches|partial|deviates|missing>"
where = ["<file:lines>", ...]     # where our code does it, or would
detail = '''<What differs, as the player would see it.>'''
checked = <true|false>            # confirmed in the code, or as reported
aaa = "<the commit this rule was audited at>"
spec = "<the rule as audited, hashed>"
```

A rule the spec has but the audit doesn't list hasn't been audited.

## Index

Each subject lives in one spec; others link to it rather than repeat it.
A new spec goes under the heading it fits.

- **The map**: [terrain](terrain.md) (ground height and terrain
  properties), [objects](objects.md) (shapes, blocking, zones),
  [scenery](scenery.md), [fog](fog.md), [minimap](minimap.md) (its
  ground colours, fog shading and things' marks),
  [lighting](lighting.md) (the sun and AA's Time of Day).
- **Units in play**: [movement](movement.md), [combat](combat.md),
  [morale](morale.md) (morale, routing and a regiment's experience),
  [supply](supply.toml), [deliveries](deliveries.md),
  [airfields](airfields.md) (aircraft stock, air attacks, recon, defence),
  [stances](stances.md) (kneeling, prone, damage and death looks,
  entrenchment).
- **Sides and their economy**: [players](players.md) (how sides stand
  towards each other, handicaps), [economy](economy.md),
  [recruiting](recruiting.md) (what's offered, cost and limits, where
  recruits appear), [buildings](buildings.md) (packing, cities, siege,
  capture, selling), [construction](construction.md) (building sites
  and repair), [research](research.md), [special
  operations](special-ops.md), [settlements](settlements.md) (Kohan II's
  centres, settlements and denizens), [companies](companies.md) (Kohan
  II's company make-up and heroes).
- **The match**: [match start](match-start.md) (placeholders, random
  things, start locations, what a side starts with),
  [objectives](objectives.md), [triggers](triggers.md) (how a map's
  triggers run), [outcome](outcome.md),
  [game events](game-events.md), [multiplayer](multiplayer.md),
  [computer players](computer-players.md) (how the AI plays: profiles,
  moods, goals and what it knows),
  [campaigns](campaigns.md) (the campaign screen, mission order,
  debriefing and progress).
- **Interface**: [interface](interface.md) (the in-match screen's
  layout, scaling and skins), [selection](selection.md),
  [commands](commands.md), [team commands](team-commands.md) (the ally
  page, flares, giving and asking, report),
  [object display](object-display.md), [messages](messages.md),
  [politics](politics.md) (the list of sides and how they stand).
- **Sound and look**: [sounds](sounds.md), [music](music.md),
  [effects](effects.md), [particles](particles.md), [models](models.md).
- **Data**: [data files](data.md) (loading, textures, string
  tables), [saved state](saved-state.md), [games](games.md) (what the
  engine fixes and what each game's data decides, across AA and Kohan II).

## What goes in

- Rules as they'd be explained to a player or a board-game designer: what
  happens, when, to whom, and in what order where the order is visible.
- Formulas and constants, saying which data field or game variable a value
  comes from by the name the data files use (`recharge_time`,
  `CombatDamageMinimum`), with stock values where they matter.
- Edge cases, ties, limits and timings.
- How sure we are, tagged on each rule: `[code]` read in the engine,
  `[game]` seen in the running game, `[data]` borne out by the shipped
  data files, `[guess]` inferred and unchecked. A rule seen more than one
  way takes each tag (`[code, game]`).
- What's still unsettled, as questions under Open questions, rather than
  left as "is open" in a rule alone.
- Which game a rule or value belongs to, where the games differ: the
  subject's spec states both, marking the one game's rule ("Kohan II
  only:", "AA's data sets …"). A rule with no game named holds for both.
  [games](games.md) maps each game's words for the same things and lists
  only the differences that span the engine.

## What stays out

- Addresses, offsets, internal class, function, member or state names, and
  anything else only the engine's code holds. Names the data files hold are
  fine.
- The engine's structure: how its code is split into functions, objects or
  state machines, its call order, helpers or variable layout. Describe
  outcomes ("an engagement interrupts the order, which resumes afterwards"),
  not mechanism ("pushes a state over the move state").
- Pseudocode that follows the engine's code. A formula is fine; a
  transcribed procedure is not.
- Anything about our own code: the spec is the game's behaviour, not a
  description of `tgsim`.

A check run on every spec before it's committed here catches the
mechanical leaks; the structural ones need a read-through.
