//! Plays Axis & Allies (2004) matches: [`tgclient`] with what's AA's own.

use std::process::ExitCode;

use tgclient::Program;

fn main() -> ExitCode {
    tgclient::run(Program {
        name: "tag-aa",
        tag: "aa",
        example_map: "allies10_berlin",
    })
}
