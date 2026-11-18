//! Plays Kohan II matches: [`tgclient`] with what's K2's own.

use std::process::ExitCode;

use tgclient::{Color, Palette, Program, View};

fn main() -> ExitCode {
    tgclient::run(Program {
        name: "tag-k2",
        tag: "k2",
        example_map: "(1v1)Battleground",
        logo: None,
        title: "K2",
        // A dark umber, parchment and gold.
        palette: Palette {
            tint: Color::srgb_u8(0x17, 0x11, 0x0b),
            light: Color::srgb_u8(0xe6, 0xd8, 0xb5),
            accent: Color::srgb(0.8, 0.62, 0.25),
        },
        backdrop: View {
            map: "(1v1)Battleground",
            focus: [128.0, 128.0],
        },
    })
}
