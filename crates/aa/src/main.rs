//! Plays Axis & Allies (2004) matches: [`tgclient`] with what's AA's own.

use std::process::ExitCode;

use tgclient::{Color, Emblem, Palette, Program, Shape, View};

const RED: Color = Color::srgb(0.75, 0.1, 0.1);
const NAVY: Color = Color::srgb(0.1, 0.15, 0.35);
const WHITE: Color = Color::WHITE;
const BLACK: Color = Color::BLACK;

/// Each nation's emblem, after the markings on its aircraft.
const EMBLEMS: &[(&str, Emblem)] = &[
    (
        "nation_usa",
        Emblem(&[Shape::Disc(1.0, NAVY), Shape::Star(0.95, WHITE)]),
    ),
    (
        "nation_gbr",
        Emblem(&[
            Shape::Disc(1.0, NAVY),
            Shape::Disc(0.66, WHITE),
            Shape::Disc(0.33, RED),
        ]),
    ),
    (
        "nation_ger",
        Emblem(&[
            Shape::Cross(0.9, 0.72, WHITE),
            Shape::Cross(0.9, 0.4, BLACK),
        ]),
    ),
    (
        "nation_rus",
        Emblem(&[
            Shape::Star(1.0, RED),
            Shape::Star(0.86, WHITE),
            Shape::Star(0.76, RED),
        ]),
    ),
    (
        "nation_jpn",
        Emblem(&[Shape::Disc(1.0, WHITE), Shape::Disc(0.88, RED)]),
    ),
];

fn main() -> ExitCode {
    tgclient::run(Program {
        name: "tag-aa",
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
        emblems: EMBLEMS,
    })
}
