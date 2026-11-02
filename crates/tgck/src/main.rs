use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::process::ExitCode;

use tgck::{Archive, Container, Map};

const USAGE: &str = "\
usage:
  tgck list     <archive.rwd> [filter]          list entries (filter: case-insensitive substring)
  tgck stats    <archive.rwd>...                file counts and sizes by extension
  tgck extract  <archive.rwd> <out-dir> [filter]
  tgck cat      <archive.rwd> <entry-name>      write one entry to stdout
  tgck sections <file>                          list the sections of any TGCK file
  tgck unpack   <file> <out-dir>                write each section, decompressed, as <name>.bin
  tgck map      <map.rmp>...                    summarise maps";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["list", ar, rest @ ..] if rest.len() <= 1 => list(ar, rest.first().copied()),
        ["stats", ars @ ..] if !ars.is_empty() => ars.iter().try_for_each(|a| stats(a)),
        ["extract", ar, out, rest @ ..] if rest.len() <= 1 => {
            extract(ar, out, rest.first().copied())
        }
        ["cat", ar, name] => cat(ar, name),
        ["sections", file] => sections(file),
        ["unpack", file, out] => unpack(file, out),
        ["map", maps @ ..] if !maps.is_empty() => maps.iter().try_for_each(|m| map(m)),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) if is_broken_pipe(e.as_ref()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("tgck: {e}");
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

/// A closed stdout means the reader has seen all it wants, not a failure.
fn is_broken_pipe(e: &(dyn std::error::Error + 'static)) -> bool {
    let io = match e.downcast_ref::<tgck::Error>() {
        Some(tgck::Error::Io(io)) => Some(io),
        _ => e.downcast_ref::<std::io::Error>(),
    };
    io.is_some_and(|io| io.kind() == std::io::ErrorKind::BrokenPipe)
}

fn matches(name: &str, filter: Option<&str>) -> bool {
    filter.is_none_or(|f| name.to_lowercase().contains(&f.to_lowercase()))
}

fn list(path: &str, filter: Option<&str>) -> Res {
    let ar = Archive::open(path)?;
    let mut out = BufWriter::new(std::io::stdout().lock());
    for e in ar.entries.iter().filter(|e| matches(&e.name, filter)) {
        writeln!(out, "{:>10}  {}", e.size, e.name)?;
    }
    Ok(())
}

fn stats(path: &str) -> Res {
    let ar = Archive::open(path)?;
    let mut by_ext: BTreeMap<String, (usize, u64)> = BTreeMap::new();
    for e in &ar.entries {
        let ext = Path::new(&e.name)
            .extension()
            .map_or("(none)".into(), |x| x.to_string_lossy().to_lowercase());
        let slot = by_ext.entry(ext).or_default();
        slot.0 += 1;
        slot.1 += e.size;
    }
    let total: u64 = ar.entries.iter().map(|e| e.size).sum();
    outln!("{path}: {} files, {}", ar.entries.len(), human(total));
    let mut rows: Vec<_> = by_ext.into_iter().collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.1.1));
    for (ext, (n, bytes)) in rows {
        outln!("  {ext:>8}  {n:>6} files  {:>10}", human(bytes));
    }
    Ok(())
}

fn extract(path: &str, out_dir: &str, filter: Option<&str>) -> Res {
    let mut ar = Archive::open(path)?;
    let entries: Vec<_> = ar
        .entries
        .iter()
        .filter(|e| matches(&e.name, filter))
        .cloned()
        .collect();
    let (mut written, mut bytes, mut skipped) = (0usize, 0u64, 0usize);
    for e in &entries {
        let Some(rel) = e.safe_path() else {
            eprintln!("skipping unsafe path: {:?}", e.name);
            skipped += 1;
            continue;
        };
        let dest = Path::new(out_dir).join(rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut f = BufWriter::new(File::create(&dest)?);
        bytes += ar.copy_to(e, &mut f)?;
        f.flush()?;
        written += 1;
    }
    outln!(
        "extracted {written} files ({}) to {out_dir}; skipped {skipped}",
        human(bytes)
    );
    Ok(())
}

fn cat(path: &str, name: &str) -> Res {
    let mut ar = Archive::open(path)?;
    let e = ar
        .find(name)
        .ok_or_else(|| format!("no entry named {name:?}"))?
        .clone();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    ar.copy_to(&e, &mut out)?;
    out.flush()?;
    Ok(())
}

fn sections(path: &str) -> Res {
    let c = Container::open(path)?;
    outln!("{path}: TGCK v{}, {} sections", c.version, c.sections.len());
    for s in &c.sections {
        let packing = if s.is_compressed() { "zlib" } else { "raw" };
        outln!(
            "  {:<28} @{:#010x}  {:>10} -> {:>10}  {packing}",
            s.name,
            s.offset,
            s.stored_size,
            s.unpacked_size
        );
    }
    Ok(())
}

fn unpack(path: &str, out_dir: &str) -> Res {
    let mut c = Container::open(path)?;
    fs::create_dir_all(out_dir)?;
    let mut written = 0;
    for s in c.sections.clone() {
        // Section names come from the file; keep only safe characters.
        let safe: String = s
            .name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if safe.is_empty() {
            eprintln!("skipping section with unusable name {:?}", s.name);
            continue;
        }
        fs::write(
            Path::new(out_dir).join(format!("{safe}.bin")),
            c.read_section(&s)?,
        )?;
        written += 1;
    }
    outln!("unpacked {written} sections to {out_dir}");
    Ok(())
}

fn map(path: &str) -> Res {
    let mut m = Map::open(path)?;
    let strings = m.strings()?;
    let name = strings
        .lines()
        .find_map(|l| l.trim().strip_prefix("map_name = "))
        .map_or("?", |n| n.trim_matches('"'));
    let hm = m.height_map()?;
    let (lo, hi) = hm
        .heights
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), &h| (lo.min(h), hi.max(h)));
    let water = hm.layer.iter().filter(|&&b| b != 0).count();
    let terrain = m.terrain()?;
    if terrain.side + 1 != hm.side {
        return Err(format!(
            "{path}: {} terrain tiles per side but {} height vertices",
            terrain.side, hm.side
        )
        .into());
    }
    let things = m.things()?;
    let extent = (hm.side - 1) as f32 * tgck::map::CELL_SIZE;
    outln!("{path}: \"{name}\"");
    let side = hm.side;
    let water_pct = 100.0 * water as f64 / hm.layer.len() as f64;
    outln!(
        "  terrain {side}x{side} vertices ({extent} world units), heights {lo:.2}..{hi:.2}, water {water_pct:.1}%"
    );
    let (used, top) = top_counts(terrain.tiles.iter().map(|t| terrain.name_of(t)), 6);
    outln!(
        "  terrain v{}, biome {}, {used} of {} types used: {top}",
        terrain.version,
        terrain.biome,
        terrain.names.len(),
    );
    let (templates, top) = top_counts(things.iter().map(|t| t.template.as_str()), 8);
    outln!("  {} objects, {templates} templates: {top}", things.len());
    let starts: Vec<String> = things
        .iter()
        .filter(|t| t.template == "start_loc")
        .map(|t| format!("({:.0},{:.0})", t.position[0], t.position[1]))
        .collect();
    outln!("  start locations: {}", starts.join(" "));
    Ok(())
}

/// The number of distinct names, and the `n` most common as "name count, ...".
fn top_counts<'a>(names: impl Iterator<Item = &'a str>, n: usize) -> (usize, String) {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for name in names {
        *counts.entry(name).or_default() += 1;
    }
    let mut rows: Vec<_> = counts.into_iter().collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.1));
    let top: Vec<String> = rows
        .iter()
        .take(n)
        .map(|(t, n)| format!("{t} {n}"))
        .collect();
    (rows.len(), top.join(", "))
}

fn human(n: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}
