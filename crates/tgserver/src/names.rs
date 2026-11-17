//! What the people in a match go by.

/// The most characters a person's name keeps.
pub const NAME_LONGEST: usize = 18;

/// How a name that's taken is numbered, unless the game's data says
/// otherwise: the name, then the number.
pub const NUMBERED: &str = "%s (%d)";

/// What a person goes by who's never said, as the game's data has it.
pub fn default_name(strings: &tgdata::Strings) -> String {
    let name = strings.get("default_lan_name").and_then(|n| name(n));
    name.unwrap_or_else(|| "Player".into())
}

/// How the game's data numbers a name that's taken, as [`unique`] takes
/// it.
pub fn numbered(strings: &tgdata::Strings) -> String {
    let numbered = strings.get("networking_duplicate_name_formate|sd");
    numbered.map_or(NUMBERED, String::as_str).to_string()
}

/// The name `typed` makes: its first [`NAME_LONGEST`] characters, without
/// the spaces either end. `None` if that leaves nothing, or it's not all
/// what can be typed.
pub fn name(typed: &str) -> Option<String> {
    let cut: String = typed.chars().take(NAME_LONGEST).collect();
    let name = cut.trim_matches(' ');
    let fit = !name.is_empty() && !name.chars().any(char::is_control);
    fit.then(|| name.to_string())
}

/// `name`, or if `taken` says it is, the first of it numbered from 1 by
/// `numbered` (a `%s` for the name and a `%d` for the number) that isn't.
/// One with no `%d` numbers nothing, so [`NUMBERED`] does instead.
pub fn unique(name: &str, numbered: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(name) {
        return name.to_string();
    }
    let numbered = match numbered.contains("%d") {
        true => numbered,
        false => NUMBERED,
    };
    (1..)
        .map(|n| fill(numbered, name, n))
        .find(|numbered| !taken(numbered))
        .expect("some number is free")
}

/// `numbered` with its first `%s` given `name` and its first `%d` `n`, in
/// one pass, so that what `name` holds is never taken for either.
fn fill(numbered: &str, name: &str, n: u32) -> String {
    let (mut filled, mut rest) = (String::new(), numbered);
    let (mut named, mut counted) = (false, false);
    while let Some(at) = rest.find('%') {
        filled.push_str(&rest[..at]);
        let tail = &rest[at..];
        if !named && tail.starts_with("%s") {
            filled.push_str(name);
            named = true;
        } else if !counted && tail.starts_with("%d") {
            filled.push_str(&n.to_string());
            counted = true;
        } else {
            filled.push('%');
            rest = &tail[1..];
            continue;
        }
        rest = &tail[2..];
    }
    filled.push_str(rest);
    filled
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_cut_short_then_trimmed() {
        assert_eq!(name("Bob").as_deref(), Some("Bob"));
        assert_eq!(name("  Bob  ").as_deref(), Some("Bob"));
        // Cut before trimmed, so the spaces cut to don't count.
        let long = "Ann                 Bob";
        assert_eq!(name(long).as_deref(), Some("Ann"));
        assert_eq!(
            name("abcdefghijklmnopqrstuvwxyz").as_deref(),
            Some("abcdefghijklmnopqr")
        );
        assert_eq!(name("Ünïcødé ").as_deref(), Some("Ünïcødé"));
        assert_eq!(name(""), None);
        assert_eq!(name("   "), None);
        assert_eq!(name("Bo\nb"), None);
    }

    #[test]
    fn a_name_taken_is_numbered_until_it_is_not() {
        let taken = |names: &'static [&'static str]| move |n: &str| names.contains(&n);
        assert_eq!(unique("Bob", NUMBERED, taken(&["Ann"])), "Bob");
        assert_eq!(unique("Bob", NUMBERED, taken(&["Bob"])), "Bob (1)");
        // Always the name as given, numbered.
        assert_eq!(
            unique("Bob", NUMBERED, taken(&["Bob", "Bob (1)"])),
            "Bob (2)"
        );
        // Exactly the name.
        assert_eq!(unique("bob", NUMBERED, taken(&["Bob"])), "bob");
        assert_eq!(unique("Bob", "%s#%d", taken(&["Bob"])), "Bob#1");
        // What the name holds is the name's.
        assert_eq!(unique("%d", NUMBERED, taken(&["%d"])), "%d (1)");
        assert_eq!(unique("%s", NUMBERED, taken(&["%s"])), "%s (1)");
        // A number first, or none at all.
        assert_eq!(unique("Bob", "%d: %s", taken(&["Bob"])), "1: Bob");
        assert_eq!(unique("Bob", "%s again", taken(&["Bob"])), "Bob (1)");
    }
}
