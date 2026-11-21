//! Reading a line into commands and words, and completing them.

use bevy::prelude::*;

use crate::{Raising, SelectedBuilding, Sim};

use super::Console;
use super::verbs::VERBS;

/// A line's commands, each split into words.
pub(crate) fn commands(line: &str) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut words = Vec::new();
    let mut word = String::new();
    // A word was started, so `""` still counts as one.
    let mut started = false;
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        let comment = c == '#' || (c == '/' && chars.peek() == Some(&'/'));
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            _ if quoted => word.push(c),
            _ if comment && !started => break,
            ';' => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
                if !words.is_empty() {
                    out.push(std::mem::take(&mut words));
                }
            }
            c if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            c => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        words.push(word);
    }
    if !words.is_empty() {
        out.push(words);
    }
    out
}

/// What tab offers after `before` on the line: verbs first, then the
/// templates `create` can raise or the kingdoms.
fn completions(world: &mut World, before: &str) -> Vec<String> {
    // A line's last command is the one being typed.
    let last = before.rsplit(';').next().unwrap_or("");
    let words = commands(last).pop().unwrap_or_default();
    let in_match = world.contains_resource::<Sim>();
    match words.as_slice() {
        [] => VERBS.iter().map(|v| v.name.to_string()).collect(),
        [verb] if verb.eq_ignore_ascii_case("help") => {
            VERBS.iter().map(|v| v.name.to_string()).collect()
        }
        [verb] if verb.eq_ignore_ascii_case("create") && in_match => world
            .resource::<Raising>()
            .0
            .templates()
            .iter()
            .map(|t| t.to_string())
            .collect(),
        [verb] if verb.eq_ignore_ascii_case("commission") && in_match => {
            let sim = &world.resource::<Sim>().0;
            let building = world.resource::<SelectedBuilding>().0;
            let template = building.map_or("", |b| sim.buildings[b].template.as_str());
            let raising = &world.resource::<Raising>().0;
            raising
                .list(template)
                .iter()
                .map(|t| t.to_string())
                .collect()
        }
        [verb] if verb.eq_ignore_ascii_case("research") && in_match => {
            let sim = &world.resource::<Sim>().0;
            let building = world.resource::<SelectedBuilding>().0;
            (building.map_or(&[][..], |b| sim.research_list(b)).iter())
                .map(|r| r.technology.to_string())
                .collect()
        }
        [verb] if verb.to_ascii_lowercase().ends_with("kingdom") && in_match => {
            let players = &world.resource::<Sim>().0.sides.players;
            players.iter().map(|p| quote(&p.name)).collect()
        }
        _ => Vec::new(),
    }
}

pub(crate) fn quote(word: &str) -> String {
    match word.contains(char::is_whitespace) {
        true => format!("\"{word}\""),
        false => word.to_string(),
    }
}

/// Completes the last word as far as the choices agree, or lists them.
pub(crate) fn complete(world: &mut World) {
    let input = world.resource::<Console>().input.clone();
    let (before, partial) = match input.rfind(|c: char| c.is_whitespace() || c == ';') {
        Some(i) => input.split_at(i + 1),
        None => ("", input.as_str()),
    };
    let (before, partial) = (before.to_string(), partial.to_string());
    let lower = partial.trim_start_matches('"').to_ascii_lowercase();
    let choices: Vec<String> = completions(world, &before)
        .into_iter()
        .filter(|c| {
            c.trim_start_matches('"')
                .to_ascii_lowercase()
                .starts_with(&lower)
        })
        .collect();
    let mut console = world.resource_mut::<Console>();
    match choices.as_slice() {
        [] => {}
        [one] => console.input = format!("{before}{one} "),
        many => {
            let mut lead = many[0].clone();
            for c in &many[1..] {
                let n = lead
                    .chars()
                    .zip(c.chars())
                    .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
                    .count();
                lead = lead.chars().take(n).collect();
            }
            if lead.len() > partial.len() {
                console.input = format!("{before}{lead}");
            } else {
                console.say(&many.join("  "));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<Vec<&str>> {
        let c = commands(line);
        // Leaked for the comparison's sake.
        c.into_iter()
            .map(|w| w.into_iter().map(|s| &*s.leak()).collect())
            .collect()
    }

    #[test]
    fn splits_commands_and_words() {
        assert_eq!(words("lookat 10 20"), [["lookat", "10", "20"]]);
        assert_eq!(
            words("  +left;  wireframe ;lookat 1 2;"),
            [vec!["+left"], vec!["wireframe"], vec!["lookat", "1", "2"]]
        );
        assert_eq!(
            words("placementkingdom \"Free French\"; echo \"a;b\" \"\""),
            [
                vec!["placementkingdom", "Free French"],
                vec!["echo", "a;b", ""]
            ]
        );
    }

    #[test]
    fn drops_comments() {
        assert!(words("# lookat 1 2").is_empty());
        assert!(words("   // lookat 1 2").is_empty());
        assert!(words("#changeres 640 480 1").is_empty());
        assert_eq!(words("zoom 1; // the rest"), [["zoom", "1"]]);
        assert_eq!(words("echo a#b"), [["echo", "a#b"]]);
    }
}
