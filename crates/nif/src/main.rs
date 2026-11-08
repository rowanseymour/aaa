use std::io::{self, Write};
use std::process::ExitCode;

use nif::{Keys, Nif, Object, Rotation};

const USAGE: &str = "\
usage:
  nif check <file.nif>...    read files and report the ones that fail or stop early
  nif dump  <file.nif>       print the objects in a model or animation";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["check", files @ ..] if !files.is_empty() => finish(check(files)),
        ["dump", file] => finish(dump(file)),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Output cut short by a closed pipe (`nif dump x | head`) is no failure.
fn finish(result: io::Result<ExitCode>) -> ExitCode {
    match result {
        Ok(code) => code,
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("nif: {e}");
            ExitCode::FAILURE
        }
    }
}

macro_rules! outln {
    ($($arg:tt)*) => {
        writeln!(io::stdout(), $($arg)*)?
    };
}

fn read(path: &str) -> Result<Nif, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    Nif::parse(&bytes).map_err(|e| format!("{path}: {e}"))
}

fn check(files: &[&str]) -> io::Result<ExitCode> {
    let (mut complete, mut partial, mut failed) = (0, 0, 0);
    for &file in files {
        match read(file) {
            Ok(nif) => match nif.incomplete {
                None => complete += 1,
                Some(kind) => {
                    outln!("{file}: stops at {kind}");
                    partial += 1;
                }
            },
            Err(e) => {
                outln!("{e}");
                failed += 1;
            }
        }
    }
    outln!("{complete} complete, {partial} stop early, {failed} failed");
    Ok(if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn dump(file: &str) -> io::Result<ExitCode> {
    let nif = match read(file) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("nif: {e}");
            return Ok(ExitCode::FAILURE);
        }
    };
    if let Some(kind) = &nif.incomplete {
        outln!("(stops at {kind}; later objects are missing)");
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
            Object::VertexColor(v) => {
                format!(
                    "vertex colour mode {} lighting {}",
                    v.vertex_mode, v.lighting
                )
            }
            Object::ExtraData(e) => format!("extra {:?} = {:?}", e.name, e.value),
            Object::Controller(c) => format!(
                "{:?} controller target {:?}{} data {:?} flags {:#06x} frequency {} phase {} time {}..{} next {:?}",
                c.kind,
                c.target,
                match c.modifier.as_str() {
                    "" => String::new(),
                    m => format!(" modifier {m:?}"),
                },
                c.data,
                c.flags,
                c.frequency,
                c.phase,
                c.start,
                c.stop,
                c.next
            ),
            Object::Keyframes(k) => {
                let rotation = match &k.rotation {
                    Rotation::Quaternion(q) => keys(q),
                    Rotation::Euler(axes) => format!(
                        "x {} y {} z {}",
                        keys(&axes[0]),
                        keys(&axes[1]),
                        keys(&axes[2])
                    ),
                };
                format!(
                    "keyframes rotation {rotation} translation {} scale {}",
                    keys(&k.translation),
                    keys(&k.scale)
                )
            }
            Object::Floats(k) => format!("floats {}", keys(k)),
            Object::Sequence(s) => {
                let blocks: String = s
                    .blocks
                    .iter()
                    .map(|b| format!("\n       {:?} controller {:?}", b.target, b.controller))
                    .collect();
                format!(
                    "sequence {:?} accumulation root {:?} text keys {:?}, {} blocks{blocks}",
                    s.name,
                    s.accumulation_root,
                    s.text_keys,
                    s.blocks.len()
                )
            }
            Object::TextKeys(t) => {
                let keys: Vec<_> = t
                    .keys
                    .iter()
                    .map(|k| format!("{}: {:?}", k.time, k.text))
                    .collect();
                format!("text keys {:?} [{}]", t.name, keys.join(", "))
            }
            Object::SkinInstance(s) => format!(
                "skin data {:?} skeleton root {:?} bones {:?}",
                s.data, s.skeleton_root, s.bones
            ),
            Object::SkinData(s) => {
                let bones: String = s
                    .bones
                    .iter()
                    .map(|b| {
                        format!(
                            "\n       bound {:?}, {} weights, at {:?}",
                            b.bound,
                            b.weights.len(),
                            b.transform.translation
                        )
                    })
                    .collect();
                format!(
                    "skin data partition {:?} at {:?}, {} bones{bones}",
                    s.partition,
                    s.transform.translation,
                    s.bones.len()
                )
            }
            Object::ParticleSystem(p) => format!(
                "particle system {:?} flags {:#06x} data {:?} world space {} modifiers {:?} properties {:?}",
                p.shape.av.name,
                p.shape.av.flags,
                p.shape.data,
                p.world_space,
                p.modifiers,
                p.shape.av.properties
            ),
            Object::ParticleData(d) => format!("particle data for {}", d.capacity),
            Object::Modifier(m) => format!(
                "modifier {:?} order {} active {} {:?}",
                m.name, m.order, m.active, m.kind
            ),
            Object::Collider(c) => format!("collider {c:?}"),
            Object::EmitterKeys(e) => {
                format!(
                    "emitter birth rate {} active {:?}",
                    keys(&e.birth_rate),
                    e.active
                )
            }
            Object::Colors(k) => format!("colours {}", keys(k)),
            Object::Visibility(v) => format!("visibility {v:?}"),
            Object::Other(kind) => kind.clone(),
        };
        outln!("{i:4} {line}");
    }
    outln!("roots {:?}", nif.roots);
    Ok(ExitCode::SUCCESS)
}

fn keys<T: std::fmt::Debug>(k: &Keys<T>) -> String {
    let keys: Vec<_> = k
        .keys
        .iter()
        .map(|k| format!("{}: {:?}", k.time, k.value))
        .collect();
    format!("{:?} [{}]", k.interpolation, keys.join(", "))
}
