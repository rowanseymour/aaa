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

One file per subject, named for the area of play (`combat.toml`,
`buildings.toml`), each paragraph or list item of a text on one line, and
a text of one line in a single `'''…'''`:

```toml
subject = "<Subject>"
summary = '''<What the subject covers.>'''
games = '''<Which game's engine and data the rules were traced in.>'''
intro = '''<Optional: what holds across the topics.>'''

[[topic]]
title = "<Topic>"
under = "<Optional: the topic this one is part of>"
intro = '''<Optional: what the topic is about.>'''

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
  that is partly a guess is two rules. A rule with none hasn't been
  tagged yet.
- Examples are worked cases of their rule, in the rule's own terms, for
  tests to load and check.
- A link's `#fragment` names a topic (its title in kebab case) or a
  rule ID.

## Index

Each subject lives in one spec; others link to it rather than repeat it.
A new spec goes under the heading it fits.

- **The map**: [terrain](terrain.toml) (ground height and terrain
  properties), [objects](objects.toml) (shapes, blocking, zones),
  [scenery](scenery.toml), [fog](fog.toml), [minimap](minimap.toml) (its
  ground colours, fog shading and things' marks),
  [lighting](lighting.toml) (the sun and AA's Time of Day).
- **Units in play**: [movement](movement.toml), [combat](combat.toml),
  [morale](morale.toml) (morale, routing and a regiment's experience),
  [supply](supply.toml), [deliveries](deliveries.toml),
  [airfields](airfields.toml) (aircraft stock, air attacks, recon, defence),
  [stances](stances.toml) (kneeling, prone, damage and death looks,
  entrenchment).
- **Sides and their economy**: [players](players.toml) (how sides stand
  towards each other, handicaps), [economy](economy.toml),
  [recruiting](recruiting.toml) (what's offered, cost and limits, where
  recruits appear), [buildings](buildings.toml) (packing, cities, siege,
  capture, selling), [construction](construction.toml) (building sites
  and repair), [research](research.toml), [special
  operations](special-ops.toml), [settlements](settlements.toml) (Kohan II's
  centres, settlements and denizens), [companies](companies.toml) (Kohan
  II's company make-up and heroes).
- **The match**: [match start](match-start.toml) (placeholders, random
  things, start locations, what a side starts with),
  [objectives](objectives.toml), [triggers](triggers.toml) (how a map's
  triggers run), [outcome](outcome.toml),
  [game events](game-events.toml), [multiplayer](multiplayer.toml),
  [computer players](computer-players.toml) (how the AI plays: profiles,
  moods, goals and what it knows),
  [campaigns](campaigns.toml) (the campaign screen, mission order,
  debriefing and progress).
- **Interface**: [interface](interface.toml) (the in-match screen's
  layout, scaling and skins), [selection](selection.toml),
  [commands](commands.toml), [team commands](team-commands.toml) (the ally
  page, flares, giving and asking, report),
  [object display](object-display.toml), [messages](messages.toml),
  [politics](politics.toml) (the list of sides and how they stand).
- **Sound and look**: [sounds](sounds.toml), [music](music.toml),
  [effects](effects.toml), [particles](particles.toml), [models](models.toml).
- **Data**: [data files](data.toml) (loading, textures, string
  tables), [saved state](saved-state.toml), [games](games.toml) (what the
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
  [games](games.toml) maps each game's words for the same things and lists
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
