use std::collections::BTreeMap;
use std::io::Write;
use std::process::ExitCode;

use aaa_data::GameData;
use tgi::de::Count;

const USAGE: &str = "\
usage:
  aaa-data coverage <game dir>          how much of each top-level block kind the definitions read
  aaa-data coverage <game dir> <kind>   each key of one kind, e.g. Thing, with how often it is read
  aaa-data audio    <game dir> [filter] sounds, speech and music by id, with their files and any missing";

fn main() -> ExitCode {
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
    let mut audio: Vec<_> = GameData::open(dir.as_ref())?
        .audio()
        .into_iter()
        .filter(|(id, _)| id.as_str().to_ascii_lowercase().contains(&filter))
        .collect();
    audio.sort_by(|a, b| a.0.cmp(&b.0));
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

fn add(total: &mut Count, count: Count) {
    total.read += count.read;
    total.unread += count.unread;
}

/// Fields read, fields in all, and the share read.
fn row(c: Count) -> String {
    let all = c.read + c.unread;
    format!("{:>8} {:>8} {:>4}%", c.read, all, 100 * c.read / all.max(1))
}
