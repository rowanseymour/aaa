# Data

What this project adds to the game's data: what its own interface draws
with, such as its fonts, and stand-ins for what the game's data names but
doesn't hold. They're laid out and written as the game's are, so they're read
the same way, after the game's own: a path or id the game has is the game's.
Name new ones so they can't clash, e.g. starting `modern_`.

`common/` is for every game, and `<tag>/` for one game by its tag, e.g. `aa`.

Images can be SVGs, drawn without text, so letters have to be paths. Icons are
drawn as big as they're shown, so they stay sharp at any scale; any other SVG is
drawn at the size it gives itself, so give it about three times the texels it's
shown at.

The fonts are under the SIL Open Font License, given beside each, e.g.
[`common/Fonts/TrueType/OFL.txt`](common/Fonts/TrueType/OFL.txt); the rest is
under this project's [licence](../LICENSE).
