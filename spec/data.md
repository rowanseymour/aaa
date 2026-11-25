# Data files

How the game reads its definition files: which files load, how ids, renames
and duplicates resolve, how values are read, how models find their textures
and sounds, and how the string tables turn keys into the text the player
reads.

Games: traced in AA. Kohan II runs the same code, but its data hasn't been
checked against these rules.

## Which files load

- Only the `.tgi` files the `index*.lst` files name are loaded, in the order
  those lists give. [code]
- A `*` pattern in a list loads its matches in sorted name order (plain
  character order, case-sensitive, so `Vulture.tgi` sorts before
  `eagle.tgi`), within that one folder only; subfolders load only if a
  list names them. [code, guess]
- A file named twice (twice in the lists, or by a name and a `*`) is read
  each time. The second reading meets its own ids as duplicates, so it
  fails at its first definition (below) and the first reading stands.
  [code]

## Ids, renames and duplicates

- `[Translate]` entries rename an old id (`ids_old`) to a new one
  (`ids_new`). Wherever the game looks an id up (in templates, maps'
  terrains and things), a renamed id is replaced by its new id first. [code]
- Renames go one step only: if A is renamed to B and B to C, A finds B's
  definition, or nothing when B has none. [code]
- A rename wins even over a definition still made under the old id. [code]
- A rename to an id with no definition finds nothing. In shipped data
  `tropical_river03` is renamed to `tropical_river02`, which is itself only a
  rename, so `tropical_river03` finds no terrain. [code]
- Renaming the same old id twice is a load error, the first rename is kept.
  Renames inside comments don't count. [code]
- Two definitions with the same `Thing` id are a load error. The first
  definition stays. The error ends the reading of the file holding the
  second: that block and every block after it in the file are skipped, the
  blocks before it are kept, and loading goes on with the next file. A
  load error doesn't stop the game. [code] The shipped
  `eastern_europe_city_center02_large` and `_small` pair is no conflict:
  one copy lives in a subfolder no index list names, so it never loads.
  [code]

## Templates

- A definition naming a template (`Template=` or `Inherit=`) takes from it,
  at every level of nesting, each field it doesn't set itself; a field it
  does set wins. Templates of templates chain the same way. [code]
- A section the definition also has is combined field by field like this.
  A section only the template has is copied in, unless the template marks
  it `optional`: then a thing that doesn't write it has none. [code]
- Sections a template marks `list` (`[list WhenHealth Template=FXC]` in
  the body component's template, and the like) can be written any number
  of times, and they add up: a thing gets every entry written in each of
  its templates, all the way up the chain, ancestors' first, followed by
  its own. Writing entries of a list never drops the inherited ones, and
  there's no way to remove one. [code] So a German medium tank has both
  `AAUnitVehicle`'s `smoke_tank` at 40% and its own template's
  `medium_tank_debris` at 0, and a packable HQ gets the five commands of
  `AABuildingPackable` plus three of its own. [code]
- Writing a section more than once where the template has that section
  but doesn't mark it `list` is a load error. [code]
- A section written inside a definition, where the enclosing section's
  template has a section of the same name, is filled like this: [code]
  - naming no template, or the same template the enclosing template's
    section names: it takes its missing fields from the enclosing
    template's section and nothing else. That section was itself filled
    from the named template when it was defined, its own fields winning,
    so what the enclosing template's section sets beats the named
    template's whole chain;
  - naming a different template: it takes its missing fields from its
    own template first (the whole chain), then any still missing from
    the enclosing template's section. Its template must be that
    section's template or a descendant of it, or the load fails.
  So an engineer's `[Ability Template=AARepairLocal] { [AbilityDefinition
  Template=AARepairAction] ... }` gets `recharge_time = 0.0` from
  `AARepairLocal`'s own `[AbilityDefinition Template=AARepairAction]`
  section, not the `1.5` that `AARepairAction` inherits from
  `AARepairAllAction`; the fields `AARepairAction` sets (`targets`,
  `name`) still come through, by way of that section. A rifle's
  `[Ability Template=AAGunLocal] { [AbilityDefinition
  Template=AAInfantryRifle] ... }` is the other case: `AAInfantryRifle`
  descends from the `AAAttack` that `AAGunLocal`'s section names, so
  its own chain's fields come before anything that section sets.
  [code, data] The game's engineers heal at the pace a zero recharge
  gives ([construction](construction.md#building-and-repairing-work)).
  [game]
- A `[Template]` lasts only as long as its file stays loaded. A file that
  holds any definitions (things, terrains and the like) is let go once
  read, and its templates go with it; a file of nothing but templates
  stays loaded for the rest of the data load, so its templates serve every
  file after it. [code] That files let go are dropped at once, not kept in
  a cache, is inferred from the shipped data loading cleanly. [guess]
- So the same template name in two files that both hold definitions is no
  conflict: each file uses its own. In shipped data
  `AAUSAAttackBattleShipBombard` (in `usa_battleship.tgi` and the
  cutscene's `hires_usa_battleship_texas.tgi`) and `AARUSRegimentParatrooper`
  (in `regiment_rus_paratrooper.tgi` and `_heavy.tgi`) are of this kind,
  and each file's things take that file's version. [code, guess]
- Defining a template whose name is still loaded (one from a templates-only
  file, or earlier in the same file) is a load error that fails the whole
  later file: none of its blocks load, and the first template stands.
  [code]

## Values

- A field is read by the type the game expects for it, not by how the value
  looks. [code]
- Numbers with a fraction: the number as written, divided by 100 when the
  text contains `%`. So `80` is 80, `80%` is 0.8 and `0.8` is 0.8. In a list
  of two or three numbers each is read this way on its own. [code]
- Whole numbers ignore a `%`. [code]

## Sounds on animations

- An animation's `start_min` and `start_max` are seconds. The sound next
  starts `start_min` plus a random fraction of `start_max - start_min` after
  the animation is created and again after each play ends. [code]
- `PlaySFX` entries play each time their animation starts, looping if they're
  flagged to, for every kind of animation. Sounds on the `SELECTED`
  animation are sometimes skipped. [code] When is open.

## Textures

- Texture renames belong to one model (`AspectNormal`, or one `Aspect`), not
  to the whole unit: each model uses only its own. [code]
- A texture a model names is first taken as the model's folder plus the
  name. The model's renames then apply, last declared first, each
  replacing every occurrence of its text anywhere in that path (case
  matters), so one rename's result can be renamed again by an earlier one.
  [code]
- Then the game takes the first that exists of: that path; then, for each
  of the view's search paths in order, the file name in that folder (a
  search path starting with `.` is relative to the texture's folder). [code]
  File names match ignoring case. [guess]
- Failing all of those the default texture is used, with a warning. The
  game never searches the archive for another file of the same name. [code]
- A map's biome can name a subfolder (`texture_subfolder_name`: `Arctic`
  for `arctic_tundra` and `boreal_forest`, `Desert` for `desert`,
  `Tropical` for the two tropical biomes). While such a map is loaded,
  whenever a texture or a model file is about to be read from a folder
  and a file of the same name exists in that subfolder of the same folder,
  the subfolder's file is read instead (`Units/USA/USATruck/Arctic/
  USATruckBase.dds` for `Units/USA/USATruck/USATruckBase.dds`). It applies
  to every model and texture, world and interface alike. [code, data]
- The swap is part of every test in the texture search above, not a step
  after it. Each candidate (the model's folder first, then each search
  path) counts as found when the file is in that folder's biome subfolder
  or in the folder itself, and the subfolder's copy wins when both are
  there. So a texture that exists only in the biome subfolder of the
  model's folder is found there, before any search path is tried: on an
  arctic map a model in `Units/USA/USAInfantryCO/` naming
  `PortraitBreathBase.dds` gets `Units/USA/USAInfantryCO/Arctic/
  PortraitBreathBase.dds`, not the plain one in `Shared/SharedTextures/`;
  on other maps the folder has none and the shared one is used. A search
  path's own biome subfolder is tried the same way when its turn comes.
  [code]
- A model that was itself read from the biome subfolder has that
  subfolder as its folder: its textures are looked for beside it there
  (and in the search paths), never in the plain folder above. One missing
  from the subfolder isn't taken from the plain model's folder. [code]
  Every shipped subfolder model has all its textures beside it. [data]

## String tables

- Each `[Text]` block in `Localization/strings_*.tgi` holds `key = "text"`
  pairs. Its `ids` and `name` describe the block and aren't strings. Every
  other value must be quoted; an unquoted one is a data error. [code]
- All tables share one set of keys, and keys are compared ignoring ASCII
  case. [code]
- A `|` in a key is part of the key: `multi_element_format_string|ss` and
  `multi_element_format_string|sss` are two strings, and the game asks for
  them with the suffix. The suffix lists the values the text is formatted
  with, in order: `s` a string, `d` a whole number (e.g.
  `objective_city_ratio|ddd = "Control %d cities (%d/%d)"`). [code] Five
  stock keys exist with more than one suffix, or with and without one.
  [data]
- A key defined twice keeps the first table's text. [code] Shipped tables
  define none twice. [data]
- Data fields such as `name = "#rus_infantry_sniper_name"` name a key with
  a leading `#`. [guess]
- Asking for a key that doesn't exist logs an error and shows a placeholder
  text, unless the asker supplied its own fallback. [code]

## Escapes

These apply to every quoted string in every data file, as it's read, not
only to the string tables. [code]

- Glyphs are defined by `[Glyph]` blocks in `Fonts/special_glyphs*.tgi`:
  `ids` names the glyph, `index` is the character it stands for. Most
  indices are 128–159, characters the font draws as icons (`ch_gem` 132,
  `ch_rank1` 134); a few are ordinary characters (`n` 10, `ch_amp` 38,
  `ch_semicolon` 59). Glyph names are compared ignoring case. [code]
- `\` takes the one character after it: `\c` becomes the glyph whose `ids`
  is `c`, so `\n` is a newline. `\\` and `\&` give a plain `\` and `&`.
  `\` before any other character that isn't a glyph name is a data error
  and is left as written. A `\` at the very end is a data error. [code]
- `&name.` becomes the glyph called `name`: `&ch_gem.` is the gem icon.
  `&&` gives a plain `&`. An unknown name is a data error and is left as
  written; a `&` with no `.` after it is a data error. [code]
- The three characters `\`, `&` and `.` are game settings, not fixed;
  these are the stock ones, as the shipped data uses them. [guess]

## Open questions

- When is a `SELECTED` animation's sound skipped? (analysis, 2026-11-08)
- Where the same file is both in `Data.rwd` and loose in the data folder,
  which is read? (analysis, 2026-11-08)
- What does a leading `@` on a quoted value do (the tables' own
  `name = "@Localization/strings_data_AA.tgi"`)? Is a quoted value without
  `#` always shown as written? (analysis, 2026-11-08)
- Which text is the placeholder for a missing key? (analysis, 2026-11-08)
- How are `%s`, `%d`, `%1`… filled, and what markup such as
  `<color=1,1,1>` does when drawn? (analysis, 2026-11-08)
