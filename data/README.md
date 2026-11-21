# Data

This project's own data files, for what its own interface draws with, such
as its fonts. They're laid out and written as the game's are, so they're read
the same way, after the game's own: a path or id the game has is the game's.
Name new ones so they can't clash, e.g. starting `modern_`.

`modern/` is for every game, and `modern_<tag>/` for one game by its tag,
e.g. `modern_aa`.

Images can be SVGs, drawn without text, so letters have to be paths. An SVG is
drawn at the size it gives itself: give an icon about three times the texels
it's drawn at, so that it stays sharp when the interface is scaled up.

The fonts are under the [SIL Open Font License](modern/Fonts/TrueType/OFL.txt);
the rest is under this project's [licence](../LICENSE).
