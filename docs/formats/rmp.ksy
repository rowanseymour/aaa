meta:
  id: rmp
  title: Axis & Allies (2004) map
  file-extension: rmp
  license: MIT
  endian: le
  imports:
    - tgck
doc: |
  A map: a TGCK container whose sections hold the terrain, the placed
  objects, the players and the scripting, as the game saves its world.
  Payloads are packed with no alignment.

  The GWORLD section's first field, the world version, decides the layout
  of several other sections; each section's own version decides the rest.
  World coordinates run from 0 to (height-map side - 1) * 2, two units per
  height-map cell, with z up.
doc-ref: README.md
seq:
  - id: header
    type: tgck::header
  - id: payloads
    size: _io.size - _io.pos - header.section_count * 0x60
  - id: sections
    type: section
    repeat: expr
    repeat-expr: header.section_count
instances:
  world_version:
    value: gworld.version
  gworld:
    value: sections[gworld_index.index].body.as<section_body>.value.as<gworld>
  gworld_index:
    pos: 0
    size: 0
    type: find_section('GWORLD', 0)
types:
  section:
    seq:
      - id: record
        type: tgck::section_record
    instances:
      body:
        value: 'record.is_compressed ? inflated_body : stored_body'
      stored_body:
        io: _root._io
        pos: record.offset
        size: record.len_stored
        type: section_body(record.name.value)
        if: not record.is_compressed
      inflated_body:
        io: _root._io
        pos: record.offset
        size: record.len_stored
        process: zlib
        type: section_body(record.name.value)
        if: record.is_compressed
  section_body:
    params:
      - id: name
        type: str
    seq:
      - id: value
        size-eos: true
        type:
          switch-on: name
          cases:
            '"MAP"': map_data
            '"MAP_HEADER"': map_header
            '"GWORLD"': gworld
            '"RULES"': world_rules
            '"TERRAIN"': terrain
            '"HEIGHT_MAP"': height_map(false)
            '"MOUNTAIN_HEIGHT_MAP"': height_map(true)
            '"IDMGR"': id_manager
            '"RESTRICTION_GROUPS"': restriction_groups
            '"TEAMS"': teams
            '"OBJECTIVES"': objectives
            '"KINGDOMS"': kingdoms
            '"USED_REQUIRED_OBJECTS"': thing_list('USED_REQUIRED_OBJECTS_INDEX')
            '"USED_REQUIRED_OBJECTS_INDEX"': thing_index
            '"DYNAMIC_THINGS"': thing_list('DYNAMIC_THINGS_INDEX')
            '"DYNAMIC_THINGS_INDEX"': thing_index
            '"TRIGGERS"': triggers
            '"DENIZEN_GROUPS"': denizen_groups
            '"SUN"': sun
            '"UPDATABLES"': updatables
            '"MISC_DATA"': misc_data
            '"MAPSTRINGS"': map_strings
            _: undecoded
  find_section:
    doc: The index of the first section named `name` at or after `i`.
    params:
      - id: name
        type: str
      - id: i
        type: u4
    instances:
      index:
        value: '(_root.sections[i].record.name.value == name ? i : next.index).as<u4>'
      next:
        pos: 0
        size: 0
        type: find_section(name, i + 1)
        if: _root.sections[i].record.name.value != name

  # Building blocks.
  string:
    seq:
      - id: len
        type: u2
        doc: In UTF-16 code units.
      - id: value
        type: str
        size: len * 2
        encoding: UTF-16LE
  strings:
    seq:
      - id: num_items
        type: u4
      - id: items
        type: string
        repeat: expr
        repeat-expr: num_items
  u4s:
    seq:
      - id: num_items
        type: u4
      - id: items
        type: u4
        repeat: expr
        repeat-expr: num_items
  f4s:
    seq:
      - id: num_items
        type: u4
      - id: items
        type: f4
        repeat: expr
        repeat-expr: num_items
  vec3:
    seq:
      - id: x
        type: f4
      - id: y
        type: f4
      - id: z
        type: f4
  amount:
    doc: A quantity of a resource, such as `money` or `ammo`.
    seq:
      - id: resource
        type: string
      - id: amount
        type: f4
  string_u4:
    seq:
      - id: name
        type: string
      - id: value
        type: u4
  amounts:
    seq:
      - id: num_items
        type: u4
      - id: items
        type: amount
        repeat: expr
        repeat-expr: num_items
  string_u4s:
    seq:
      - id: num_items
        type: u4
      - id: items
        type: string_u4
        repeat: expr
        repeat-expr: num_items
  flag:
    doc: A byte that is 0 or 1.
    seq:
      - id: value
        type: u1
        valid:
          max: 1

  undecoded:
    seq:
      - id: data
        size-eos: true

  # Small sections.
  map_data:
    doc: MAP, the map resource's data version.
    seq:
      - id: data_version
        type: u4
        valid: 0
  gworld:
    doc: GWORLD, the game's world state.
    seq:
      - id: version
        type: u4
        valid:
          max: 28
        doc: The world version, which other sections' layouts follow.
      - id: rng_seed
        type: u4
      - id: game_ticks
        type: u4
        doc: Ticks the game has run; 0 in every shipped map.
      - id: game_time_begin
        type: f4
        doc: The game time at the start of the last tick.
      - id: game_time_end
        type: f4
        doc: The game time at the end of the last tick.
      - id: game_time_per_tick
        type: f4
        doc: Seconds.
      - id: next_player_id
        type: u4
      - id: next_camera_id
        type: u4
        if: version >= 4
        doc: The number the editor gives the next camera it names (`camera_3`).
      - id: next_area_id
        type: u4
        if: version >= 4
        doc: The number the editor gives the next area it names (`area_4`).
      - id: next_structure_queue_item_id
        type: u4
        if: version >= 27
      - id: map_status
        type: u4
        if: version >= 5
        doc: 0 unverified, 1 repaired, 2 verified; a mission only starts on a verified map.
      - id: mission_state
        type: u4
        if: version >= 7
        doc: 0 not started, 1 running, 2 won.
  misc_data:
    doc: |
      MISC_DATA, world state that fits nowhere else. The game reads it from
      this section from world version 18; every shipped map is newer.
    seq:
      - id: num_used_names
        type: u4
      - id: used_names
        type: string_u4
        repeat: expr
        repeat-expr: num_used_names
        doc: |
          Names handed out from the game's name lists, so none is given
          twice: the list's id and the name's index in it.
      - id: pre_cutscene_game_speed
        type: f4
        doc: The speed to go back to when a cutscene ends.
      - id: active_weather
        type: string
        doc: A weather id; empty for none.
      - id: ui_script_listens
        type: strings
        if: _root.world_version >= 24
        doc: The UI script events triggers listen for.
      - id: has_game_counter
        type: flag
        if: _root.world_version >= 28
      - id: game_counter
        type: game_counter
        if: _root.world_version >= 28 and has_game_counter.value == 1
  game_counter:
    doc: The countdown a trigger shows on screen.
    seq:
      - id: start_time
        type: f4
        doc: The game time it was started at.
      - id: duration
        type: f4
        doc: Seconds it counts down from.
  denizen_groups:
    doc: DENIZEN_GROUPS, the herds wildlife keeps to.
    seq:
      - id: num_groups
        type: u4
      - id: groups
        size-eos: true
        doc: |
          Undecoded: each is a u4 id, then the group. No shipped map has
          any.
  map_strings:
    doc: |
      MAPSTRINGS, the map's string table: text in `.tgi` syntax mapping the
      string ids other sections use (map name, objectives, ...) to text.
    seq:
      - id: text
        type: str
        size-eos: true
        encoding: windows-1252
  sun:
    doc: SUN, the map's light, and the screen effects scripts drive.
    seq:
      - id: version
        type: u4
        valid:
          max: 2
        doc: 1 or 2 in shipped maps.
      - id: updatable
        type: updatable
        doc: Registered only while a fade runs; -1 otherwise.
      - id: direction
        type: vec3
        doc: The unit vector the light travels along.
      - id: diffuse
        type: vec3
        doc: As in `Templates/template_sun.tgi`.
      - id: ambient
        type: vec3
      - id: specular
        type: vec3
      - id: target_diffuse
        type: vec3
        doc: The colours above fade toward these.
      - id: target_ambient
        type: vec3
      - id: target_specular
        type: vec3
      - id: color_fade_time
        type: f4
        doc: Seconds left in the colour fade.
      - id: dimmer
        type: f4
        doc: A brightness multiplier.
      - id: target_dimmer
        type: f4
      - id: dimmer_fade_time
        type: f4
        doc: Seconds left in the dimmer fade.
      - id: last_update_time
        type: f4
        doc: The game time of the last fade step; -1 before any.
      - id: letterbox
        type: overlay
        if: version >= 1
        doc: The cinematic black bars.
      - id: screen_fade
        type: overlay
        if: version >= 1
  overlay:
    doc: A screen effect scripts turn on and off.
    seq:
      - id: on
        type: flag
      - id: start
        type: f4
        doc: The game time it was turned on or off at; ignored in version 1.
      - id: duration
        type: f4
        doc: Seconds it takes to come or go; ignored in version 1.

  # The bookkeeping of the simulation.
  id_manager:
    doc: |
      IDMGR, the game's object ids: a 16-bit slot, with the slot's
      generation in the top 16 bits so a reused slot gets a fresh id.
    seq:
      - id: version
        type: u4
        valid: 0
      - id: next_slot
        type: u4
        doc: Where the search for a free slot starts.
      - id: num_generations
        type: u4
        doc: The highest slot ever used.
      - id: capacity
        type: u4
        valid: 0x10000
      - id: generations
        type: u2
        repeat: expr
        repeat-expr: num_generations
        doc: Each slot's generation, which goes up when its object is removed.
  updatables:
    doc: |
      UPDATABLES, the queues that schedule objects' updates. The game reads
      it from world version 11.
    seq:
      - id: count
        type: u4
        doc: How many updatables the world has; ids run up to it.
      - id: delayed
        type: update_queue
        repeat: expr
        repeat-expr: 2
        doc: Ring buffers of ticks, each slot holding the updates due then.
      - id: periodic
        type: update_queue
        repeat: expr
        repeat-expr: 3
        if: _root.world_version >= 22
        doc: One slot per tick of a period of 1, 16 and 80 ticks.
  update_queue:
    seq:
      - id: version
        type: u4
        valid: 0
      - id: current_slot
        type: u4
      - id: num_slots
        type: u4
      - id: slots
        type: update_slot
        repeat: expr
        repeat-expr: num_slots
  update_slot:
    seq:
      - id: num_updates
        type: u4
      - id: updates
        type: update
        repeat: expr
        repeat-expr: num_updates
  update:
    seq:
      - id: updatable_id
        type: s4
      - id: argument
        type: u4
        doc: Passed to the updatable when the update runs.

  # Terrain.
  height_map:
    doc: |
      HEIGHT_MAP holds the ground's height at each vertex, or the water
      surface's at water vertices. MOUNTAIN_HEIGHT_MAP has the same layout
      and holds the depth of the bed below the water (negative).
    params:
      - id: is_mountain
        type: bool
    seq:
      - id: version
        type: u4
        doc: 1.
      - id: min
        type: f4
        doc: -10.
      - id: max
        type: f4
        doc: 10.
      - id: default
        type: f4
      - id: unknown
        type: u4
        doc: Unknown; 1.
      - id: num_vertices
        type: u4
        doc: side * side, a square; side is 257 on standard maps.
      - id: heights
        type: f4
        repeat: expr
        repeat-expr: num_vertices
        doc: Row-major.
      - id: layer
        type: u1
        repeat: expr
        repeat-expr: num_vertices
        doc: In HEIGHT_MAP non-zero marks water; all 0 in MOUNTAIN_HEIGHT_MAP.
      - id: cutscene_mode
        type: flag
        if: is_mountain and _root.world_version >= 10
        doc: The world's flag for being mid-cutscene, saved here; 0 in every shipped map.
  terrain:
    doc: |
      TERRAIN, the ground's type per tile. Tiles are the cells between
      height-map vertices, so a side has one fewer.
    seq:
      - id: version
        type: u4
        valid:
          max: 7
        doc: 6 or 7 in shipped maps.
      - id: unknown
        type: u4
        doc: Unknown; the game requires 1.
      - id: biome
        type: string
        doc: E.g. `temperate_grassland`.
      - id: names
        type: strings
        doc: Terrain type ids, e.g. `temperate_grass03`, which tiles index.
      - id: num_tiles
        type: u4
        doc: side * side, a square.
      - id: tile_size
        type: u4
        valid: '(version >= 7 ? 8 : (version >= 5 ? 6 : (version == 4 ? 4 : 2)))'
      - id: tiles
        type: tile(version)
        size: tile_size
        repeat: expr
        repeat-expr: num_tiles
        doc: Row-major.
      - id: ai_cache
        size-eos: true
        doc: |
          Undecoded: the AI's analysis of the map, then from version 6 one
          more object. The game can rebuild it.
  tile:
    doc: |
      Version 7 widens the terrain index and reorders the fields; versions
      before 6 end early.
    params:
      - id: version
        type: u4
    seq:
      - id: terrain_v7
        type: u2
        if: version >= 7
      - id: terrain_old
        type: u1
        if: version < 7
      - id: flags_old
        type: u1
        if: version < 7
      - id: unknown
        type: u2
        if: version >= 4
      - id: water_body
        type: u1
        if: version >= 5
        doc: The water body id; 0 for dry land. It matches HEIGHT_MAP's layer.
      - id: unknown2
        type: u1
        if: version >= 5
        doc: Usually 0.
      - id: flags_v7
        type: u1
        if: version >= 7
      - id: unused
        type: u1
        if: version >= 7
    instances:
      terrain:
        value: 'version >= 7 ? terrain_v7 : terrain_old'
        doc: The index into the TERRAIN names.
      flags:
        value: 'version >= 7 ? flags_v7 : flags_old'
        doc: Bits 0-3; the game recomputes bits 4-6 from the terrain's group.

  # The map header and rules.
  map_header:
    doc: MAP_HEADER, what a map offers before it's loaded.
    seq:
      - id: version
        type: u4
        valid:
          min: 4
          max: 7
      - id: rules
        type: world_rules
      - id: parameters
        type: world_parameters
      - id: header_version
        type: u4
        doc: The version again, so that it travels with the header.
      - id: checksum
        type: u4
        doc: 0 in a saved map; the game fills it in when it launches one.
      - id: name
        type: string
        doc: A string id, defined in MAPSTRINGS.
      - id: description
        type: string
        doc: A string id, defined in MAPSTRINGS.
      - id: size
        type: f4
        repeat: expr
        repeat-expr: 2
        if: version >= 7
        doc: Width and height in world units.
      - id: build
        type: f4
        doc: The build number of the game that saved it.
      - id: version_number
        type: u4
        doc: '`app_version` as a number, two decimal digits a part: 30100 for 0.3.1.0.'
      - id: app_version
        type: string
        doc: The version of the game that saved it, e.g. `0.3.1.0`.
      - id: app_id
        type: string
        doc: The game the map is for, `A&A`; the game refuses maps for another.
      - id: edition
        type: string
        doc: The edition of the game that saved it, e.g. `North America AA Dev`.
      - id: unknown
        type: u1
        doc: Unknown; 0 in every shipped map.
      - id: path
        type: string
        doc: The map's path when it was saved.
      - id: campaign
        type: string
        doc: The campaign a saved game belongs to; empty for none.
      - id: campaign_mission
        type: s4
        if: version >= 6
        doc: The mission's index in that campaign; -1 for none.
      - id: saved
        type: u8
        doc: When the map was saved, a Windows FILETIME.
      - id: players
        type: player(version)
        repeat: until
        repeat-until: _.present.value == 0
        doc: Ends with an absent entry.
      - id: creator
        type: world_creator
  player:
    params:
      - id: version
        type: u4
    seq:
      - id: present
        type: flag
      - id: ai
        type: flag
        if: present.value == 1
        doc: The computer plays it.
      - id: kingdom
        type: string
        if: present.value == 1
      - id: name
        type: string
        if: present.value == 1
      - id: local
        type: flag
        if: present.value == 1 and version >= 5
        doc: It's the player on the machine that saved the map.
  world_creator:
    doc: |
      What the editor generated the map from. Its rules and parameters are
      a second copy of the header's, the same in every shipped map.
    seq:
      - id: class_name
        type: string
        valid:
          expr: _.value == 'KKC_WorldCreator'
      - id: version
        type: u4
        valid: 3
      - id: rules
        type: world_rules
      - id: parameters
        type: world_parameters
      - id: size
        type: f4
        repeat: expr
        repeat-expr: 2
      - id: biome
        type: string
      - id: weather
        type: string
        doc: Empty for none.
      - id: seed
        type: u4
  world_rules:
    doc: |
      The game options a map is played with. RULES holds the rules the world
      plays by; every shipped map's match its header's.
    seq:
      - id: version
        type: u4
        valid: 4
      - id: map_type
        type: u4
        doc: An index into the game's map types; 0 in every shipped map.
      - id: game_type
        type: string
      - id: num_rules
        type: u4
      - id: rules
        type: rule
        repeat: expr
        repeat-expr: num_rules
  rule:
    seq:
      - id: name
        type: string
      - id: locked
        type: flag
        doc: The map fixes it, so players can't change it.
      - id: type
        type: u4
        enum: rule_type
        valid:
          expr: _.to_i <= 3
      - id: bool_value
        type: flag
        if: type == rule_type::bool
      - id: int_value
        type: s4
        if: type == rule_type::int
      - id: float_value
        type: f4
        if: type == rule_type::float
      - id: choice_value
        type: u4
        if: type == rule_type::choice
        doc: The index of one of the rule's choices.
  world_parameters:
    doc: The sides a map is set up with.
    seq:
      - id: version
        type: u4
        valid:
          min: 11
          max: 13
      - id: num_teams
        type: u4
      - id: teams
        type: team_setup
        repeat: expr
        repeat-expr: num_teams
      - id: num_kingdoms
        type: u4
      - id: kingdoms
        type: kingdom_setup(version)
        repeat: expr
        repeat-expr: num_kingdoms
  team_setup:
    seq:
      - id: name
        type: string
      - id: display_name
        type: string
  kingdom_setup:
    params:
      - id: version
        type: u4
    seq:
      - id: name
        type: string
      - id: independent
        type: flag
      - id: nation
        type: string
      - id: faction
        type: string
      - id: color
        type: string
      - id: handicap
        type: string
      - id: display_name
        type: string
      - id: color_markup
        type: string
        if: version >= 12
        doc: Text markup in the kingdom's colour.
      - id: ai
        type: string
        doc: The strategic AI that plays it, if set.
      - id: required
        type: flag
        doc: It must be in the game.
      - id: playable
        type: flag
        doc: A human may play it.
      - id: team
        type: string
      - id: objectives
        type: strings
      - id: technologies
        type: strings
        doc: Technologies it starts with.
      - id: merge_into
        type: string
        doc: The kingdom it merges into; empty for none.
      - id: actors
        type: strings
        doc: Actor templates it starts with besides the map's.
      - id: removed
        type: flag
        doc: It's removed when the game starts.
      - id: ai_controlled
        type: flag
        doc: The strategic AI plays it until a human takes it.
      - id: resources
        type: amounts
        if: version >= 13
        doc: Bonus stockpiles.

  # Objectives and triggers.
  objectives:
    doc: OBJECTIVES, the goals a map sets its kingdoms, which triggers then give, complete or fail.
    seq:
      - id: num_objectives
        type: u4
      - id: objectives
        type: objective
        repeat: expr
        repeat-expr: num_objectives
      - id: next_number
        type: u4
        doc: |
          Above every objective's number in shipped maps, even where higher
          ones were deleted, so presumably what the editor numbers the next.
  objective:
    seq:
      - id: version
        type: u4
        valid: 0
      - id: name
        type: string
        doc: E.g. `objective_0`, by which triggers and kingdoms refer to it.
      - id: text
        type: string
        doc: A string id; the same as the name in every shipped map.
      - id: primary
        type: flag
        doc: Shown with the primary objectives rather than the optional ones.
  triggers:
    doc: |
      TRIGGERS, the map's scripting. Each trigger waits for any of its
      events, checks its conditions and then runs its effects.
    seq:
      - id: version
        type: u4
        valid:
          min: 4
          max: 5
      - id: next_id
        type: u4
        doc: The id the next new trigger gets.
      - id: num_triggers
        type: u4
      - id: triggers
        type: trigger
        repeat: expr
        repeat-expr: num_triggers
      - id: num_variables
        type: u4
      - id: variables
        type: variable
        repeat: expr
        repeat-expr: num_variables
        doc: Named values that triggers can set and compare.
      - id: legacy_string
        type: string
        if: version == 4
        doc: Read and ignored by the game.
      - id: legacy_value
        type: u4
        if: version == 4
        doc: Read and ignored by the game.
      - id: unknown_strings
        type: strings
        if: version >= 5
        doc: Unknown.
  int_value:
    seq:
      - id: value
        type: u4
  float_value:
    seq:
      - id: value
        type: f4
  variable:
    seq:
      - id: name
        type: string
      - id: value
        type: f4
  trigger:
    seq:
      - id: version
        type: u4
        valid: 4
      - id: id
        type: u4
        doc: By which `mi_trigger` parameters refer to it.
      - id: unknown
        type: u4
        doc: Unknown.
      - id: active
        type: flag
        doc: Whether it starts out listening for its events.
      - id: repeats
        type: flag
        doc: Whether it stays active after running its effects.
      - id: activation_time
        type: f4
        doc: The game time it was last activated.
      - id: name
        type: string
      - id: comment
        type: string
        doc: The map author's note.
      - id: next_element
        type: u4
        doc: The id the trigger's next new element gets.
      - id: events
        type: trigger_elements
      - id: conditions
        type: trigger_elements
      - id: effects
        type: trigger_elements
  trigger_elements:
    seq:
      - id: num_elements
        type: u4
      - id: elements
        type: trigger_element
        repeat: expr
        repeat-expr: num_elements
  trigger_element:
    doc: |
      An instance of a template the game defines by name, such as
      `actor_enters_area`.
    seq:
      - id: version
        type: u4
        valid:
          min: 2
          max: 3
      - id: name
        type: string
        doc: The template's name.
      - id: id
        type: u4
        doc: Unique within its trigger.
      - id: num_parameters
        type: u4
      - id: parameters
        type: parameter
        repeat: expr
        repeat-expr: num_parameters
  parameter:
    seq:
      - id: version
        type: u4
        valid: 4
      - id: kind
        type: string
        doc: What it is, e.g. `ma_actor`.
      - id: source
        type: string
        doc: |
          Where its value comes from, e.g. `mi_actor_named`, which decides
          how the value is stored. Sources the game doesn't know store none.
      - id: set
        type: flag
        doc: Whether a value was given; the editor shows a placeholder otherwise.
      - id: value
        type:
          switch-on: source.value
          cases:
            '"mi_int"': int_value
            '"mi_compare"': int_value
            '"mi_actor_any"': int_value
            '"mi_actor_id"': int_value
            '"mi_actor_condition"': int_value
            '"mi_actor_event"': int_value
            '"mi_kingdom_any"': int_value
            '"mi_kingdom_all"': int_value
            '"mi_kingdom_id"': int_value
            '"mi_kingdom_condition"': int_value
            '"mi_kingdom_event"': int_value
            '"mi_hero_random"': int_value
            '"mi_area"': int_value
            '"mi_camera"': int_value
            '"mi_boolean"': int_value
            '"mi_state"': int_value
            '"mi_trigger"': int_value
            '"mi_actor_data_any"': int_value
            '"mi_difficulty_level_map"': int_value
            '"mi_fixed"': float_value
            '"mi_location"': vec3
            '"mi_variable"': string
            '"mi_actor_named"': string
            '"mi_kingdom_named"': string
            '"mi_hero_named"': string
            '"mi_objective"': string
            '"mi_ability"': string
            '"mi_animation"': string
            '"mi_technology"': string
            '"mi_command"': string
            '"mi_vfx"': string
            '"mi_sound"': string
            '"mi_speech"': string
            '"mi_music"': string
            '"mi_actor_data_named"': string
            '"mi_organization_data"': string
            '"mi_unit_data"': string
            '"mi_building_data"': string
            '"mi_transported_actor_data"': string
            '"mi_sai_data"': string
            '"mi_flare_data_location"': string
            '"mi_flare_data_actor"': string
            '"mi_string"': string
            '"mi_localized_string"': string
            '"mi_difficulty_level_specified"': string
            '"mi_weather"': string
            '"mi_sun"': string
            '"mi_script_command"': string
            '"mi_cutscene"': string
            '"mi_formation"': string
            '"mi_resource"': string
        doc: |
          An integer, or the id or index of what the source names; a fixed
          number; a location; or a name or string id.

  # Teams, kingdoms and restriction groups.
  restriction_groups:
    doc: |
      RESTRICTION_GROUPS, what the teams set to each group may build and
      research, as set in the editor.
    seq:
      - id: version
        type: u4
        valid: 0
      - id: next_number
        type: u4
        doc: What the editor numbers the next group it creates.
      - id: num_groups
        type: u4
      - id: groups
        type: restriction_group
        repeat: expr
        repeat-expr: num_groups
  restriction_group:
    seq:
      - id: version
        type: u4
        valid: 4
      - id: name
        type: string
        doc: E.g. `restriction_group_0003`, by which teams refer to it.
      - id: number
        type: u4
        doc: The number in its name.
      - id: display_name
        type: string
        doc: A string id; the same as the name in every shipped map.
      - id: allowed_actors
        type: strings
        doc: Actor templates.
      - id: allowed_technologies
        type: strings
      - id: required_actors
        type: strings
      - id: required_technologies
        type: strings
  teams:
    doc: |
      TEAMS, the sides kingdoms join, sharing victory. Relations are listed
      by the position of the kingdom or team they're with.
    seq:
      - id: num_teams
        type: u4
      - id: teams
        type: team(_index)
        repeat: expr
        repeat-expr: num_teams
  team:
    params:
      - id: index
        type: u4
    seq:
      - id: present
        type: flag
      - id: head
        type: thing_head(5)
        if: present.value == 1
      - id: version
        type: u4
        valid:
          max: 1
        if: present.value == 1
      - id: index_again
        type: u4
        valid: index
        if: present.value == 1
        doc: The team's position, which the game requires.
      - id: display_name
        type: string
        if: present.value == 1
        doc: A string id.
      - id: kingdom_relations
        type: u4s
        if: present.value == 1
      - id: team_relations
        type: u4s
        if: present.value == 1
      - id: restriction_group
        type: string
        if: present.value == 1 and version >= 1
  kingdoms:
    doc: |
      KINGDOMS, the players, human or AI, and the independent powers such
      as neutral civilians. Relations are listed by the position of the
      kingdom or team they're with; a kingdom's relation to itself is 0.
    seq:
      - id: num_majors
        type: u4
        doc: How many kingdoms are major, that is not independent.
      - id: num_kingdoms
        type: u4
      - id: kingdoms
        type: kingdom_slot(_index)
        repeat: expr
        repeat-expr: num_kingdoms
  kingdom_slot:
    params:
      - id: index
        type: u4
    seq:
      - id: present
        type: flag
      - id: kingdom
        type: kingdom(index)
        if: present.value == 1
  kingdom:
    params:
      - id: index
        type: u4
    seq:
      - id: thing
        type: actor_thing(4)
        doc: Its actor holds only a Properties component, with no properties.
      - id: updatable_version
        type: u4
        valid: 1
        if: _root.world_version >= 22
      - id: updatable_id
        type: s4
        if: _root.world_version >= 22
      - id: version
        type: u4
        valid:
          min: 13
          max: 20
        doc: Earlier versions are undecoded.
      - id: index_again
        type: u4
        valid: index
        doc: The kingdom's position, which the game requires.
      - id: major_index
        type: s4
        doc: Its position among the major kingdoms; -1 if independent.
      - id: independent
        type: u4
        doc: 1 if independent.
      - id: name
        type: string
      - id: color
        type: string
      - id: relations
        type: relations
        doc: To each kingdom, by position.
      - id: resources
        type: amounts
        doc: Stockpiles.
      - id: team
        type: u4
        doc: The team's thing id; 0 for none.
      - id: team_relations
        type: relations
        doc: To each team, by position.
      - id: use_default_stockpile
        type: flag
        doc: Whether the mission's start replaces the stockpiles with the game's default amount.
      - id: recruits_version
        type: u4
        valid: 1
      - id: num_recruits
        type: u4
        valid: 0
      - id: current_recruit
        type: s4
        doc: The recruit being worked on; -1 for none.
      - id: has_faction
        type: u1
      - id: faction
        type: faction
        if: has_faction != 0
        doc: The commander, set on campaign maps and chosen in the lobby otherwise.
      - id: nation
        type: string
        doc: The nation id, e.g. `nation_gbr`; empty when chosen in the lobby.
      - id: objectives
        type: string_u4s
        doc: Its objectives' states by objective name; 0 active, 1 completed, 2 failed.
      - id: outcome
        type: u4
        doc: 0 still playing, 1 won, 2 lost.
      - id: start_technologies
        type: strings
        if: version >= 14
        doc: Technologies it's given when the mission starts.
      - id: num_properties
        type: u4
      - id: properties
        type: property(0)
        repeat: expr
        repeat-expr: num_properties
        doc: |
          Shared properties it has, such as technologies, laid out as in an
          actor's Properties component without the class, which is 0.
      - id: required
        type: flag
        doc: It must be in the game.
      - id: playable
        type: flag
        doc: A human may play it.
      - id: ai_profile
        type: string
        doc: The strategic AI that plays it, e.g. `ger_sai`; empty for none.
      - id: restriction_group
        type: string
        doc: The restriction group it's in; empty for none.
      - id: stats
        type: kingdom_stats
        if: independent == 0
      - id: num_saved_groups
        type: u4
      - id: saved_groups
        type: saved_group
        repeat: expr
        repeat-expr: num_saved_groups
      - id: instance_numbers
        type: string_u4s
        if: version >= 15
        doc: The next instance number by actor template.
      - id: used_badges
        type: u4s
        if: version >= 16
      - id: default_victory_condition
        type: flag
        if: version >= 17
        doc: Whether the usual victory check applies, which triggers can turn off.
      - id: resigned
        type: flag
        if: version >= 18
      - id: city_ratio_victory
        type: flag
        if: version >= 19
        doc: Its team won by holding the share of cities the rules require.
      - id: bonus_stockpile
        type: amounts
        if: version >= 20
        doc: Amounts added to the stockpiles when the mission starts.
  relations:
    seq:
      - id: num_items
        type: u4
      - id: items
        type: u4
        enum: relation
        repeat: expr
        repeat-expr: num_items
  faction:
    doc: A kingdom's commander and the state of their abilities.
    seq:
      - id: name
        type: string
        doc: The faction id, e.g. `montgomery`.
      - id: version
        type: u4
        valid: 0
      - id: experience
        type: f4
        doc: The commander's experience, which abilities cost.
      - id: num_abilities
        type: u4
      - id: abilities
        type: ability
        repeat: expr
        repeat-expr: num_abilities
  ability:
    doc: The state of a commander's or an actor's ability.
    seq:
      - id: version
        type: u4
        valid:
          max: 1
      - id: use_count
        type: u4
      - id: last_used
        type: f4
        doc: The game time it was last used; -1000 or the lowest f4 for never.
      - id: unknown
        type: u4
        doc: Unknown; 0 in every shipped map.
      - id: unknown2
        type: u1
        doc: Unknown; 1 in every shipped map.
      - id: active_targets
        type: u4s
        if: version >= 1
        doc: The things it's acting on.
  saved_group:
    doc: A selection of units the player saved, to recall with a key.
    seq:
      - id: version
        type: u4
        valid: 0
      - id: name
        type: string
        doc: Its digits are the group's number.
      - id: unknown
        type: string
        doc: Unknown.
      - id: ids
        type: u4s
        doc: The actors' thing ids.
      - id: grouped
        type: flag
        doc: Whether the units move together as one group.
      - id: group_thing
        type: u4
        doc: The thing that leads them when they do.
  kingdom_stats:
    doc: A kingdom's statistics, which the score screen and graph show.
    seq:
      - id: favorites
        type: favorites
      - id: totals
        type: totals
      - id: num_snapshots
        type: u4
      - id: snapshots
        type: snapshot
        repeat: expr
        repeat-expr: num_snapshots
        doc: Taken every 60 seconds of game time, for the score graph.
      - id: time_since_snapshot
        type: f4
        doc: Seconds toward the next snapshot.
  favorites:
    doc: |
      Tallies from which the game picks the kingdom's favourite unit, hero,
      building, enemy and, from version 2, organization: a list per
      category by template, then from version 1 a list per category by
      thing id.
    seq:
      - id: version
        type: u4
        valid:
          max: 2
      - id: by_template
        type: template_tallies(version)
        repeat: expr
        repeat-expr: 'version >= 2 ? 5 : 4'
      - id: by_id
        type: id_tallies
        repeat: expr
        repeat-expr: 'version >= 2 ? 5 : 4'
        if: version >= 1
  template_tallies:
    params:
      - id: version
        type: u4
    seq:
      - id: num_items
        type: u4
      - id: items
        type: template_tally(version)
        repeat: expr
        repeat-expr: num_items
  template_tally:
    params:
      - id: version
        type: u4
    seq:
      - id: template
        type: string
      - id: value_v0
        type: u4
        if: version == 0
        doc: Ignored by the game.
      - id: value
        type: f4
        if: version >= 1
  id_tallies:
    seq:
      - id: num_items
        type: u4
      - id: items
        type: id_tally
        repeat: expr
        repeat-expr: num_items
  id_tally:
    seq:
      - id: id
        type: u4
      - id: value
        type: f4
  totals:
    doc: |
      Running totals: each resource's total production, ten slots in the
      game's resource order (`money`, `ammo`, `oil`, then the unit and
      building limits provided and consumed), then a counter per statistic
      in the order of the ids in the game's `Scoring/Scoring.tgi`
      (`TRIBUTE_GIVEN`, `TRIBUTE_RECEIVED`, ...). Version 0 leaves out the
      statistics from `CITIES_CAPTURED` to `FACTION_ABILITIES_USED` and
      `CITIES_OWNED`. The slots of the counts the game works out live (the
      `..._OWNED` ones) hold leftover memory.
    seq:
      - id: version
        type: u4
        valid:
          max: 1
      - id: values
        type: f4
        repeat: expr
        repeat-expr: 'version >= 1 ? 66 : 60'
  snapshot:
    doc: A sample for the score graph.
    seq:
      - id: version
        type: u4
        valid: 0
      - id: totals
        type: totals
      - id: values
        type: f4
        repeat: expr
        repeat-expr: 21
        doc: Each resource's rate, ten slots; the hero inventory's size; then the scores.

  # Things: the placed objects.
  thing_index:
    doc: |
      DYNAMIC_THINGS_INDEX or USED_REQUIRED_OBJECTS_INDEX, the size of each
      record in the matching thing list.
    seq:
      - id: data_version
        type: u4
        valid: 0
      - id: num_records
        type: u4
      - id: record_sizes
        type: u4
        repeat: expr
        repeat-expr: num_records
  thing_list:
    doc: |
      DYNAMIC_THINGS, the objects placed on the map, or
      USED_REQUIRED_OBJECTS, which no shipped map has any of. The game reads
      the latter from world version 25.
    params:
      - id: index_name
        type: str
    seq:
      - id: data_version
        type: u4
        valid: 0
      - id: version
        type: u4
        valid: 11
      - id: records
        type: thing
        size: index.record_sizes[_index]
        repeat: expr
        repeat-expr: index.num_records
    instances:
      index:
        value: _root.sections[index_at.index].body.as<section_body>.value.as<thing_index>
      index_at:
        pos: 0
        size: 0
        type: find_section(index_name, 0)
  thing_head:
    doc: What every saved object's record starts with.
    params:
      - id: class_wanted
        type: s4
        doc: The class the context requires, or -1 for any.
    seq:
      - id: template
        type: string
        doc: The template's name, e.g. `temperate_trees`, `start_loc`.
      - id: class_id
        type: u4
        enum: thing_class
        valid:
          expr: class_wanted < 0 or _.to_i == class_wanted
        doc: The kind of object, which decides the rest of the record.
      - id: id
        type: u4
      - id: version
        type: u4
        valid: 2
      - id: name
        type: string
        doc: The name scripts refer to it by; empty for most.
      - id: unknown
        type: u4
        valid: 0
      - id: position
        type: vec3
      - id: facing
        type: vec3
        doc: A unit vector in x and y; cameras add their tilt in z.
      - id: flags
        type: u4
  placement:
    doc: What follows the head in a thing record.
    seq:
      - id: version
        type: u4
        valid: 5
      - id: scale
        type: f4
        doc: Size relative to the template's.
      - id: variation
        type: s4
        doc: Which of the template's model variations it shows; -1 for none.
      - id: unknown
        type: u4
        valid: 0
      - id: unknown2
        type: u4
        doc: Unknown.
      - id: unknown3
        type: u2
        valid: 0
  thing:
    doc: A record in a thing list, sized by its index.
    seq:
      - id: head
        type: thing_head(-1)
      - id: placement
        type: placement
      - id: detail
        size-eos: true
        type:
          switch-on: head.class_id
          cases:
            thing_class::actor: actor
            thing_class::area: area
            thing_class::camera: camera
            _: undecoded
        doc: Other classes' details are undecoded.
  actor_thing:
    doc: |
      A thing record whose detail is an actor, inside another record with
      no size of its own: a kingdom, or an organization's member.
    params:
      - id: class_wanted
        type: s4
    seq:
      - id: head
        type: thing_head(class_wanted)
      - id: placement
        type: placement
      - id: actor
        type: actor
  area:
    doc: A region scripts and triggers test units against.
    seq:
      - id: version
        type: u4
      - id: id
        type: u4
        if: has_id
      - id: id_version
        type: u4
        valid: 2
        if: has_id
      - id: name
        type: string
        doc: The name the map's objectives and triggers use.
      - id: shape
        type: u4
        doc: 1 for a circle, 3 for a box.
      - id: radius
        type: f4
      - id: bounds
        type: f4
        repeat: expr
        repeat-expr: 4
        doc: The box about the position, min x, min y, max x, max y.
      - id: unknown
        type: u4
        valid: 0
    instances:
      name_len_without_id:
        pos: 4
        type: u2
      has_id:
        value: name_len_without_id * 2 != _io.size - 34
        doc: |
          Some maps, such as the cutscenes, leave out the id; the name's
          length tells which.
  camera:
    doc: A camera position for cutscenes and scripted views.
    seq:
      - id: version
        type: u4
        doc: 1 or 2.
      - id: name
        type: string
      - id: unknown
        type: f4
        doc: Unknown.

  # Actors: things made of components, such as units, buildings and herds.
  actor:
    doc: |
      The components' state follows the header with no lengths, in record
      order: Construction first, Mind last, the others by bit. Where a
      component's layout is undecoded, the state is kept as bytes; that
      only works where the record's size is known.
    seq:
      - id: version
        type: u4
        valid:
          min: 9
          max: 14
        doc: Earlier versions are undecoded.
      - id: kingdom
        type: u4
        doc: The owning kingdom's thing id; 0 for none.
      - id: flags
        type: u8
      - id: template_components
        type: u4
        doc: The components its template gives it, a bit each (see `component`).
      - id: components
        type: u4
        doc: |
          The components the record holds state for: the template's, plus
          any the game adds, such as a mind.
      - id: slots
        type: component_slot(_index)
        repeat: expr
        repeat-expr: 31
        if: is_decoded
        doc: One per component bit, in record order.
      - id: trailer
        type: actor_trailer(version)
        if: is_decoded
      - id: state
        size-eos: true
        if: not is_decoded
        doc: The undecoded state and trailer.
    instances:
      is_decoded:
        value: |
          (components & 0x0c805002) == 0 and ((components & 0x10) == 0
            or (template_components & 0x80) != 0
            or ((template_components & 0x800000) == 0 and (template_components & 0x100) != 0))
        doc: |
          Whether every component's layout is decoded: not a Projectile,
          Wall, Settlement, Aircraft, Flare or MetaGame, nor an aircraft's
          mind.
  component_slot:
    params:
      - id: slot
        type: u4
    seq:
      - id: state
        type: component(bit, _parent.template_components)
        if: ((_parent.components >> bit) & 1) != 0
    instances:
      bit:
        value: '(slot == 0 ? 15 : (slot == 30 ? 4 : (slot < 5 ? slot - 1 : (slot < 15 ? slot : slot + 1))))'
  component:
    params:
      - id: bit
        type: u4
      - id: template_components
        type: u4
    seq:
      - id: state
        type:
          switch-on: bit
          cases:
            0: body
            2: character
            3: mover
            4: mind(template_components)
            5: performer
            6: properties
            7: organization
            8: element
            9: plain_component
            10: denizen
            11: supply
            13: structure
            15: construction
            16: recruit
            17: experience
            18: plain_component
            19: mobile_deployment
            20: defender
            21: research
            22: plain_component
            24: transport
            25: lair
            28: plain_component
            29: city
            30: hq
        doc: |
          By bit: 0 Body, 1 Projectile, 2 Character, 3 Mover, 4 Mind,
          5 Performer, 6 Properties, 7 Organization, 8 Element, 9 Economy,
          10 Denizen, 11 Supply, 12 Wall, 13 Structure, 14 Settlement,
          15 Construction, 16 Recruit, 17 Experience, 18 Builder,
          19 MobileDeployment, 20 Defender, 21 Research, 22 Detection,
          23 Aircraft, 24 Transport, 25 Lair, 26 Flare, 27 MetaGame,
          28 Portal, 29 City, 30 HQ; as the templates' `[...Component]`
          sections name them.
  actor_trailer:
    doc: What follows an actor's components.
    params:
      - id: version
        type: u4
    seq:
      - id: main_component
        type: s4
        doc: The bit of the component in charge; -1 for none.
      - id: unknown1
        type: u4
        if: version >= 2
      - id: unknown2
        type: u4
        if: version >= 3 and version < 12
      - id: has_unknown_string
        type: flag
        if: version >= 4
      - id: unknown_string
        type: string
        if: version >= 4 and has_unknown_string.value == 1
      - id: has_unknown3
        type: flag
        if: version >= 5
      - id: unknown3
        type: u4
        if: version >= 5 and has_unknown3.value == 1
      - id: unknown_string2
        type: string
        if: version >= 6
      - id: unknown4
        type: u4
        repeat: expr
        repeat-expr: 2
        if: version >= 8
      - id: unknown5
        type: u4
        if: version >= 10
      - id: lists
        type: u4s
        repeat: expr
        repeat-expr: 3
        if: version >= 11
      - id: num_triples
        type: u4
        if: version >= 11
      - id: triples
        type: u4
        repeat: expr
        repeat-expr: num_triples * 3
        if: version >= 11
      - id: unknown6
        type: u4
        if: version >= 13
  updatable:
    doc: The component's id among the world's updatable objects.
    seq:
      - id: version
        type: u4
        valid:
          max: 1
      - id: id
        type: s4

  # The decoded components.
  construction:
    doc: A building's construction or upgrade progress.
    seq:
      - id: updatable
        type: updatable
      - id: version
        type: u4
        valid:
          max: 1
      - id: progress
        type: f4s
      - id: upgrade_progress
        type: f4s
        if: version >= 1
      - id: upgrade_template
        type: string
        if: version >= 1
        doc: The template being upgraded to; empty for none.
      - id: unknown
        type: u4
        repeat: expr
        repeat-expr: 2
        if: version == 0
  body:
    seq:
      - id: version
        type: u4
        valid:
          max: 4
      - id: updatable
        type: updatable
        if: version >= 2
      - id: unknown
        type: f4
        if: version >= 1
      - id: fraction
        type: f4
        doc: A fraction the game multiplies by the template's maximum.
      - id: unknown2
        type: u4
        if: version >= 3
      - id: unknown3
        type: u4
        if: version >= 4
  mover:
    doc: Fields are unknown unless noted.
    seq:
      - id: updatable
        type: updatable
      - id: version
        type: u4
        valid:
          max: 8
      - id: unknown_vector
        type: vec3
      - id: unknown
        type: f4
        repeat: expr
        repeat-expr: 2
      - id: fraction
        type: f4
        if: version >= 8
        doc: A fraction the game multiplies by a template value.
      - id: old
        type: vec3
        if: version < 4
        doc: Ignored by the game.
      - id: unknown2
        type: f4
        repeat: expr
        repeat-expr: 2
      - id: old_flag
        type: u1
        if: version >= 1 and version < 7
      - id: flag
        type: u1
        if: version >= 2
      - id: unknown3
        type: vec3
        if: version >= 3
      - id: unknown4
        type: f4
        if: version >= 3
      - id: flag2
        type: u1
        if: version >= 5
        doc: Also set in the actor's flags.
      - id: unknown5
        type: u4
        if: version >= 5
      - id: v6_flag
        type: u1
        if: version >= 6
      - id: v6_a
        type: vec3
        if: version >= 6
      - id: v6_flag2
        type: u1
        if: version >= 6
      - id: v6_b
        type: vec3
        if: version >= 6
      - id: v6_c
        type: vec3
        if: version >= 6
  properties:
    seq:
      - id: updatable
        type: updatable
      - id: version
        type: u4
        valid:
          max: 1
      - id: num_properties
        type: u4
      - id: properties
        type: property_slot
        repeat: expr
        repeat-expr: num_properties
  property_slot:
    seq:
      - id: property_class
        type: u4
        valid:
          expr: _ <= 1 or _ == 0xffffffff
        doc: 0, or 1 for the kind with an extra value; 0xFFFFFFFF for none.
      - id: property
        type: property(property_class)
        if: property_class != 0xffffffff
  property:
    doc: A shared property, such as a nation's technology.
    params:
      - id: property_class
        type: u4
    seq:
      - id: version
        type: u4
        valid:
          max: 3
      - id: unknown
        type: u4
      - id: unknown2
        type: u4
        if: version >= 1
      - id: id
        type: string
        doc: The shared property data's id.
      - id: unknown3
        type: u1
        if: version >= 2
      - id: unknown4
        type: u4
        if: version >= 2
      - id: extra_version
        type: u4
        valid: 0
        if: property_class == 1 and version >= 3
      - id: extra
        type: u4
        if: property_class == 1 and version >= 3

  # Components read to their end but not named. Their fields are unknown
  # unless noted.
  plain_component:
    doc: Economy, Builder, Detection and Portal.
    seq:
      - id: updatable
        type: updatable
      - id: version
        type: u4
        valid: 0
  character:
    seq:
      - id: updatable
        type: updatable
      - id: version
        type: u4
        valid:
          max: 2
      - id: unknown1
        type: u4
      - id: unknown2
        type: u4
        if: version < 1
      - id: animation
        size: 8
        doc: The animation's type and index.
      - id: animation_version
        type: u4
        valid: 0
      - id: animation_state
        size: 13
      - id: unknown3
        type: u4
        if: version >= 2
  performer:
    seq:
      - id: updatable
        type: updatable
      - id: version
        type: u4
        valid:
          min: 4
          max: 9
        doc: Earlier versions are undecoded.
      - id: unknown1
        size: 24
      - id: unknown2
        type: u4s
        if: version < 5
      - id: unknown3
        size: 8
      - id: num_entries
        type: u4
      - id: entries
        type: performer_entry(version)
        repeat: expr
        repeat-expr: num_entries
      - id: unknown4
        type: u4
      - id: unknown5
        type: u4
        if: version >= 6
      - id: num_abilities
        type: u4
        if: version >= 5
      - id: abilities
        type: ability
        repeat: expr
        repeat-expr: num_abilities
        if: version >= 5
      - id: unknown6
        type: u1
        if: version >= 7
  performer_entry:
    params:
      - id: version
        type: u4
    seq:
      - id: unknown1
        size: 8
      - id: unknown2
        size: 4
        if: version >= 7
      - id: unknown3
        size: 9
        if: version >= 8
      - id: unknown4
        size: 4
        if: version >= 9
  damage:
    doc: The damage an element deals; its tail depends on the world version.
    seq:
      - id: unknown1
        size: 8
      - id: name
        type: string
      - id: unknown2
        size: 8
      - id: unknown3_v15
        type: u1
        if: _root.world_version >= 15 and _root.world_version < 18
      - id: unknown3
        type: u4
        if: _root.world_version >= 18
      - id: unknown4
        type: u4
        if: _root.world_version >= 20
  element:
    seq:
      - id: version
        type: u4
        valid:
          max: 3
      - id: updatable
        type: updatable
        if: version >= 1
      - id: unknown
        type: u1
        if: version <= 1
      - id: num_damages
        type: u4
        if: version >= 3
      - id: damages
        type: damage
        repeat: expr
        repeat-expr: num_damages
        if: version >= 3
  organization:
    doc: A herd or regiment, holding its members' whole records.
    seq:
      - id: updatable
        type: updatable
      - id: version
        type: u4
        valid:
          max: 10
      - id: layout_version
        type: u4
        valid: 0
      - id: layout
        type: string
        doc: The `OrganizationLayout` the members are arranged by.
      - id: slots
        type: strings
        doc: Each layout slot's element template; empty for an empty slot.
      - id: num_positions
        type: u4
      - id: positions
        type: organization_position
        repeat: expr
        repeat-expr: num_positions
        doc: The members where they stand.
      - id: unknown1
        type: u4
        repeat: expr
        repeat-expr: num_positions
        if: version >= 2 and version <= 6
      - id: position_templates
        type: string
        repeat: expr
        repeat-expr: num_positions
        if: version >= 7
      - id: unknown2
        size: 12
      - id: unknown3
        type: u4
        if: version >= 3
      - id: unknown4
        type: u4
        if: version == 0
      - id: unknown5
        size: num_positions * 8
      - id: unknown6
        size: num_positions * 4
        if: version >= 1
      - id: unknown7
        size: 6
        if: version >= 4
      - id: damage
        type: damage
        if: version >= 4
      - id: unknown8
        size: 4
        if: version >= 5
      - id: unknown9
        size: 4
        if: version >= 6
      - id: unknown10
        size: 8
        if: version >= 8
      - id: unknown11
        size: 4
        if: version >= 9
      - id: unknown12
        size: 4
        if: version >= 10
  organization_position:
    seq:
      - id: index
        type: s4
        doc: -1 for an empty position.
      - id: member
        type: actor_thing(3)
        if: index >= 0
  denizen:
    seq:
      - id: updatable
        type: updatable
      - id: version
        type: u4
        valid:
          max: 4
      - id: unknown1
        type: u1
        if: version >= 1
      - id: unknown2
        type: u1
        if: version >= 2
      - id: num_unknown3
        type: u4
        if: version >= 3
      - id: unknown3
        size: num_unknown3 * 8
        if: version >= 3
      - id: unknown4
        type: u4
        if: version >= 4
      - id: unknown5
        type: u4s
        if: version >= 3
      - id: unknown6
        type: u4s
        if: version >= 3
  supply:
    seq:
      - id: version
        type: u4
        valid:
          max: 2
      - id: updatable
        type: updatable
        if: version >= 1
      - id: unknown
        size: 8
        if: version >= 2
  structure:
    seq:
      - id: version
        type: u4
        valid:
          max: 13
      - id: updatable
        type: updatable
        if: version >= 6
      - id: unknown1
        type: u4
      - id: unknown2
        type: strings
        if: version < 1
      - id: num_queued
        type: u4
        valid: 0
        if: version >= 2
        doc: Queued items' layout is undecoded; no shipped map has any.
      - id: unknown3
        size: 8
        if: version >= 2
      - id: unknown4
        type: u4
        if: version >= 11
      - id: unknown5
        type: u4
        if: version >= 12
      - id: unknown6
        type: u4
        if: version >= 7
  layout:
    seq:
      - id: id
        type: string
      - id: version
        type: u4
        valid: 0
      - id: unknown1
        type: string
      - id: unknown2
        type: strings
  recruit:
    seq:
      - id: updatable
        type: updatable
      - id: version
        type: u4
        valid:
          max: 3
      - id: has_unknown1
        type: u1
        if: version >= 1
      - id: unknown1
        size: 8
        if: version >= 1 and has_unknown1 != 0
      - id: num_entries
        type: u4
        if: version >= 1 and version <= 2
      - id: entries
        type: recruit_entry
        repeat: expr
        repeat-expr: num_entries
        if: version >= 1 and version <= 2
      - id: unknown2
        type: u4
        if: version >= 1 and version <= 2
      - id: unknown3
        type: u1
        if: version == 2
  recruit_entry:
    seq:
      - id: unknown
        type: string
      - id: has_layout
        type: u1
      - id: layout
        type: layout
        if: has_layout != 0
  experience:
    seq:
      - id: version
        type: u4
        valid: 0
      - id: unknown
        type: u4
  mobile_deployment:
    seq:
      - id: updatable
        type: updatable
      - id: version
        type: u4
        valid:
          max: 3
      - id: unknown1
        type: u4
      - id: unknown2
        type: u4
        if: version >= 1
      - id: unknown3
        type: id_list
        if: version == 2
  id_list:
    seq:
      - id: version
        type: u4
        valid: 0
      - id: ids
        type: u4s
  defender:
    seq:
      - id: updatable
        type: updatable
      - id: version
        type: u4
        valid:
          max: 2
      - id: num_abilities
        type: u4
        if: version >= 1
      - id: abilities
        type: ability
        repeat: expr
        repeat-expr: num_abilities
        if: version >= 1
      - id: num_unknown
        type: u4
        if: version >= 2
      - id: unknown
        size: num_unknown * 8
        if: version >= 2
  research:
    seq:
      - id: updatable
        type: updatable
      - id: version
        type: u4
        valid: 0
      - id: unknown
        type: strings
  transport:
    seq:
      - id: version
        type: u4
        valid:
          max: 6
      - id: unknown1
        type: string
      - id: has_layout
        type: u1
      - id: layout
        type: layout
        if: has_layout != 0
      - id: unknown2
        size: 18
      - id: unknown3
        type: u4s
        if: version == 0
      - id: unknown4
        type: u4
        if: version >= 4
      - id: unknown5
        type: u4
      - id: unknown6
        type: id_list
      - id: unknown7
        type: id_list
      - id: has_unknown8
        type: u1
        if: version >= 3
      - id: unknown8
        size: 8
        if: version >= 3 and has_unknown8 != 0
      - id: unknown9
        type: string
        if: version >= 5
      - id: unknown10
        type: u4
        if: version >= 6
  lair:
    seq:
      - id: updatable
        type: updatable
      - id: version
        type: u4
        valid: 0
      - id: unknown1
        type: u4s
      - id: unknown2
        size: 8
  city:
    seq:
      - id: updatable
        type: updatable
      - id: version
        type: u4
        valid:
          max: 1
      - id: unknown
        type: u4s
        if: version >= 1
  hq:
    seq:
      - id: updatable
        type: updatable
      - id: version
        type: u4
        valid:
          max: 3
      - id: badge
        type: s4
      - id: unknown1
        type: u4s
        if: version == 1
      - id: num_entries
        type: u4
        if: version >= 2
        doc: |
          Version 0 records end before this in every map, though the game's
          loader reads on as for version 2.
      - id: entries
        type: hq_entry(version)
        repeat: expr
        repeat-expr: num_entries
        if: version >= 2
  hq_entry:
    params:
      - id: version
        type: u4
    seq:
      - id: present
        type: u1
      - id: unknown
        size: 'version < 3 ? 5 : 12'
        if: present != 0

  # Minds: an organization's or an element's, as the template's components
  # decide. Aircraft minds are undecoded.
  mind:
    params:
      - id: template_components
        type: u4
    seq:
      - id: state
        type:
          switch-on: (template_components & 0x80) != 0
          cases:
            true: organization_mind
            false: element_mind
  mind_base:
    doc: What every mind's state starts with.
    seq:
      - id: updatable
        type: updatable
      - id: version
        type: u4
        valid:
          max: 3
      - id: unknown1
        size: 8
      - id: num_commands
        type: u4
      - id: commands
        type: command
        repeat: expr
        repeat-expr: num_commands
      - id: unknown2
        type: u4
        if: version >= 1
      - id: states
        type: strings
        if: version >= 1
        doc: The names of its mind states.
      - id: has_command
        type: u1
        if: version >= 3
      - id: command
        type: command
        if: version >= 3 and has_command != 0
  command:
    doc: An order queued for an actor.
    seq:
      - id: version
        type: u4
        valid:
          max: 3
      - id: unknown1
        type: string
      - id: flags
        type: u4
      - id: unknown2
        type: u4
      - id: unknown3
        type: string
      - id: unknown4
        size: 21
      - id: unknown5
        type: u1
        if: version >= 3
      - id: unknown6
        type: string
        if: version >= 1
      - id: layout
        type: layout
        if: (flags & 0x20) != 0
      - id: unknown7
        type: string
        if: version >= 2
  path:
    seq:
      - id: version
        type: u4
        valid: 0
      - id: num_points
        type: u4
      - id: points
        size: num_points * 36
      - id: unknown
        type: u4
  element_mind:
    seq:
      - id: version
        type: u4
        valid:
          max: 14
      - id: base
        type: mind_base
        if: version >= 1
      - id: rest
        type: element_mind_rest(version)
        if: version >= 2
  element_mind_rest:
    params:
      - id: version
        type: u4
    seq:
      - id: unknown1
        size: 8
      - id: unknown2
        type: u4
        if: version >= 9
      - id: unknown3
        size: 52
      - id: unknown4
        type: u4
        if: version >= 12
      - id: path
        type: path
      - id: unknown5
        size: 24
        if: version >= 10
      - id: unknown6
        type: u1
        if: version >= 5 and version < 7
      - id: unknown7
        type: u4
        if: version >= 7
      - id: unknown8
        size: 8
      - id: unknown9
        size: 29
        if: version < 4
      - id: unknown10
        size: 26
      - id: unknown11
        type: u1
        if: version >= 8
      - id: unknown12
        size: 12
      - id: unknown13
        type: u4
        if: version >= 3
      - id: unknown14
        type: u4
        if: version >= 4
      - id: unknown15
        type: u1
        if: version >= 4 and version < 6
      - id: unknown16
        size: 12
        if: version >= 4
      - id: unknown17
        type: string
        if: version >= 4
      - id: unknown18
        size: 12
        if: version >= 9
      - id: unknown19
        size: 8
        if: version >= 13
      - id: unknown20
        size: 20
        if: version >= 11
      - id: unknown21
        size: 8
        if: version >= 14
  organization_mind:
    seq:
      - id: version
        type: u4
        valid:
          max: 21
      - id: base
        type: mind_base
        if: version >= 1
      - id: unknown
        size: 8
        if: version >= 1
      - id: rest
        type: organization_mind_rest(version)
        if: version >= 2
  organization_mind_rest:
    params:
      - id: version
        type: u4
    seq:
      - id: unknown1
        type: u4s
      - id: unknown2
        type: u4
      - id: unknown3
        size: 5
        if: version >= 9
      - id: unknown4
        type: u1
        if: version >= 12
      - id: unknown5
        size: 37
      - id: unknown6
        size: 8
        if: version >= 15
      - id: path
        type: path
      - id: unknown7
        type: u4
        if: version >= 3
      - id: unknown8
        size: 16
        doc: |
          Holds a u4, 0 in every map, that the game's loader doesn't appear
          to read; its place among these bytes is a guess.
      - id: unknown9
        type: u1
        if: version < 8
      - id: unknown10
        type: u4
      - id: unknown11
        type: string
        if: version >= 6
      - id: unknown12
        size: 8
        if: version >= 6
      - id: unknown13
        size: 8
        if: version >= 4
      - id: unknown14
        type: u4
        if: version >= 5 and version < 13
      - id: unknown15
        size: 13
      - id: unknown16
        type: string
      - id: unknown17
        size: 8
      - id: unknown18
        type: u4
        if: version >= 11
      - id: unknown19
        size: 8
        if: version >= 20
      - id: unknown20
        size: 9
      - id: unknown21
        type: u1
        if: version >= 10
      - id: unknown22
        size: 45
      - id: num_unknown23
        type: u4
      - id: unknown23
        size: num_unknown23 * 8
      - id: unknown24
        type: u4
        if: version >= 18
      - id: unknown25
        type: u1
        if: version >= 19
      - id: unknown26
        type: u1
        if: version >= 5
      - id: unknown27
        size: 52
      - id: unknown28
        size: 12
        if: version >= 17
      - id: unknown29
        size: 9
      - id: unknown30
        type: u1
        if: version >= 7
      - id: unknown31
        type: u4s
        if: version >= 7
      - id: unknown32
        type: u4
      - id: unknown33
        size: 12
        if: version >= 14
      - id: unknown34
        size: 8
        if: version >= 21
      - id: unknown35
        type: u4
        if: version >= 16
      - id: unknown36
        type: u4
        if: version >= 14
enums:
  relation:
    0: itself
    1: ally
    2: neutral
    3: enemy
  thing_class:
    3: actor
    4: kingdom
    5: team
    6: area
    7: camera
  rule_type:
    0: bool
    1: int
    2: float
    3: choice
