//! Plays Kohan II matches: [`tgclient`] with what's K2's own.

use std::process::ExitCode;

use tgclient::Program;

fn main() -> ExitCode {
    tgclient::run(Program {
        name: "tag-k2",
        tag: "k2",
        example_map: "(1v1)Battleground",
    })
}
