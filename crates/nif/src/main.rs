use std::process::ExitCode;

use nif::{Nif, Object};

const USAGE: &str = "\
usage:
  nif check <file.nif>...    read files and report the ones that fail or stop early
  nif dump  <file.nif>       print the scene graph and meshes";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["check", files @ ..] if !files.is_empty() => check(files),
        ["dump", file] => dump(file),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn read(path: &str) -> Result<Nif, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    Nif::parse(&bytes).map_err(|e| format!("{path}: {e}"))
}

fn check(files: &[&str]) -> ExitCode {
    let (mut complete, mut partial, mut failed) = (0, 0, 0);
    for &file in files {
        match read(file) {
            Ok(nif) => match nif.incomplete {
                None => complete += 1,
                Some(kind) => {
                    println!("{file}: stops at {kind}");
                    partial += 1;
                }
            },
            Err(e) => {
                println!("{e}");
                failed += 1;
            }
        }
    }
    println!("{complete} complete, {partial} stop early, {failed} failed");
    if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn dump(file: &str) -> ExitCode {
    let nif = match read(file) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("nif: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(kind) = &nif.incomplete {
        println!("(stops at {kind}; later objects are missing)");
    }
    for (i, o) in nif.objects.iter().enumerate() {
        let line = match o {
            Object::Node(n) => format!(
                "node {:?} flags {:#06x} children {:?} at {:?}",
                n.av.name, n.av.flags, n.children, n.av.transform.translation
            ),
            Object::Shape(s) => format!(
                "shape {:?} flags {:#06x} data {:?} properties {:?}",
                s.av.name, s.av.flags, s.data, s.av.properties
            ),
            Object::Geometry(g) => format!(
                "geometry {} vertices, {} triangles, {} uv sets, colours {}",
                g.positions.len(),
                g.triangles.len(),
                g.uv_sets.len(),
                !g.colors.is_empty()
            ),
            Object::Texturing(t) => {
                format!("texturing base {:?}", t.base.as_ref().map(|b| b.source))
            }
            Object::SourceTexture(t) => format!("texture {:?} pixels {:?}", t.file, t.pixels),
            Object::Pixels(p) => format!("pixels format {} levels {:?}", p.format, p.mipmaps),
            Object::Material(m) => format!("material diffuse {:?} alpha {}", m.diffuse, m.alpha),
            Object::Alpha(a) => format!("alpha flags {:#06x} threshold {}", a.flags, a.threshold),
            Object::Stencil(s) => format!("stencil draw mode {}", s.draw_mode),
            Object::ExtraData(e) => format!("extra {:?} = {:?}", e.name, e.value),
            Object::Other(kind) => kind.clone(),
        };
        println!("{i:4} {line}");
    }
    println!("roots {:?}", nif.roots);
    ExitCode::SUCCESS
}
