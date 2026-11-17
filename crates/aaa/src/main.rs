//! Plays Axis & Allies (2004) matches: [`tgclient`] with what's AA's own.

use std::process::ExitCode;

use tgclient::{Color, Palette, Program, View};

fn main() -> ExitCode {
    tgclient::run(Program {
        name: "aaa-game",
        tag: "aa",
        example_map: "allies10_berlin",
        logo: Some(include_bytes!("../../../assets/logo/logo.png")),
        title: "AAA",
        // A dark olive and a cream, to go with the logo.
        palette: Palette {
            tint: Color::srgb_u8(0x0f, 0x14, 0x0c),
            light: Color::srgb_u8(0xdc, 0xd9, 0xc6),
            accent: Color::srgb(0.75, 0.6, 0.3),
        },
        // The beach a match on Normandy starts on, and its defences.
        backdrop: View {
            map: "allies06_normandy",
            focus: [341.0, 233.0],
        },
    })
}
