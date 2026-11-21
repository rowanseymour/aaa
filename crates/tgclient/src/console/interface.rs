//! Verbs for the window and the interface around a match: screenshots,
//! pressing buttons and keys, waiting, and the clock, scale and tint.

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::time::Real;
use bevy::window::PrimaryWindow;
use tgrender::shot::Taken;

use crate::{Orders, Screen};
use tgsim::Action;

use super::play::barred;
use super::verbs::{Reply, number, usage};
use super::{Console, SETTLE_FRAMES};

pub(crate) fn screenshot(world: &mut World, args: &[&str]) -> Reply {
    let [path] = args else {
        return Err(usage("screenshot"));
    };
    let path = std::path::PathBuf::from(path);
    // A windowless run draws to an image, which is saved once it's settled.
    if !tgrender::shot::take(world, path.clone()) {
        world.spawn(Screenshot::primary_window()).observe(
            move |captured: On<ScreenshotCaptured>, mut commands: Commands| {
                let saved = tgrender::shot::save(&captured.image, &path)
                    .map(|_| path.clone())
                    .map_err(|e| format!("{}: {e}", path.display()));
                commands.trigger(Taken(saved));
            },
        );
    }
    let now = world.resource::<Time<Real>>().elapsed_secs_f64();
    let mut console = world.resource_mut::<Console>();
    console.shots += 1;
    console.shot_asked = now;
    Ok(String::new())
}

/// Says how a screenshot went, and lets the next command run.
pub(crate) fn shot_taken(taken: On<Taken>, mut console: ResMut<Console>) {
    let said = match &taken.0 {
        Ok(path) => format!("saved {}", path.display()),
        Err(e) => format!("screenshot: {e}"),
    };
    console.say(&said);
    console.shots = console.shots.saturating_sub(1);
}

pub(crate) fn ui(world: &mut World) -> String {
    let (buttons, unnamed) = crate::drive::buttons(world);
    let line = |b: &crate::drive::Seen| match b.chosen {
        true => format!("{}  # chosen", b.id),
        false => b.id.clone(),
    };
    let mut lines: Vec<String> = buttons.iter().map(line).collect();
    if unnamed > 0 {
        lines.push(format!("# and {unnamed} with no id"));
    }
    lines.join("\n")
}

pub(crate) fn click(world: &mut World, args: &[&str]) -> Reply {
    let [id] = args else {
        return Err(usage("click"));
    };
    crate::drive::click(world, id)?;
    world.resource_mut::<Console>().settling = SETTLE_FRAMES;
    Ok(String::new())
}

pub(crate) fn press(world: &mut World, args: &[&str]) -> Reply {
    let [chord] = args else {
        return Err(usage("press"));
    };
    let held = crate::drive::press(world, chord)?;
    let mut console = world.resource_mut::<Console>();
    console.held = held;
    console.settling = SETTLE_FRAMES;
    Ok(String::new())
}

pub(crate) fn ask(world: &mut World, args: &[&str]) -> Reply {
    if args.is_empty() {
        return Err(usage("ask"));
    }
    world.write_message(crate::intent::Intent(args.join(" ")));
    world.resource_mut::<Console>().settling = SETTLE_FRAMES;
    Ok(String::new())
}

pub(crate) fn wait(world: &mut World, args: &[&str]) -> Reply {
    let [seconds] = args else {
        return Err(usage("wait"));
    };
    let seconds = number(seconds).ok().filter(|s| (0.0..=600.0).contains(s));
    let seconds = seconds.ok_or("takes 0 to 600 seconds")?;
    let now = world.resource::<Time<Real>>().elapsed_secs_f64();
    world.resource_mut::<Console>().until = Some(now + f64::from(seconds));
    Ok(String::new())
}

/// The match's schedule, which only a match under way carries out.
pub(crate) fn playing(world: &mut World) -> Result<Mut<'_, Orders>, String> {
    let screen = world.get_resource::<State<Screen>>();
    if screen.is_none_or(|s| *s.get() != Screen::Match) {
        return Err("only during a match".into());
    }
    world.get_resource_mut::<Orders>().ok_or("no match".into())
}

pub(super) fn game_speed(world: &mut World, args: &[&str]) -> Reply {
    let mut orders = playing(world)?;
    match args {
        [] => Ok(format!("{}", orders.speeding())),
        [speed] => {
            let speed = number(speed)?;
            if !tgsim::SPEEDS.contains(&speed) {
                let (slowest, fastest) = tgsim::SPEEDS.into_inner();
                return Err(format!("takes {slowest} to {fastest}"));
            }
            barred(&orders, &Action::Speed(speed))?;
            orders.act(Action::Speed(speed));
            Ok(String::new())
        }
        _ => Err(usage("gamespeed")),
    }
}

pub(crate) fn ui_scale(world: &mut World, args: &[&str]) -> Reply {
    let drawn = world.resource::<UiScale>().0;
    let mut scale = world.resource_mut::<crate::ui_scale::UiScaleSetting>();
    match args {
        [] => Ok(format!("{} (drawn at {drawn})", scale.0)),
        [n] => {
            let n = number(n)?;
            if !crate::ui_scale::UI_SCALES.contains(&n) {
                let (low, high) = crate::ui_scale::UI_SCALES.into_inner();
                return Err(format!("takes {low} to {high}"));
            }
            scale.0 = n;
            Ok(String::new())
        }
        _ => Err(usage("uiscale")),
    }
}

pub(super) fn ui_tint(world: &mut World, args: &[&str]) -> Reply {
    let mut theme = world.resource_mut::<crate::ui::Theme>();
    match args {
        [] => {
            let [r, g, b, _] = theme.tint.to_srgba().to_f32_array();
            let [lr, lg, lb, _] = theme.light.to_srgba().to_f32_array();
            Ok(format!("{r} {g} {b} {lr} {lg} {lb}"))
        }
        [_, _, _] | [_, _, _, _, _, _] => {
            let parts = args
                .iter()
                .map(|a| number(a))
                .collect::<Result<Vec<_>, _>>()?;
            if !parts.iter().all(|c| (0.0..=1.0).contains(c)) {
                return Err("takes each part from 0 to 1".into());
            }
            theme.tint = Color::srgb(parts[0], parts[1], parts[2]);
            if let [_, _, _, r, g, b] = parts[..] {
                theme.light = Color::srgb(r, g, b);
            }
            Ok(String::new())
        }
        _ => Err(usage("uitint")),
    }
}

pub(crate) fn resolution(world: &mut World, args: &[&str]) -> Reply {
    let mut windows = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
    let Ok(mut window) = windows.single_mut(world) else {
        return Err("no window".into());
    };
    match args {
        [] => Ok(format!("{} {}", window.width(), window.height())),
        [w, h] => {
            let (w, h) = (number(w)?, number(h)?);
            if w < 1.0 || h < 1.0 {
                return Err("takes a width and height of at least 1".into());
            }
            window.resolution.set(w, h);
            Ok(String::new())
        }
        _ => Err(usage("resolution")),
    }
}

pub(super) fn game_pause(world: &mut World, args: &[&str]) -> Reply {
    let mut orders = playing(world)?;
    let pause = match args {
        [] => !orders.pausing(),
        ["1"] => true,
        ["0"] => false,
        _ => return Err(usage("gamepause")),
    };
    barred(&orders, &Action::Pause(pause))?;
    orders.act(Action::Pause(pause));
    if pause && let Some(mut display) = world.get_resource_mut::<crate::displays::Displays>() {
        display.keep_paused();
    }
    Ok(String::new())
}
