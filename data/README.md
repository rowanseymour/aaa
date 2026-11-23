# Data

This project's own data files, for what its own interface draws with, such
as its fonts. They're laid out and written as the game's are, so they're read
the same way, after the game's own: a path or id the game has is the game's.
Name new ones so they can't clash, e.g. starting `modern_`.

`modern/` is for every game, and `modern_<tag>/` for one game by its tag,
e.g. `modern_aa`.

Images can be SVGs, drawn without text, so letters have to be paths. Icons are
drawn as big as they're shown, so they stay sharp at any scale; any other SVG is
drawn at the size it gives itself, so give it about three times the texels it's
shown at.

The fonts are under the SIL Open Font License, given beside each, e.g.
[`modern/Fonts/TrueType/OFL.txt`](modern/Fonts/TrueType/OFL.txt); the rest is
under this project's [licence](../LICENSE).
