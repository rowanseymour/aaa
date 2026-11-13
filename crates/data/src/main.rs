use std::collections::BTreeMap;
use std::io::Write;
use std::process::ExitCode;

use aaa_data::GameData;
use tgi::de::Count;

const USAGE: &str = "\
usage:
  aaa-data coverage <game dir>          how much of each top-level block kind the definitions read
  aaa-data coverage <game dir> <kind>   each key of one kind, e.g. Thing, with how often it is read
  aaa-data audio    <game dir> [filter] sounds, speech and music by id, with their files and any missing
  aaa-data cues     <game dir> [filter] the audio object templates play, by what sets it off
  aaa-data effects  <game dir> [filter] the visual effects object templates show, by what sets them off
  aaa-data messages <game dir>          the kinds of message, then the game events and the lines they post
  aaa-data generals <game dir>          each nation's generals and their special operations as tooltips show them";

/// Prints the library's warnings, such as data blocks that don't read.
struct Stderr;

impl log::Log for Stderr {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!(
                "aaa-data: {}: {}",
                record.level().as_str().to_lowercase(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

fn main() -> ExitCode {
    if log::set_logger(&Stderr).is_ok() {
        log::set_max_level(log::LevelFilter::Warn);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["coverage", dir] => coverage(dir, None),
        ["coverage", dir, kind] => coverage(dir, Some(kind)),
        ["audio", dir] => audio(dir, ""),
        ["audio", dir, filter] => audio(dir, filter),
        ["cues", dir] => cues(dir, ""),
        ["cues", dir, filter] => cues(dir, filter),
        ["effects", dir] => effects(dir, ""),
        ["effects", dir, filter] => effects(dir, filter),
        ["messages", dir] => messages(dir),
        ["generals", dir] => generals(dir),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e)
            if e.downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::BrokenPipe) =>
        {
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("aaa-data: {e}");
            ExitCode::FAILURE
        }
    }
}

type Res = Result<(), Box<dyn std::error::Error>>;

/// `println!` that returns an error instead of panicking when stdout is closed
/// (e.g. piped into `head`).
macro_rules! outln {
    ($($arg:tt)*) => {
        writeln!(std::io::stdout(), $($arg)*)?
    };
}

fn coverage(dir: &str, kind: Option<&str>) -> Res {
    let coverage = GameData::open(dir.as_ref())?.coverage();
    match kind {
        None => {
            let mut kinds: BTreeMap<&str, Count> = BTreeMap::new();
            let mut total = Count::default();
            for (path, _, count) in coverage.keys() {
                let kind = path.split('/').next().unwrap_or(path);
                add(kinds.entry(kind).or_default(), count);
                add(&mut total, count);
            }
            let mut kinds: Vec<_> = kinds.into_iter().collect();
            kinds.sort_by_key(|(k, c)| (std::cmp::Reverse(c.read + c.unread), *k));
            outln!("{:>8} {:>8} {:>5}  kind", "read", "fields", "");
            for (kind, c) in kinds {
                outln!("{}  {kind}", row(c));
            }
            outln!("{}  (all)", row(total));
        }
        Some(kind) => {
            let kind = kind.to_ascii_lowercase();
            outln!("{:>8} {:>8}  key", "read", "unread");
            for (path, key, c) in coverage.keys() {
                if path.split('/').next() == Some(kind.as_str()) {
                    outln!("{:>8} {:>8}  {path}/{key}", c.read, c.unread);
                }
            }
        }
    }
    Ok(())
}

/// Each audio whose id contains `filter`, ignoring case.
fn audio(dir: &str, filter: &str) -> Res {
    let filter = filter.to_ascii_lowercase();
    let all = GameData::open(dir.as_ref())?.audio();
    let mut audio: Vec<_> = (all.iter())
        .filter(|(id, _)| id.as_str().to_ascii_lowercase().contains(&filter))
        .collect();
    audio.sort_by(|a, b| a.0.cmp(b.0));
    let mut missing = 0;
    for (id, a) in &audio {
        let files = [&a.leadin, &a.files, &a.fallout].map(|f| f.len());
        outln!(
            "{id}  {:?}  {} files",
            a.category,
            files.iter().sum::<usize>()
        );
        for f in a.leadin.iter().chain(&a.files).chain(&a.fallout) {
            outln!("    {f}");
        }
        for f in &a.missing {
            outln!("    missing: {f}");
        }
        missing += a.missing.len();
    }
    outln!("{} audio, {missing} missing files", audio.len());
    Ok(())
}

/// The audio of each object template whose id contains `filter`, ignoring
/// case, with a `?` on audio that isn't defined.
fn cues(dir: &str, filter: &str) -> Res {
    let filter = filter.to_ascii_lowercase();
    let game = GameData::open(dir.as_ref())?;
    let audio = game.audio();
    let fighters = game.fighters();
    let mut objects: Vec<_> = game
        .object_audio()
        .into_iter()
        .filter(|(id, _)| id.as_str().to_ascii_lowercase().contains(&filter))
        .collect();
    objects.sort_by(|a, b| a.0.cmp(&b.0));
    let mut undefined = BTreeMap::new();
    let mut line = |event: &str, id: &aaa_data::Id| -> std::io::Result<()> {
        let mark = match audio.contains_key(id) {
            true => "",
            false => {
                *undefined.entry(id.to_string()).or_insert(0) += 1;
                " ?"
            }
        };
        writeln!(std::io::stdout(), "    {event}: {id}{mark}")
    };
    for (id, a) in &objects {
        outln!("{id}");
        let events = [
            ("selected", &a.selected),
            ("commanded", &a.commanded),
            ("commanded aggressive", &a.commanded_aggressive),
            ("created", &a.created),
            ("destroyed", &a.destroyed),
            ("start moving", &a.start_moving),
            ("stop moving", &a.stop_moving),
            ("disturbed", &a.disturbed),
        ];
        for (event, cues) in events {
            for c in cues {
                line(event, &c.audio)?;
            }
        }
        for (state, c) in &a.entered {
            line(&format!("entered {state}"), &c.audio)?;
        }
        for (state, c) in &a.left {
            line(&format!("left {state}"), &c.audio)?;
        }
        for anim in &a.animations {
            line(&format!("animation {}", anim.kind), &anim.audio)?;
        }
        for (i, w) in fighters.get(id).iter().flat_map(|f| &f.weapons).enumerate() {
            for c in &w.sounds {
                line(&format!("weapon {i}"), &c.audio)?;
            }
        }
    }
    outln!("{} templates with audio", objects.len());
    for (id, n) in undefined {
        outln!("undefined: {id} ({n})");
    }
    Ok(())
}

/// The effects of each object template whose id contains `filter`, ignoring
/// case, with a `?` on effects that aren't defined and each effect's kind.
fn generals(dir: &str) -> Res {
    let game = GameData::open(dir.as_ref())?;
    let rules = game.experience_rules();
    outln!(
        "starting {}, +{} a second",
        rules.starting,
        rules.per_second
    );
    let operations = game.special_operations();
    for nation in game.nations() {
        for g in &nation.generals {
            let max = g
                .experience_max
                .map_or("none".to_string(), |m| m.to_string());
            outln!("{} {} (max {max}, public {})", nation.id, g.name, g.public);
            for o in &g.operations {
                let Some(op) = operations.get(&o.id) else {
                    outln!("  {} (cost {}): no definition", o.id, o.cost);
                    continue;
                };
                let area = op.area.as_deref().unwrap_or("-");
                let icon = op.icon.as_deref().unwrap_or("-");
                outln!(
                    "  {} [{}] cost {}, recharge {}, area {area}, {icon}",
                    op.name,
                    o.id,
                    o.cost,
                    op.recharge
                );
                for l in &op.effects {
                    outln!("    {}{}", if l.indented { "  " } else { "" }, l.text);
                }
                outln!("    {}", op.description.as_deref().unwrap_or("-"));
            }
        }
    }
    Ok(())
}

fn messages(dir: &str) -> Res {
    let game = GameData::open(dir.as_ref())?;
    for k in game.message_kinds() {
        let icon = k.icon.as_deref().unwrap_or("-");
        let filter = k.filter.as_deref().unwrap_or("-");
        outln!("{}\t{icon}\t{:?}\t{filter}", k.id, k.colour);
    }
    outln!();
    let mut numbered: Vec<_> = game.numbered().into_iter().collect();
    numbered.sort();
    outln!("numbered: {}", numbered.len());
    for id in numbered {
        outln!("  {id}");
    }
    outln!();
    let mut events: Vec<_> = game.game_events().into_values().collect();
    events.sort_by(|a, b| a.id.cmp(&b.id));
    for e in events {
        let kind = e.kind.as_ref().map_or("-", |k| k.as_str());
        let filters = [&e.filter, &e.message_filter].map(|f| f.as_deref().unwrap_or("-"));
        let line = e.message.as_ref().map_or("-".into(), |m| format!("{m:?}"));
        outln!("{}\t{kind}\t{}\t{}\t{line}", e.id, filters[0], filters[1]);
    }
    Ok(())
}

fn effects(dir: &str, filter: &str) -> Res {
    let filter = filter.to_ascii_lowercase();
    let game = GameData::open(dir.as_ref())?;
    let vfx = game.vfx();
    let fighters = game.fighters();
    let mut objects: Vec<_> = game
        .object_effects()
        .into_iter()
        .filter(|(id, _)| id.as_str().to_ascii_lowercase().contains(&filter))
        .collect();
    objects.sort_by(|a, b| a.0.cmp(&b.0));
    let mut undefined = BTreeMap::new();
    let mut line = |event: &str, c: &aaa_data::VfxCue| -> std::io::Result<()> {
        let what = match vfx.get(&c.vfx) {
            Some(v) => {
                let mut what = match &v.model {
                    Some(m) => format!("{} {m}", v.category),
                    None => v.category.clone(),
                };
                if v.permanent {
                    what += " permanent";
                }
                if let Some(d) = v.duration {
                    what += &format!(" {d}s");
                }
                what
            }
            None => {
                *undefined.entry(c.vfx.to_string()).or_insert(0) += 1;
                "?".into()
            }
        };
        let verb = if c.stop { "stop " } else { "" };
        let at = c.location.as_deref().unwrap_or("");
        let mut extra = String::new();
        if let Some(d) = c.duration {
            extra += &format!(" for {d}s");
        }
        if c.delay > 0.0 {
            extra += &format!(" after {}s", c.delay);
        }
        if c.every > 1 {
            extra += &format!(" every {}", c.every);
        }
        if !c.attached {
            extra += " detached";
        }
        if let Some(t) = &c.terrain {
            extra += &format!(" on {t}");
        }
        let flags = c.targets.required.iter().map(|f| format!("+{f}"));
        let flags: Vec<_> = flags
            .chain(c.targets.excluded.iter().map(|f| format!("-{f}")))
            .collect();
        if !flags.is_empty() {
            extra += &format!(" if {}", flags.join(","));
        }
        writeln!(
            std::io::stdout(),
            "    {event}: {verb}{} at {} {at}{extra}  ({what})",
            c.vfx,
            c.at
        )
    };
    for (id, e) in &objects {
        outln!("{id}");
        let events = [
            ("created", &e.created),
            ("destroyed", &e.destroyed),
            ("constructed", &e.constructed),
            ("start moving", &e.start_moving),
            ("stop moving", &e.stop_moving),
            ("disturbed", &e.disturbed),
        ];
        for (event, cues) in events {
            for c in cues {
                line(event, c)?;
            }
        }
        for (state, c) in &e.entered {
            line(&format!("entered {state}"), c)?;
        }
        for (state, c) in &e.left {
            line(&format!("left {state}"), c)?;
        }
        for h in &e.health {
            line(&format!("health {:?} {}", h.trigger, h.health), &h.cue)?;
        }
    }
    for (id, f) in fighters
        .iter()
        .filter(|(id, _)| id.as_str().to_ascii_lowercase().contains(&filter))
    {
        for (i, w) in f.weapons.iter().enumerate() {
            let e = &w.effects;
            for (event, cues) in [
                ("start", &e.start),
                ("shot", &e.shot),
                ("per target", &e.per_target),
                ("end", &e.end),
            ] {
                for c in cues {
                    line(&format!("{id} weapon {i} {event}"), c)?;
                }
            }
        }
    }
    outln!(
        "{} templates with effects, {} effects",
        objects.len(),
        vfx.len()
    );
    for (id, n) in undefined {
        outln!("undefined: {id} ({n})");
    }
    Ok(())
}

fn add(total: &mut Count, count: Count) {
    total.read += count.read;
    total.unread += count.unread;
}

/// Fields read, fields in all, and the share read.
fn row(c: Count) -> String {
    let all = c.read + c.unread;
    format!("{:>8} {:>8} {:>4}%", c.read, all, 100 * c.read / all.max(1))
}
