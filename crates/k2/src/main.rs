//! Plays Kohan II matches: [`tgclient`] with what's K2's own.

use std::process::ExitCode;

use tgclient::Program;

fn main() -> ExitCode {
    tgclient::run(Program {
        name: "k2",
        tag: "k2",
        example_map: "(1v1)Battleground",
        version: env!("CARGO_PKG_VERSION"),
    })
}
