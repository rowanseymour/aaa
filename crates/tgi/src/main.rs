use std::io::Write;
use std::process::ExitCode;

use tgi::{Block, Item};

const USAGE: &str = "\
usage:
  tgi check <file.tgi>...    parse files and report warnings
  tgi dump  <file.tgi>       print the parsed tree in normalised form";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["check", files @ ..] if !files.is_empty() => check(files),
        ["dump", file] => dump(file),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("tgi: {e}");
            ExitCode::FAILURE
        }
    }
}

fn check(files: &[&str]) -> std::io::Result<()> {
    let mut out = std::io::stdout().lock();
    let (mut blocks, mut fields, mut warnings) = (0, 0, 0);
    for path in files {
        let parsed = tgi::parse_bytes(&std::fs::read(path)?);
        count(&parsed.root, &mut blocks, &mut fields);
        for w in &parsed.warnings {
            writeln!(out, "{path}:{}: {}", w.line, w.message)?;
        }
        warnings += parsed.warnings.len();
    }
    writeln!(
        out,
        "{} files, {blocks} blocks, {fields} fields, {warnings} warnings",
        files.len()
    )
}

fn count(b: &Block, blocks: &mut usize, fields: &mut usize) {
    for item in &b.items {
        match item {
            Item::Field(_) => *fields += 1,
            Item::Block(child) => {
                *blocks += 1;
                count(child, blocks, fields);
            }
        }
    }
}

fn dump(path: &str) -> std::io::Result<()> {
    let parsed = tgi::parse_bytes(&std::fs::read(path)?);
    let mut out = std::io::stdout().lock();
    write_items(&mut out, &parsed.root, 0)?;
    for w in &parsed.warnings {
        eprintln!("{path}:{}: {}", w.line, w.message);
    }
    Ok(())
}

fn write_items(out: &mut impl Write, block: &Block, depth: usize) -> std::io::Result<()> {
    let pad = "    ".repeat(depth);
    for item in &block.items {
        match item {
            Item::Field(f) => {
                write!(out, "{pad}")?;
                if let Some(ty) = &f.ty {
                    write!(out, "{ty} ")?;
                }
                write!(out, "{}", f.key)?;
                match (&f.value, f.quoted) {
                    (Some(v), true) => write!(out, " = \"{v}\"")?,
                    (Some(v), false) => write!(out, " = {v}")?,
                    (None, _) => {}
                }
                writeln!(out)?;
            }
            Item::Block(b) => {
                let mut parts = b.header.words.clone();
                parts.extend(b.header.attrs.iter().map(|(k, v)| format!("{k}={v}")));
                writeln!(out, "{pad}[{}]", parts.join(" "))?;
                writeln!(out, "{pad}{{")?;
                write_items(out, b, depth + 1)?;
                writeln!(out, "{pad}}}")?;
            }
        }
    }
    Ok(())
}
