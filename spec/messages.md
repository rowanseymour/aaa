# Messages

The message log: the short lines of text, each behind a round icon, that
appear at the top left of the screen during play, what posts them, who sees
them, how they come and go, and the history panel that keeps them.

Games: traced in AA. Kohan II runs the same code, but its data hasn't been
checked against these rules.

## Message kinds

- Every message has a kind, one of the `MessageType` entries in
  `Game/message_types.tgi` (`Templates/template_message_type.tgi` gives the
  fields). The kind decides the icon, the text colour, whether a filter
  setting can hide it, a sound, and what clicking the icon does. [code, data]
- The game refers to kinds by their position in that file, so the order of
  its entries matters: `HERO_POSITIVE` first, then `HERO_NEUTRAL`,
  `HERO_KILLED`, `COMPANY_POSITIVE` ... down to `TUTORIAL` last, as shipped.
  [code, data]
- `icon` is the picture in front of the line, a path under `Game/`
  (`MessageTypeIcons/IconGenericEvent.png`), 16 by 17 pixels. The folder
  also holds `.tga` copies and icons no kind uses (`IconHero*`,
  `IconRegiment*`, `IconMetaGameError`, `IconMetaGameEvent`,
  `IconMetaGameWarning`). [data]
- `color` is the line's text colour, white by default. Only `BANNED`
  (magenta) and `TUTORIAL` (white) set one, so in a match every line is
  white unless its text carries its own colour (chat, below). [code, data]
- `filter` names a setting (a UVar such as `UIMessageFilterCompany`); while
  it's false, messages of that kind are dropped as they're posted, so they
  never reach the log or its history either. `NULL` means the kind can't be
  hidden. [code, data]
- `message_sound` names a sound (`Audio/sounds_ui.tgi`) played each time a
  message of that kind is posted during a match. Only `GENERIC_ERROR` has
  one, `error`. [code, data]
- The kinds that matter in a match, with their stock icons:

  | Kind | Icon (`MessageTypeIcons/`) | Filter |
  |------|------|--------|
  | `HERO_POSITIVE`, `HERO_NEUTRAL`, `HERO_KILLED` | `IconGenericEvent.png` | `UIMessageFilterHero` |
  | `COMPANY_POSITIVE`, `COMPANY_GENERIC` | `IconCompanyGeneric.png` | `UIMessageFilterCompany` |
  | `COMPANY_NEGATIVE` | `IconCompanyNegative.png` | `UIMessageFilterCompany` |
  | `COMPANY_ROUTED` | `IconCompanyRouted.png` | `UIMessageFilterCompany` |
  | `COMPANY_DESTROYED` | `IconCompanyDestroyed.png` | `UIMessageFilterCompany` |
  | `REGIMENT_GENERIC` / `_NEGATIVE` / `_DESTROYED` | the matching `IconCompany*.png` | `UIMessageFilterRegiment` |
  | `BUILDING_GENERIC` | `IconBuildingGeneric.png` | `UIMessageFilterBuilding` |
  | `BUILDING_NEGATIVE` | `IconBuildingNegative.png` | none |
  | `BUILDING_CAPTURED` | `IconBuildingCaptured.png` | none |
  | `BUILDING_DESTROYED` | `IconBuildingDestroyed.png` | none |
  | `GENERIC_EVENT` | `IconGenericEvent.png` (blue "!") | `UIMessageFilterGeneric` |
  | `GENERIC_WARNING` | `IconGenericWarning.png` (yellow "!") | none |
  | `GENERIC_ERROR` | `IconGenericError.png` (red cross) | none |
  | `QUEST_POSITIVE` | `IconQuestPositive.png` (green envelope) | none |
  | `QUEST_NEGATIVE` | `IconQuestNegative.png` (red envelope) | none |
  | `QUEST_MESSAGE` | `IconQuestMessage.png` (white envelope) | none |
  | `KINGDOM_DEFEATED` | `IconKingdomDefeated.png` | none |
  | `CHAT_PUBLIC`, `CHAT_PRIVATE`, `LAN_CHAT_*` | `IconChat.png` | `UIMessageFilterChat` |
  | `CHAT_SERVER`, `PLAYER_JOINLEAVE`, `AFK_MESSAGE`, `IM_SERVER` | `IconSystemMessage.png` | `UIMessageFilterChat`, `UIMessageFilterPlayerJoinLeave`, `UIMessageFilterAFK`, none |
  | `BUDDY_MESSAGE` | `IconChat.png` | `UIMessageFilterBuddy` |
  | `TUTORIAL` | `IconGenericEvent.png` | `UIMessageFilterTutorial` |

  The `METAGAME_*` kinds belong to the World War II campaign map, not to
  battles. [data]
- Kohan II has two kinds AA lacks, `TECH_POSITIVE` and `TECH_NEGATIVE`
  (both `IconGenericEvent.png`, no filter), which its `technology_gained`
  and `technology_lost` events use. AA's events of those names use
  `GENERIC_EVENT`. [code, data]
- Every filter setting is true in stock `UVars.tgi`, so nothing is hidden by
  kind unless the player changes it. `UIMessageFilterAFK` isn't in the file
  and defaults to true. [code, data]

## Game events

Most messages in a battle come from game events, each defined in
`Game/game_events.tgi` (fields in `Templates/template_game_event.tgi`).

- How an event is delivered (who is told, its `filter` and
  `message_filter`, fog, loading) is in [game events](game-events.md).
  Here: an event's `game_message` is its line's text, with `%`
  placeholders, posted as the kind its `message_type` names. An empty
  `game_message` posts no line. The texts are in
  `Localization/strings_rtse_data.tgi` under
  `game_events_<event>_game_message`, except `hq_attach`, `hq_detach` and
  `city_ratio_victory`, which are in `strings_data_AA.tgi`. [code, data]
- An event that names a unit or building gives its line that thing's
  position; one that names a place gives it that place. That's the line's
  location, used by its icon and the cycle key (below). [code]
- The shipped events and their lines (the placeholders are filled in the
  order the text has them; the fills are read from the text, apart from
  the ones marked seen and the resource transfers, whose order is
  confirmed in the code):

  | Event | Text | Kind | Notes |
  |-------|------|------|-------|
  | `hq_attach` | "%s: attaching %s" (the HQ, the regiment) | `GENERIC_EVENT` | seen: "1st Infantry Division HQ: attaching 3rd Conscript Regiment" |
  | `hq_detach` | "%s: detaching %s" | `GENERIC_EVENT` | |
  | `display_when_loading_primary_objective` | "Primary Objective gained: %s" (the objective's text) | `QUEST_MESSAGE` | posted when a primary objective is given; seen: "Primary Objective gained: 1. Destroy Rommel's Corps HQ." |
  | `display_when_loading_secondary_objective` | "Secondary Objective gained: %s" | `QUEST_MESSAGE` | |
  | `complete_primary_objective`, `complete_secondary_objective` | "Primary Objective completed: %s", "Secondary ..." | `QUEST_POSITIVE` | |
  | `failed_primary_objective`, `failed_secondary_objective` | "Primary Objective failed: %s", "Secondary ..." | `QUEST_NEGATIVE` | |
  | `remove_primary_objective`, `remove_secondary_objective` | none (`game_message` is empty) | `QUEST_NEGATIVE` | no line |
  | `building_construction_complete` | "%s construction complete" | `BUILDING_GENERIC` | filter `UIEventFilterProduction` |
  | `building_repair_complete` | "%s repair complete" | `BUILDING_GENERIC` | |
  | `building_sold` | "%s sold for %s" | `BUILDING_GENERIC` | |
  | `building_under_siege` | "%s is under siege" | `BUILDING_NEGATIVE` | filter `UIEventFilterSiege`; never posted in AA: a building going under siege ([buildings](buildings.md), Under siege) doesn't raise it [code] |
  | `wall_breached` | "Wall Breached" | `BUILDING_NEGATIVE` | filter `UIEventFilterSiege` |
  | `militia_dispatched` | "%s militia dispatched" | `BUILDING_NEGATIVE` | filter `UIEventFilterSiege` |
  | `static_actor_bombarded` | "%s being bombarded" | `BUILDING_NEGATIVE` | filter `UIEventFilterSiege` |
  | `actor_captured` | "You have captured %s" | `BUILDING_CAPTURED` | see [game events](game-events.md), Capture |
  | `actor_captured_by_enemy` | "%s captured by %s" | `BUILDING_NEGATIVE` | building, taker |
  | `static_actor_lost` | "%s destroyed" | `BUILDING_DESTROYED` | a building of yours; see Destroyed and lost |
  | `mobile_actor_lost` | "%s destroyed" | `COMPANY_DESTROYED` | a regiment or truck of yours |
  | `actor_destroyed` | "%s's %s destroyed" (owner, thing) | `COMPANY_POSITIVE` | an enemy's, whoever destroyed it |
  | `company_recruited` | "%s recruited" | `COMPANY_GENERIC` | a single unit put out, such as a truck; filter `UIEventFilterProduction` |
  | `company_commissioned` | "%s commissioned" | `COMPANY_GENERIC` | a regiment put out, Division HQs included; same filter; seen: "1st Infantry Division HQ commissioned" |
  | `company_experience_gained_level` | "%s has gained a level" | `COMPANY_POSITIVE` | |
  | `company_engaged`, `company_has_been_engaged` | "%s has entered combat", "%s has been engaged" | `COMPANY_GENERIC` | filter `UIEventFilterEngagement`, line `UIEventFilterEngagementMessages` |
  | `company_routed` | "%s has routed from battle" | `COMPANY_ROUTED` | same two filters |
  | `mobile_actor_bombarded` | "%s being bombarded" | `COMPANY_NEGATIVE` | same two filters |
  | `bombard_target_lost_in_fog` | "%s lost bombard target in fog" | `COMPANY_NEGATIVE` | |
  | `kingdom_upkeep_shortage` | "Your production is %d%% short of your costs" | `GENERIC_WARNING` | |
  | `kingdom_upkeep_repaired` | "You have repaired your upkeep" | `GENERIC_EVENT` | |
  | `technology_gained`, `technology_lost` | "%s technology gained", "%s technology lost" | `GENERIC_EVENT` | |
  | `research_complete` | "%s research complete" | `GENERIC_EVENT` | |
  | `my_flare_created`, `my_flare_created_on_actor` | "%s placed", "%s placed on %s" | `GENERIC_EVENT` | flare name, target |
  | `ally_flare_created`, `ally_flare_created_on_actor` | "%s placed by %s", "%s placed on %s by %s" | `GENERIC_EVENT` | |
  | `resources_given`, `resources_received` | "%i%s given to %s", "%i%s received from %s" | `GENERIC_EVENT` | amount (rounded to the nearest whole), resource glyph, side; the giver gets the first, the receiver the second [code] |
  | `actor_given`, `actor_received` | "%s given to %s", "%s received from %s" | `GENERIC_EVENT` | |
  | `resource_request_received`, `actor_request_received` | "%s is requesting %i%s", "%s is requesting %s" | `GENERIC_EVENT` | side, then amount (rounded) and resource glyph, or the thing [code] |
  | `building_invalid_placement` | "%s requires an unobstructed %s" | `GENERIC_ERROR` | |
  | `kingdom_defeated`, `team_defeated` | "%s has been defeated" | `KINGDOM_DEFEATED` | global |
  | `kingdom_resigned`, `team_resigned` | "%s has resigned" | `KINGDOM_DEFEATED` | global |
  | `kingdom_victory`, `team_victory` | "%s has won" | `QUEST_POSITIVE` | global |
  | `city_ratio_victory` | "%s controls %d cities" | `QUEST_POSITIVE` | global |
  | `chat_sound` | none | | only plays the chat sound; filter `UIEventFilterChatSound` |

  `hero_*`, `marker_discovered`, `lair_reward` and `teleport_failed_*`
  have texts too but nothing in Axis & Allies raises them, as far as the
  data goes; `territory_positive`/`_negative` belong to the campaign map.
  [code, data, guess]
- Stock filter values (`UVars.tgi`): `UIEventFilterSiege`,
  `UIEventFilterProduction` and `UIEventFilterChatSound` true;
  `UIEventFilterEngagement` false; `UIEventFilterEngagementMessages` isn't
  in the file and defaults to false. So out of the box a battle posts no
  engagement, rout or "being bombarded" lines for units. [code, data, game]

## Names in lines

Wherever a line names a unit, regiment or building (the `%s` of "%s
recruited", "%s: attaching %s", "%s captured by %s", the destroyed lines
and the rest), it uses that thing's own name, the one the selection panel
shows for it, worked out as follows. [code]

- A building that belongs to a city takes the city's name; a city centre
  gets its name from its template's `name_list_ids` (a list in
  `Namelists/`). [code, data]
- Otherwise a name the map gives the thing wins: scenario regiments and
  buildings named in the editor keep that name ("Rommel's Corps HQ").
  [code]
- Otherwise a thing whose template sets `numbered = true` and that belongs
  to a side gets a number before its template's `name`: "1st Infantry
  Division HQ", "3rd Conscript Regiment". `numbered` is an ordinary
  `Actor` field (`template_actor.tgi`, default false), inherited like any
  other through `Template=` / `Inherit=`. The stock data sets it in seven
  blocks of `template_organization_aa.tgi`: `AAOrganizationArmor`,
  `-Infantry`, `-Paratrooper`, `-Mobile`, `-Engineer`, `-Artillery` and
  `AAOrganizationHQ`. Each nation's regiment kinds (`AARUSOrganizationHQ`
  and the rest) build on those, and the definitions in `Regiments/` on
  the nation kinds, so every regiment is numbered, packed and unpacked HQs
  alike (`regiment_rus_hq_armor_unpacked` is an `AARUSOrganizationHQ`).
  What's named is the regiment, not the building or truck inside it.
  Companies, trucks, buildings and `AAOrganizationAmbient` regiments
  aren't numbered. To tell from the data, follow a definition's template
  chain to the nearest `numbered`. That file, like most of the data, is
  Latin-1 (its header's ©), so a search tool that expects UTF-8 may treat
  it as binary and skip it; `grep -a` finds it. [code, data, game]
- Anything else shows its template's `name` alone ("Supply Depot Truck").
  [code, data]
- Numbers are counted per side and per template: each side's first regiment
  of a template is 1, its next of the same template 2, and so on, however
  many of them still exist; a number is never handed out again within a
  match. Two templates with the same `name` count separately, so a packed
  and an unpacked Infantry Division HQ template can each have a "1st".
  Regiments on the map at the start are numbered in the order the map
  lists them, before anything is recruited. [code, data]
- A regiment keeps its number for life, through packing and unpacking,
  attaching and detaching. [code]
- The ordinal comes from the `actor_name_numbered*` strings in
  `strings_rtse.tgi`: "%dst %s" when the number ends in 1, "%dnd %s" in 2,
  "%drd %s" in 3, "%dth %s" otherwise, and always "th" when the number's
  last two digits are 11 to 20 ("11th", "12th", "111th"). [code, data]
- In a saved game each regiment keeps its number, but the count isn't
  saved: after loading, a side's next regiment of a template starts again
  at 1. [code]
- A side named in a line (the owner in "%s's %s destroyed", the taker in
  "%s captured by %s") is shown as the gem glyph (`ch_gem`, from
  `KingdomGlyphInfo` in `AVars_AA.tgi`) in the side's colour, a space, then
  the side's name in the line's own colour. The name is the side's, the
  same one the selection panel shows under a unit's name, never its
  general's: what a side is called is in [players](players.md#side-names).
  [code, data]
- An independent side (one of a map's `independent_N` sides) gets no gem,
  and no space: the line shows just its name. The selection panel leaves
  the chip out for the same sides, so the two always agree. [code]

## Destroyed and lost

- When a thing is destroyed, by any cause (shot down, out of supply,
  disbanded), two kinds of line can follow. Nothing about who did it
  matters to either. [code]
  - Its owner is told "%s destroyed": `static_actor_lost` for a building,
    `mobile_actor_lost` for anything else (a regiment, a truck). [code]
  - Every side that is the owner's enemy and can see the thing at that
    moment is told "%s's %s destroyed" (`actor_destroyed`), the owner and
    the thing. So the player here gets it for any enemy regiment or
    building they see die, whether they, an ally or a third side killed
    it; allies and neutrals of the owner are told nothing. [code]
- Only some things count: regiments, buildings, and units that stand on
  their own because they pack or unpack (trucks), put out units, or carry
  others. A company inside a regiment never gets a line of its own; its
  regiment does, when the regiment goes. Projectiles never do. [code]
- A thing that belongs to no side posts neither line. [code]
- A building that goes down with its city (its city centre already being
  destroyed) posts no line for itself. [code]
- Wall pieces post neither line. The first piece of a city's wall to fall
  while the rest still stands posts `wall_breached` ("Wall Breached") to
  the wall's owner instead; later pieces post nothing. [code]
- Things that leave the map without being destroyed post no destroyed or
  lost line: a building sold (it posts `building_sold`), a building
  replaced by its upgrade, a truck turning into its building or a building
  packing into its truck. [code]
- Disbanding a regiment destroys it, so its owner gets "%s destroyed" and
  enemies who can see it get "%s's %s destroyed". A disbanded lone unit
  (a supply or depot truck) just leaves the map and posts nothing. [code]
- `building_sold` ("%s sold for %s") takes the building's name, then the
  payment written as a cost. It goes to the owner when a building is sold,
  and also when a building whose template has `can_sell` true is
  destroyed (whether or not it could be sold at that moment), after its
  destroyed line ([buildings](buildings.md#disbanding-and-selling)).
  [code]

## Production lines

- When a building finishes putting something out, its owner is told what
  came out, by the thing's name (Names in lines): "%s commissioned"
  (`company_commissioned`) for a regiment, whatever recruits it (a Division
  HQ from the Corps HQ, infantry from a Division HQ), and "%s recruited"
  (`company_recruited`) for anything sent by transport, regiments
  included ([deliveries](deliveries.md#the-order)), and for a single unit
  that isn't a regiment, such as a
  supply or depot truck. [code, game]
- The line is about the new thing, so its location is where that came out.
  It comes before any line the new regiment then raises, such as "%s:
  attaching %s" when it joins an HQ. [code]
- If nothing could be put out (no room), no line is posted. [code]

## Other messages

Some lines come straight from the game rather than from an event. Their
texts are keys in the `Localization/` string tables ([data
files](data.md), String tables, says how keys resolve); the line shows the key's text with its placeholders filled.

- Screenshots: "Screenshot taken: %s" (`screenshot_taken`), the full path
  of the saved picture, or "Screenshot failed" (`screenshot_failed`), both
  `GENERIC_EVENT`, shown to the player who took it. [code, data, game]
- Saving: "Game Saved: '%s'" (`game_saved`, `GENERIC_EVENT`) and "Error
  saving '%s': %s" (`save_error`, `GENERIC_ERROR`); in a multiplayer game
  "The host is saving the game." (`networking_game_save_begin`). [code,
  data]
- Orders that fail, all `GENERIC_ERROR` and shown only to the side that
  gave the order, with the unit or building concerned as the line's
  location where there is one. They include "Insufficient funds for %s.
  Requires %s." (`recruit_error_affording`, `construct_error_affording`,
  `research_error_affording`, `denizen_error_affording`), "Insufficient
  unit limit to recruit %s" (`recruit_error_unit_limit`), "Could not
  create %s", "There are enemies nearby", the `hq_error_*` attach refusals
  ("Cannot attach to %s. HQ is full."), `tai_error_pack`/`_unpack` ("Cannot
  pack %s"), the `mobile_deployment_error_*` packing refusals,
  `structure_error_queue_*` ("%s paused production queue - no supply"),
  `settlement_error_*` and `settlement_auto_placement_failed`,
  `transport_error_unloading`, `element_error_detach`,
  `organization_error_*`, `kill_error_invalid_target`,
  `bombard_error_location` ("Invalid bombard target", at the clicked
  place), `ui_error_command_target` ("Invalid target for %s", no
  location), `ui_error_command_target_actor` ("%s is an invalid target
  for %s", at the clicked thing; when each is posted and what fills it is
  in [commands](commands.md), Targeted commands), the
  `ui_error_tribute_*` and `ui_error_team_command_target_invalid` refusals,
  the special-operation refusals (`ui_error_faction_ability_*`),
  `ability_effect_invalid_location` and `ability_effect_summon_failed`.
  [code, data]
- Multiplayer, all `GENERIC_EVENT` to every player unless noted: players
  joining ("%s has joined the game", `PLAYER_JOINLEAVE`), leaving and
  being kicked (below), the vote to kick, a new host, game speed changes ("Game speed
  changed to %.0f%%"), pausing and the vote to unpause (every line and
  when it's posted: [multiplayer](multiplayer.md), Pause, speed, chat,
  votes), renaming, the vote
  to resign ("%s voted for %s to resign"), an AI taking over a player
  (`sai_midgame_name_change`), the end-of-game notices
  (`ui_mission_message_*`, `ui_mission_no_human_opponents_remaining*`,
  `gamespy_stats_submit*`) and map transfer problems (`GENERIC_WARNING`).
  [code, data]
- A player leaving a session, in the lobby or the match, posts one line to
  everyone still in it, as `GENERIC_EVENT`, not `PLAYER_JOINLEAVE`, so the
  Room filter doesn't hide it: [code, data]
  - "Player %s quit the game" (`networking_player_quit`) for every
    departure that isn't a kick or a bad CD key: choosing to leave, and
    being dropped after holding the match, whether lost, slow or out of
    sync (see [multiplayer](multiplayer.md)). The game doesn't tell these
    apart in the line.
  - "Player %s was kicked from the game" (`networking_player_kicked`)
    after a kick, by the host in the lobby or by vote in the match.
  - "Player %s had an invalid or duplicate CD key"
    (`networking_player_invalid_cdkey`) when the online service turns
    their key down.
  - The kicked player sees "You were kicked from the game"
    (`networking_player_was_kicked`, `GENERIC_EVENT`) instead, and no
    line for the other players, who drop out of its view (see
    [multiplayer](multiplayer.md)).
- In a match the leaver's line comes first, then a line for each
  computer player taking over a side ("%s taken over by %s",
  `sai_midgame_name_change`, `GENERIC_EVENT`): see Taking over a side in
  [multiplayer](multiplayer.md). [code]
- The %s in those lines and in "%s has joined the game" is the person's
  name, not their side's, shown like a side in a line (above): a glyph, a
  space, then the name in the line's own colour. In a match the glyph is
  the gem (`KingdomGlyphInfo`) in their side's colour, or for a player
  without a side the observer glyph (`ObserverGlyphInfo`, `ch_observer`)
  in white; in the lobby it's always white. [code, data] In an internet
  game the online service can supply a different glyph for a player.
  [code, guess]
- `PLAYER_JOINLEAVE` carries "%s has joined the game" and the online
  lobby's chat-room lines: "%s joined", "%s left", "%s was kicked"
  (`ui_lobby_player_*_format`). [code, data]
- When the host leaves a match that is over (End Mission), every other
  machine is told and posts "The host has ended the game."
  (`ui_mission_message_host_has_ended_game`, `GENERIC_EVENT`); the host
  itself doesn't. A host who leaves before the match is over sends no
  such notice: the others see only the quit line. [code]
- `ability_effect_describe_economy` ("%s Economy: %s"), from a special
  operation that reveals an enemy's economy, is `GENERIC_EVENT`. [code,
  data]
- Scenario triggers: the `kingdom_display_message` effect ("Side sent
  Message") posts its text as `GENERIC_EVENT` to that side's player only;
  given no side, to everyone. A trigger whose effects fail posts "Trigger
  %s could not process effects" (`trigger_error_fail_evaluate`,
  `GENERIC_WARNING`). [code, data]
- Engine warnings and errors can be echoed into the log as
  `GENERIC_WARNING` / `GENERIC_ERROR` lines when `WarningsDisplayedAtEnd`
  is true and `WarningsDisplayedImmediately` false. `UVars_AA.tgi` sets
  `WarningsDisplayedAtEnd` false, so the shipped game doesn't. [code, data]

## Chat

- Chat typed in a match goes through the log. The line is "%s: %s"
  (`public_chat`, `team_chat` or `private_chat`): the sender's name, then
  what they said. The name is coloured `UIChatPlayerColorNormal` (white),
  or `UIChatPlayerColorBuddy` (green) for a buddy; the words are coloured
  by channel: `ChatPublicColor` (yellow) to all, `ChatTeamColor` (green) to
  allies, `ChatWhisperColor` (aqua) to one player (`UVars.tgi`). [code,
  data, guess]
- Chat from your own side or an ally is posted as `GENERIC_EVENT` with a
  location: the point the sender's camera was looking at when they sent
  it, which travels with the message. Chat from anyone else (an enemy, an observer) as `CHAT_PUBLIC`
  or `CHAT_PRIVATE` (`LAN_CHAT_*` on a LAN), so the Chat filter hides only
  the latter. Each chat line also raises `chat_sound`. [code]
- The chat sound: every chat message that reaches the player in a match,
  to all, to the team or to them alone, plays `chat_sfx`
  (`Audio/sounds_game_events_aa.tgi`, the file
  `Sounds/Interface/GenericError.wav`) through the `chat_sound` game event
  (`Game/game_events.tgi`). It has no line of its own; the chat line is
  posted separately. [code, data]
- Chat seen in the lobby (staging screen) makes no sound: the sound needs
  a match under way. [code] The player's own messages come back like
  anyone else's and so sound too. [guess]
- `chat_sfx` is an `AudioFeedback` sound, as [game events](game-events.md)
  describes: flat, `volume = 90%`, pitch varied by up to 1%, one at a
  time. Its category is `SOUND`, so it's heard at Main volume × FX volume.
  [code, data]
- The options' Chat Notification switch is `UIEventFilterChatSound`, the
  event's `filter`. Off, the sound goes and the chat line still shows.
  The other way round, the Chat message filter (`UIMessageFilterChat`, on
  the chat page) hides enemy and observer chat lines but leaves the sound.
  The game play page's Notifications switches don't touch it. [code, data]
- The event's other conditions apply: no sound while a saved game is
  loading or the match is still starting up. [code]
- No chat sound either while the match is frozen (while it saves, and
  while it waits for every machine to finish loading); the line still
  posts. Pausing doesn't stop it: chat while paused sounds as usual.
  [code]
- Watching a film replays its chat as it was recorded, so the lines and
  the sound come again at the same moments. [code, guess]
- Errors while sending ("The person you were attempting to chat with was
  not found", `private_chat_warning_*`) are `GENERIC_WARNING`. [code, data]
- A whisper's sender doesn't get the "%s: %s" line: their own machine
  posts "Whispering to %s: %s" (`ui_lobby_whispering_to`) at once, as
  `CHAT_PRIVATE`. [code, data]
- Chat typed in the connection dialog posts "%s: %s" (`public_chat`) as
  `GENERIC_EVENT` on every machine, without the channel colours or the
  chat sound. Where each kind of chat reaches, and the chat bar, are in
  [multiplayer](multiplayer.md). [code]

## Who sees a line

- Each player has their own log; nothing is shared. A line shows on a
  player's screen only when the thing that posted it was meant for them:
  their own events and errors, global events (see [game
  events](game-events.md)), and the multiplayer notices above. [code]
- Lines are never merged or counted: the same event twice posts two lines.
  [code]

## The log on screen

- The log sits at the top left, just right of the left-hand button column
  and below the top bar: its box starts 57 pixels from the left and 45 from
  the top, is 564 wide, and may reach down to 70% of the screen's height
  (`UI/Game/message_log.tgi`, `GameMessageLog`, in the 1024 by 768 layout).
  [data, game]
- It shows at most `UIMessageLogMaximumLines` lines (`UVars.tgi`, stock 5;
  the options allow 1 to 15). [code, data]
- Lines run oldest at the top, newest at the bottom. The box is anchored at
  its top and grows downward as lines arrive (`expands_down = true`). A new
  line goes at the bottom; when the log is already full, the oldest line
  goes to make room, so the rest move up one. [code, data, game]
- Each line stays fully visible for `UIMessageLogFadeTime` milliseconds
  after it appears (`UVars.tgi`, stock 10000; options 1 to 20000), then
  fades out evenly over one second, icon and text together, and is gone.
  The lines below it move up to close the gap. [code, data]
- Changing the number of lines in the options takes effect at once,
  dropping the oldest lines or making room for more. [code]
- A long line wraps at the box's width; the wrapped part starts under the
  first line's text, not under the icon, and the next message starts below
  the whole wrapped block. [data, game]

## What a line looks like

- Each line is an icon and its text. The icon is drawn 16 by 17 pixels, 10
  pixels in from the box's left edge and 2 down from the line's top; the
  text starts 24 pixels in, so about 8 pixels right of the icon. [data,
  game]
- The text uses the `small` font (`Fonts/font_small.tgi`): `A&A.ttf` at
  size 12, bold, with a drop shadow at 40% opacity (Arial 11 when that font
  is missing). It's left-justified, in the kind's colour (white). There is
  no box or panel behind the text. [data, game]
- Lines are about 20 pixels apart at 1024 by 768, wrapped lines too. [game]

## Clicking and keys

- The icon is a button; the text isn't. The icon's tooltip, and what
  clicking it does, depend on the line's kind: [code, data]
  - `QUEST_POSITIVE`, `QUEST_NEGATIVE`, `QUEST_MESSAGE`: "Open objectives
    panel" (`ui_tooltip_message_quest`), opening the objectives panel.
  - `BUDDY_MESSAGE`: "Open buddy list"; `IM_SERVER`: "Respond to buddy
    request".
  - Any other line with a location, in a match: "Move camera to message
    location" (`ui_tooltip_message_location`); clicking centres the camera
    there.
  - `CHAT_PUBLIC` and `CHAT_PRIVATE`, under a condition of the session
    (probably online play): "Open player options"
    (`ui_tooltip_message_chat`).
  - Anything else has no tooltip and does nothing.
- Pressing the icon plays `push_button`, like any push button
  ([sounds](sounds.md)). [data]
- Clicking a line's icon also makes the next Space press start again
  from the newest message. [code]
- The camera goes to where the unit or building the line is about is now,
  as the player sees it, if it still exists; otherwise to where it was
  when the line was posted. [code]
- Space (`CycleMessageLog` in `hotkeys_ui.txt`) steps the camera back
  through the messages, one message per press, starting from the newest:
  [code, data]
  - A press on a message with a location moves the camera there. A press
    on one without a location does nothing, and the next press goes on to
    the one before it: lines without a location aren't skipped, they cost
    a press each.
  - The steps go by message number, counting every message posted since
    the log was last cleared, which happens when a player hosts or joins
    a game and when a match ends (filtered-out kinds aren't numbered).
    Once the history has dropped older messages, presses past the oldest
    kept one do nothing until the count reaches the first message, and
    only then does it wrap round to the newest. So after a long match,
    stepping past the 50 kept messages means many dead presses. [code]
  - The next press starts again from the newest only after a pause longer
    than `UIMessageLogCycleTimeout` seconds (stock 1.0) since the last
    press, or after a line's icon was clicked. A new line arriving doesn't
    restart it: within the timeout the next press carries on downward,
    and the new line is reached only after wrapping round. [code]

## History

- The game keeps the last `MessageLogMaximumLines` messages (`UVars.tgi`,
  stock 50), dropping the oldest past that, whether or not they've faded
  from the screen. [code, data]
- The control panel's message log button ("Display Message Log",
  "Displays a log of all recent messages.") opens the side panel's Message
  Log page: the kept messages in the same icon-and-text form, oldest at the
  top, newest at the bottom, with a scrollbar, kept scrolled to the newest.
  They don't fade there. Its icons work as above. [code, data]
- That page is a window like the Technology Panel
  ([object display](object-display.md#technology-display)): 542 by 514 at 279, 53
  on the 1024 by 768 layout (`SideMessageLogPanel` in
  `UI/Game/game_interface.tgi`). F3 presses the button
  (`hotkeys_custom.txt`); pressing it again closes the page, as do its
  close button, its "Close" button, F3 or Esc while it has the keyboard,
  and Backspace. [code, data]
- Opening it in a single-player game pauses the game unless it's
  already paused, and closing it unpauses only if opening it paused it;
  in a multiplayer game, hosted or joined, nothing pauses (see
  [interface](interface.md#windows-over-the-match)). Opening and closing it make no
  sound beyond the button's. [code]
- That page is the only record a player sees. There's no separate chat or
  event history, and nothing is written to a file in normal play; with
  the game's logging turned up (`log 99` in `startup/autoexec.txt`) every
  line also goes to its log file. [code, guess] A recorded game holds
  the orders that made a player leave, so watching it should post the
  same lines again. [guess]

## Options

The options screen's game play page holds the log's settings, saved like
any other setting:

- Message Log Fade: a slider from "Fast" to "Slow" setting
  `UIMessageLogFadeTime` (1 to 20000 ms). There's no switch to stop lines
  fading. [code, data]
- Maximum Message Lines: a slider setting `UIMessageLogMaximumLines` (1 to
  15). [code, data]
- Notifications: "Engagement" (`UIEventFilterEngagement`), "Audio Only"
  (on means `UIEventFilterEngagementMessages` false: engagement sounds
  without lines), "Settlement Under Siege" (`UIEventFilterSiege`) and
  "Production" (`UIEventFilterProduction`). [code, data]
- The chat page: "Chat" (`UIMessageFilterChat`), "Buddy"
  (`UIMessageFilterBuddy`), "Away" (`UIMessageFilterAFK`) and "Room"
  (`UIMessageFilterPlayerJoinLeave`). The other kind filters aren't on any
  page. [code, data]
- Nothing turns the log off altogether. [data]

## Editor

- The scenario editor has its own log (`UI/Editor/message_log.tgi`,
  `EditorMessageLog`): along the bottom of the screen from 280 pixels in to
  180 from the right, growing upward. It carries the editor's own lines,
  such as "Map preparation complete" (`prepare_ok`), "Map verified"
  (`verify_ok`), "City %s: attaching %s" (`prepare_structure_attached_to_city`,
  `BUILDING_GENERIC`, with the city as location), "Map Saved: '%s'", "%s
  attached to %s" (`editor_hero_attached`), and placement refusals such as
  `settlement_error_*` and `required_object_error` (`GENERIC_ERROR`).
  [code, data]
- It works like the match's log: at most `UIMessageLogMaximumLines`
  lines, each fading after `UIMessageLogFadeTime` as above. [code]

## Open questions

- At 800 by 600 and 1280 by 960, does the log keep its 1024 by 768 pixel
  position, width and font size, or scale with the screen? The data
  only swaps some textures per resolution (`UI/800/`, `UI/1280/`), not
  layouts. (analysis, 2026-11-11)
- After loading a saved game, do new regiments really start again at "1st"
  (so two "1st Infantry Regiment"s can exist)? Check in the game.
  (analysis, 2026-11-11)
