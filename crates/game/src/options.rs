//! The options, a page over the whole screen that the main menu and the
//! menu over a match both open, and what's chosen in them.

use aaa_data::{GameData, HotkeyConfiguration, Id, SoundSwitches};
use aaa_render::camera::RtsCamera;
use bevy::anti_alias::fxaa::Fxaa;
use bevy::anti_alias::smaa::Smaa;
use bevy::ecs::observer::IntoEntityObserver;
use bevy::ecs::system::NonSendMarker;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, SliderValue, ValueChange};
use bevy::window::{
    Monitor, MonitorSelection, OnMonitor, PrimaryMonitor, PrimaryWindow, RawHandleWrapper,
    VideoModeSelection, WindowMode, WindowResized,
};
use bevy::winit::WINIT_WINDOWS;
use raw_window_handle::RawDisplayHandle;

use crate::sound::{Channel, Sounds, Volumes};
use crate::supply::Border;
use crate::ui::{
    Card, Chosen, Dropdown, Font, Ink, Surface, button, called, card, label, layer, padded, slider,
    slug, switch, text, toggle,
};
use crate::{Screen, UI_SCALES, UiScaleSetting, menu};

const HOTKEYS: &str = "options_dialog_Child_text_6";
const BORDER: &str = "options_dialog_Child_text_15";
const RESOLUTION: &str = "options_dialog_Child_text_18";
const GAME_TAB: &str = "options_dialog_ShowGameplayButton_Tooltip";
const VIDEO_TAB: &str = "options_dialog_ShowVideoButton_Tooltip";
const AUDIO_TAB: &str = "options_dialog_ShowAudioButton_Tooltip";
const VOLUME: &str = "options_dialog_AudioPage_VolumeTitle_Label_Text";
const SOUNDS: &str = "options_dialog_AudioPage_SoundsTitle_Label_Text";
const CHAT: &str = "options_dialog_Child_text_31";
const REPLIES: &str = "options_dialog_Child_text_32";
/// What the game calls each of the [`Volumes`], by [`Channel::ALL`].
const VOLUMES: [(&str, &str); 4] = [
    ("options_dialog_Child_text_27", "Main Volume"),
    ("options_dialog_Child_text_28", "FX Volume"),
    ("options_dialog_Child_text_29", "Speech Volume"),
    ("options_dialog_Child_text_30", "Music Volume"),
];
const GAME: &str = "options_dialog_GamePlayPage_GameTitle_Label_Text";
const EXTRAS: &str = "options_dialog_GamePlayPage_MapTitle_Label_Text";
const BASIC: &str = "options_dialog_VideoPage_BasicTitle_Label_Text";
const OK: &str = "options_dialog_OptionsButtonOK_Client_View_text";

/// What the player has chosen, and what there is to choose from.
#[derive(Resource)]
pub struct Options {
    /// The sets of hotkeys the game has, as it lists them.
    configurations: Vec<HotkeyConfiguration>,
    /// The id of the set in use.
    hotkeys: Option<Id>,
    /// What the game calls the choice of hotkeys.
    hotkeys_label: String,
    /// What the game calls the choice to show the supply border.
    border_label: String,
    /// What the game calls the choice of the window's size.
    resolution_label: String,
    /// What the choices of [`Display`], [`AntiAliasing`] and the
    /// interface's scale are called, which the game has no words for.
    display_label: String,
    anti_aliasing_label: String,
    ui_scale_label: String,
    /// What the game calls each tab, by [`Tab`], and the groups of options
    /// in them.
    game_tab: String,
    video_tab: String,
    audio_tab: String,
    game_title: String,
    extras_title: String,
    basic_title: String,
    volume_title: String,
    sounds_title: String,
    /// What the game calls each [`Sound`] switch.
    chat_label: String,
    replies_label: String,
    /// What the game calls each of the [`Volumes`], by [`Channel::ALL`].
    volume_labels: [String; 4],
    /// What the game's button to leave the options says.
    ok: String,
}

impl Options {
    /// The game's own choices.
    pub fn new(game: &GameData) -> Self {
        let strings = game.strings();
        let configurations = game.hotkey_configurations(&strings);
        let named = game.default_hotkey_configuration();
        Options::of(configurations, named, &strings)
    }

    /// With `named` the set of hotkeys in use, or the first where there's
    /// no such set.
    pub fn of(
        configurations: Vec<HotkeyConfiguration>,
        named: Option<Id>,
        strings: &aaa_data::Strings,
    ) -> Self {
        let known = named.filter(|id| configurations.iter().any(|c| c.id == *id));
        let said = crate::ui::said(strings);
        // The game calls each tab "... Options", which the page's title
        // already says.
        let tab = |key: &str, or: &str| {
            let said = said(key, or);
            said.strip_suffix(" Options").unwrap_or(&said).to_string()
        };
        Options {
            hotkeys: known.or_else(|| configurations.first().map(|c| c.id.clone())),
            configurations,
            hotkeys_label: said(HOTKEYS, "Hotkeys"),
            border_label: said(BORDER, "Supply Border"),
            resolution_label: said(RESOLUTION, "Resolution"),
            display_label: "Display".into(),
            anti_aliasing_label: "Anti-aliasing".into(),
            ui_scale_label: "Interface Scale".into(),
            game_tab: tab(GAME_TAB, "Gameplay"),
            video_tab: tab(VIDEO_TAB, "Video"),
            audio_tab: tab(AUDIO_TAB, "Audio"),
            game_title: said(GAME, "Game"),
            extras_title: said(EXTRAS, "Auxiliaries"),
            basic_title: said(BASIC, "Basic"),
            volume_title: said(VOLUME, "Volume"),
            sounds_title: said(SOUNDS, "Sounds"),
            chat_label: said(CHAT, "Chat Notification"),
            replies_label: said(REPLIES, "Unit Acknowledgement"),
            volume_labels: VOLUMES.map(|(key, or)| said(key, or)),
            ok: said(OK, "Ok"),
        }
    }

    /// The set of hotkeys in use, if the game has any.
    pub fn hotkeys(&self) -> Option<&HotkeyConfiguration> {
        let chosen = self.hotkeys.as_ref()?;
        self.configurations.iter().find(|c| c.id == *chosen)
    }
}

/// Which list of choices is open.
#[derive(Resource, Default, PartialEq, Clone, Copy, Debug)]
enum Open {
    #[default]
    Nothing,
    Hotkeys,
    Display,
    Resolution,
    AntiAliasing,
    UiScale,
}

/// The options shown: those of the game, of how it's drawn, or of how it
/// sounds.
#[derive(Resource, Default, PartialEq, Clone, Copy, Debug)]
enum Tab {
    #[default]
    Game,
    Video,
    Audio,
}

/// The options, over the whole screen.
#[derive(Component)]
pub struct Page;

/// A button showing a [`Tab`]'s options.
#[derive(Component)]
struct TabButton(Tab);

/// Where the shown tab's options go.
#[derive(Component)]
struct Body;

/// Where the choice of hotkeys goes.
#[derive(Component)]
struct Hotkeys;

/// A button choosing a set of hotkeys, by its id.
#[derive(Component)]
struct HotkeysChoice(Id);

/// A choice from a fixed list, shown by name.
trait Listed: Copy + PartialEq + Send + Sync + 'static {
    const ALL: &'static [Self];
    /// The list it's chosen from.
    const LIST: Open;
    /// What the list's button is [`called`](crate::ui::called).
    const ID: &'static str;
    fn name(self) -> &'static str;
}

/// A button choosing a [`Listed`].
#[derive(Component)]
struct Pick<T: Listed>(T);

/// Spawns the list to choose a `T` from in `holder`, saying `current`.
fn listed<T: Listed, M>(
    commands: &mut Commands,
    holder: Entity,
    open: Open,
    current: Option<T>,
    choices: impl Iterator<Item = T>,
    choose: impl IntoEntityObserver<M> + Copy,
) {
    commands.entity(holder).despawn_children();
    let list = Dropdown {
        id: T::ID.into(),
        row: holder,
        width: px(LIST_WIDTH),
        label: current.map_or("", T::name).into(),
        open: open == T::LIST,
    };
    let choices = choices.map(|c| (slug(c.name()), (Pick(c), InList), c.name().to_string()));
    list.spawn(
        commands,
        (Opener(T::LIST), InList),
        open_list,
        choices,
        choose,
    );
}

/// How the window takes the screen.
#[derive(PartialEq, Clone, Copy, Debug)]
enum Display {
    /// With a frame, at the size chosen.
    Windowed,
    /// Without a frame, covering the screen at the size the screen is
    /// already: switching to or from other windows is quick, as nothing
    /// changes the screen's own mode.
    Borderless,
    /// Taking the screen for itself, in the screen's mode.
    Fullscreen,
}

impl Listed for Display {
    const ALL: &'static [Self] = &[Display::Windowed, Display::Borderless, Display::Fullscreen];
    const LIST: Open = Open::Display;
    const ID: &'static str = "options.display";

    fn name(self) -> &'static str {
        match self {
            Display::Windowed => "Windowed",
            Display::Borderless => "Borderless",
            Display::Fullscreen => "Fullscreen",
        }
    }
}

impl Display {
    fn of(mode: WindowMode) -> Self {
        match mode {
            WindowMode::Windowed => Display::Windowed,
            WindowMode::BorderlessFullscreen(_) => Display::Borderless,
            WindowMode::Fullscreen(..) => Display::Fullscreen,
        }
    }

    fn mode(self) -> WindowMode {
        match self {
            Display::Windowed => WindowMode::Windowed,
            Display::Borderless => WindowMode::BorderlessFullscreen(MonitorSelection::Current),
            Display::Fullscreen => {
                WindowMode::Fullscreen(MonitorSelection::Current, VideoModeSelection::Current)
            }
        }
    }
}

/// How the edges of the map's 3D scene are smoothed.
#[derive(Resource, Default, PartialEq, Clone, Copy, Debug)]
enum AntiAliasing {
    Off,
    /// Four samples to each pixel: the one count every GPU wgpu runs on
    /// takes.
    #[default]
    Msaa,
    Fxaa,
    Smaa,
}

impl Listed for AntiAliasing {
    const ALL: &'static [Self] = &[
        AntiAliasing::Off,
        AntiAliasing::Msaa,
        AntiAliasing::Fxaa,
        AntiAliasing::Smaa,
    ];
    const LIST: Open = Open::AntiAliasing;
    const ID: &'static str = "options.anti-aliasing";

    fn name(self) -> &'static str {
        match self {
            AntiAliasing::Off => "Off",
            AntiAliasing::Msaa => "MSAA",
            AntiAliasing::Fxaa => "FXAA",
            AntiAliasing::Smaa => "SMAA",
        }
    }
}

/// Where the choice of [`AntiAliasing`] goes.
#[derive(Component)]
struct AntiAliasingHolder;

/// Where the choice of the interface's scale goes.
#[derive(Component)]
struct UiScaleHolder;

/// A button choosing the interface's scale.
#[derive(Component)]
struct UiScaleChoice(f32);

/// The interface's scales offered, of [`UI_SCALES`].
const UI_SCALE_CHOICES: [f32; 7] = [0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0];

/// A scale as the options show it, e.g. `125%`.
fn percentage(scale: f32) -> String {
    format!("{:.0}%", scale * 100.0)
}

/// Where the choice of [`Display`] goes.
#[derive(Component)]
struct DisplayHolder;

/// The window's size before it last took the whole screen, to go back to.
#[derive(Resource, Default)]
struct WindowedSize(Option<Vec2>);

/// Where the choice of the window's size goes.
#[derive(Component)]
struct Resolution;

/// A button choosing a size for the window.
#[derive(Component)]
struct ResolutionChoice(UVec2);

/// A button that opens a list of choices, by the list.
#[derive(Component)]
struct Opener(Open);

/// A button of a list of choices: what opens it, or a choice.
#[derive(Component)]
struct InList;

/// A slider setting one of the [`Volumes`].
#[derive(Component)]
struct VolumeSlider(Channel);

/// Says what one of the [`Volumes`] is at, as a percentage.
#[derive(Component)]
struct VolumeLevel(Channel);

/// A sound the options switch on or off.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Sound {
    Chat,
    Replies,
}

/// The button switching a [`Sound`].
#[derive(Component)]
struct SoundToggle(Sound);

impl Options {
    fn sound_label(&self, sound: Sound) -> &str {
        match sound {
            Sound::Chat => &self.chat_label,
            Sound::Replies => &self.replies_label,
        }
    }
}

impl Sound {
    fn on(self, switches: &mut SoundSwitches) -> &mut bool {
        match self {
            Sound::Chat => &mut switches.chat,
            Sound::Replies => &mut switches.replies,
        }
    }
}

/// The button that shows or hides the supply border.
#[derive(Component)]
struct BorderToggle;

/// The sizes offered for the window, of those the screen has room for
/// with its frame.
const RESOLUTIONS: [(u32, u32); 9] = [
    (1024, 768),
    (1280, 720),
    (1280, 800),
    (1366, 768),
    (1440, 900),
    (1600, 900),
    (1920, 1080),
    (2560, 1440),
    (3840, 2160),
];

pub struct OptionsPlugin;

impl Plugin for OptionsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Open>()
            .init_resource::<Tab>()
            .init_resource::<AntiAliasing>()
            .init_resource::<WindowedSize>()
            .add_systems(OnEnter(Screen::Options), screen)
            .add_observer(set_volume)
            .add_systems(PreUpdate, follow_window)
            .add_systems(PostUpdate, anti_alias)
            .add_systems(
                Update,
                (
                    close_list,
                    show_body,
                    show_hotkeys,
                    show_display,
                    show_resolution,
                    show_anti_aliasing,
                    show_ui_scale,
                    mark_chosen,
                    show_border,
                    show_volumes,
                    show_sound_switches,
                )
                    .chain()
                    .run_if(any_with_component::<Page>),
            );
    }
}

fn screen(mut commands: Commands, options: Res<Options>) {
    commands.spawn((Camera2d, DespawnOnExit(Screen::Options)));
    let page = page(&mut commands, &options);
    commands.entity(page).insert(DespawnOnExit(Screen::Options));
}

/// Spawns the options over the whole screen, on top of whatever's there,
/// until their button to leave them is pressed.
pub fn page(commands: &mut Commands, options: &Options) -> Entity {
    commands.insert_resource(Open::Nothing);
    commands.insert_resource(Tab::Game);
    let page = Node {
        position_type: PositionType::Absolute,
        width: percent(100),
        height: percent(100),
        ..default()
    };
    let page = commands
        .spawn((
            Page,
            page,
            Surface::Solid,
            GlobalZIndex(layer::PAGE),
            crate::ui::Covers,
        ))
        .id();
    let main = Node {
        flex_grow: 1.0,
        flex_direction: FlexDirection::Column,
        padding: UiRect::all(px(32)),
        row_gap: px(24),
        ..default()
    };
    let main = commands.spawn((ChildOf(page), main)).id();
    commands.spawn((text("Options", Font::Title), ChildOf(main)));
    let body = Node {
        flex_direction: FlexDirection::Column,
        row_gap: px(24),
        ..default()
    };
    commands.spawn((Body, ChildOf(main), body));
    // The tabs down the right, as the menus have their buttons.
    let side = commands.spawn((ChildOf(page), menu::side())).id();
    for (tab, name) in [
        (Tab::Game, &options.game_tab),
        (Tab::Video, &options.video_tab),
        (Tab::Audio, &options.audio_tab),
    ] {
        commands
            .spawn((
                TabButton(tab),
                toggle(name, Font::Large),
                called(match tab {
                    Tab::Game => "options.tab.game",
                    Tab::Video => "options.tab.video",
                    Tab::Audio => "options.tab.audio",
                }),
                ChildOf(side),
            ))
            .observe(move |_: On<Activate>, mut shown: ResMut<Tab>| {
                shown.set_if_neq(tab);
            });
    }
    commands.spawn((menu::spacer(), ChildOf(side)));
    commands
        .spawn((
            button(&options.ok, Font::Large),
            called("options.ok"),
            ChildOf(side),
        ))
        .observe(leave);
    page
}

/// Leaves the options: for the main menu from its own screen, else for
/// what they were spawned over.
fn leave(
    _: On<Activate>,
    mut commands: Commands,
    screen: Res<State<Screen>>,
    mut next: ResMut<NextState<Screen>>,
    pages: Query<Entity, With<Page>>,
) {
    match screen.get() {
        Screen::Options => next.set(Screen::MainMenu),
        _ => pages.iter().for_each(|p| commands.entity(p).despawn()),
    }
}

/// How wide the names of options are, the gap after them, and the lists
/// of choices.
const NAME_WIDTH: f32 = 160.0;
const NAME_GAP: f32 = 12.0;
const LIST_WIDTH: f32 = 240.0;

/// A group of options under a `title`, for a [`Body`].
fn section(commands: &mut Commands, body: Entity, title: &str) -> Entity {
    let section = Node {
        flex_direction: FlexDirection::Column,
        row_gap: px(8),
        ..default()
    };
    let section = commands.spawn((ChildOf(body), section)).id();
    commands.spawn((text(title, Font::Heading), ChildOf(section)));
    section
}

/// An option's `name` in a `section`, beside whatever's put in the place
/// marked `holder` to choose it by.
fn choice(commands: &mut Commands, section: Entity, name: &str, holder: impl Bundle) {
    let row = Node {
        align_items: AlignItems::Center,
        column_gap: px(NAME_GAP),
        ..default()
    };
    let name = (
        Node {
            width: px(NAME_WIDTH),
            ..default()
        },
        children![label(name, Font::Body, Ink::Muted)],
    );
    commands.spawn((
        ChildOf(section),
        row,
        children![name, (holder, Node::default())],
    ));
}

/// Shows the options of the tab chosen.
fn show_body(
    mut commands: Commands,
    options: Res<Options>,
    volumes: Res<Volumes>,
    tab: Res<Tab>,
    body: Single<(Entity, Ref<Body>)>,
) {
    let (body, added) = *body;
    if !tab.is_changed() && !added.is_added() {
        return;
    }
    commands.entity(body).despawn_children();
    commands.insert_resource(Open::Nothing);
    match *tab {
        Tab::Game => {
            let game = section(&mut commands, body, &options.game_title);
            choice(&mut commands, game, &options.hotkeys_label, Hotkeys);
            let extras = section(&mut commands, body, &options.extras_title);
            commands
                .spawn((
                    BorderToggle,
                    ChildOf(extras),
                    switched(),
                    called("options.supply-border"),
                ))
                .observe(|_: On<Activate>, mut border: ResMut<Border>| border.0 = !border.0);
        }
        Tab::Video => {
            let basic = section(&mut commands, body, &options.basic_title);
            choice(&mut commands, basic, &options.display_label, DisplayHolder);
            choice(&mut commands, basic, &options.resolution_label, Resolution);
            let aa = &options.anti_aliasing_label;
            choice(&mut commands, basic, aa, AntiAliasingHolder);
            let scale = &options.ui_scale_label;
            choice(&mut commands, basic, scale, UiScaleHolder);
        }
        Tab::Audio => {
            let volume = section(&mut commands, body, &options.volume_title);
            for (channel, name) in Channel::ALL.into_iter().zip(&options.volume_labels) {
                let row = Node {
                    width: px(LIST_WIDTH),
                    align_items: AlignItems::Center,
                    column_gap: px(NAME_GAP),
                    ..default()
                };
                let level = Node {
                    width: px(SLIDER_WIDTH),
                    ..default()
                };
                let holder = children![(
                    row,
                    children![
                        (VolumeSlider(channel), slider(level, volumes.get(channel))),
                        (VolumeLevel(channel), text("", Font::Body)),
                    ],
                )];
                choice(&mut commands, volume, name, holder);
            }
            let switches = section(&mut commands, body, &options.sounds_title);
            for sound in [Sound::Chat, Sound::Replies] {
                commands
                    .spawn((
                        SoundToggle(sound),
                        ChildOf(switches),
                        switched(),
                        called(match sound {
                            Sound::Chat => "options.sound.chat",
                            Sound::Replies => "options.sound.replies",
                        }),
                    ))
                    .observe(move |_: On<Activate>, mut sounds: ResMut<Sounds>| {
                        let on = sound.on(&mut sounds.switches);
                        *on = !*on;
                    });
            }
        }
    }
}

/// A button switching something on or off, as wide as an option's name
/// and its list beside it, saying which it is once [`say_switched`].
fn switched() -> impl Bundle {
    let width = Node {
        width: px(NAME_WIDTH + NAME_GAP + LIST_WIDTH),
        justify_content: JustifyContent::Start,
        ..padded()
    };
    (switch(card(width)), Card, children![text("", Font::Body)])
}

/// Has a [`switched`] button say what it switches and whether it's on.
fn say_switched(children: &Children, texts: &mut Query<&mut Text>, name: &str, on: bool) {
    let state = match on {
        true => "On",
        false => "Off",
    };
    let mut texts = texts.iter_many_mut(children);
    if let Some(Ok(mut text)) = texts.fetch_next() {
        text.0 = format!("{name}: {state}");
    }
}

/// Says whether each sound switch is on.
fn show_sound_switches(
    options: Res<Options>,
    sounds: Res<Sounds>,
    toggles: Query<(&Children, Ref<SoundToggle>)>,
    mut texts: Query<&mut Text>,
) {
    for (children, toggle) in &toggles {
        if sounds.is_changed() || toggle.is_added() {
            let on = *toggle.0.on(&mut sounds.switches.clone());
            say_switched(children, &mut texts, options.sound_label(toggle.0), on);
        }
    }
}

/// How wide a volume's slider is, leaving room in its row for its level.
const SLIDER_WIDTH: f32 = 180.0;

/// Sets the volume a slider's moved to.
fn set_volume(
    change: On<ValueChange<f32>>,
    mut commands: Commands,
    sliders: Query<&VolumeSlider>,
    mut volumes: ResMut<Volumes>,
) {
    if let Ok(VolumeSlider(channel)) = sliders.get(change.source) {
        volumes.set(*channel, change.value);
        commands
            .entity(change.source)
            .insert(SliderValue(volumes.get(*channel)));
    }
}

/// Says what each volume is at.
fn show_volumes(volumes: Res<Volumes>, mut levels: Query<(&mut Text, Ref<VolumeLevel>)>) {
    for (mut text, level) in &mut levels {
        if volumes.is_changed() || level.is_added() {
            text.0 = format!("{:.0}%", volumes.get(level.0) * 100.0);
        }
    }
}

/// Opens a list of choices, or closes it if it's the one open.
fn open_list(activate: On<Activate>, openers: Query<&Opener>, mut open: ResMut<Open>) {
    if let Ok(Opener(list)) = openers.get(activate.entity) {
        *open = match *open == *list {
            true => Open::Nothing,
            false => *list,
        };
    }
}

/// Shows the hotkeys in use, and the others to choose from while picking.
fn show_hotkeys(
    mut commands: Commands,
    options: Res<Options>,
    open: Res<Open>,
    holder: Single<(Entity, Ref<Hotkeys>)>,
) {
    let (holder, added) = *holder;
    if !options.is_changed() && !open.is_changed() && !added.is_added() {
        return;
    }
    commands.entity(holder).despawn_children();
    let list = Dropdown {
        id: "options.hotkeys".into(),
        row: holder,
        width: px(LIST_WIDTH),
        label: options.hotkeys().map_or("", |c| c.name.as_str()).into(),
        open: *open == Open::Hotkeys,
    };
    let choices = options.configurations.iter();
    let choices = choices.map(|c| {
        let choice = (HotkeysChoice(c.id.clone()), InList);
        (slug(c.id.as_str()), choice, c.name.clone())
    });
    list.spawn(
        &mut commands,
        (Opener(Open::Hotkeys), InList),
        open_list,
        choices,
        choose_hotkeys,
    );
}

fn choose_hotkeys(
    activate: On<Activate>,
    choices: Query<&HotkeysChoice>,
    mut options: ResMut<Options>,
    mut open: ResMut<Open>,
) {
    if let Ok(choice) = choices.get(activate.entity) {
        options.hotkeys = Some(choice.0.clone());
        *open = Open::Nothing;
    }
}

/// Shows how the window takes the screen, and the other ways while
/// picking.
fn show_display(
    mut commands: Commands,
    open: Res<Open>,
    holder: Single<(Entity, Ref<DisplayHolder>)>,
    window: Query<(&Window, Option<&RawHandleWrapper>), With<PrimaryWindow>>,
    mut shown: Local<Option<Display>>,
) {
    let (holder, added) = *holder;
    let window = window.single().ok();
    let display = window.map(|(w, _)| Display::of(w.mode));
    if *shown == display && !open.is_changed() && !added.is_added() {
        return;
    }
    *shown = display;
    // Wayland has no exclusive fullscreen: asked for, it does nothing.
    let wayland = window
        .and_then(|(_, h)| h)
        .is_some_and(|h| matches!(h.get_display_handle(), RawDisplayHandle::Wayland(_)));
    let choices = Display::ALL.iter().copied();
    let choices = choices.filter(|d| !(wayland && *d == Display::Fullscreen));
    listed(
        &mut commands,
        holder,
        *open,
        display,
        choices,
        choose_display,
    );
}

fn choose_display(
    activate: On<Activate>,
    choices: Query<&Pick<Display>>,
    mut window: Query<&mut Window, With<PrimaryWindow>>,
    mut windowed: ResMut<WindowedSize>,
    mut open: ResMut<Open>,
) {
    let Ok(choice) = choices.get(activate.entity) else {
        return;
    };
    if let Ok(mut window) = window.single_mut() {
        // Taking the whole screen resizes the window to it, which not every
        // platform undoes on leaving.
        match (Display::of(window.mode), choice.0) {
            (Display::Windowed, Display::Windowed) => {}
            (Display::Windowed, _) => windowed.0 = Some(window.size()),
            (_, Display::Windowed) => {
                if let Some(size) = windowed.0 {
                    window.resolution.set(size.x, size.y);
                }
            }
            _ => {}
        }
        window.mode = choice.0.mode();
    }
    *open = Open::Nothing;
}

/// Keeps the window's mode to what it is when the platform changes it, as
/// on leaving fullscreen by the system's own shortcut, so the options say
/// what it is and choosing the mode it had again isn't taken as no change.
fn follow_window(
    mut resized: MessageReader<WindowResized>,
    mut window: Query<(Entity, &mut Window), With<PrimaryWindow>>,
    _winit: NonSendMarker,
) {
    let Ok((entity, mut window)) = window.single_mut() else {
        return;
    };
    if !resized.read().any(|r| r.window == entity) {
        return;
    }
    let full =
        WINIT_WINDOWS.with_borrow(|w| w.get_window(entity).map(|w| w.fullscreen().is_some()));
    let Some(full) = full else {
        return;
    };
    let windowed = Display::of(window.mode) == Display::Windowed;
    if full == windowed {
        let now = if full {
            Display::Borderless
        } else {
            Display::Windowed
        };
        window.mode = now.mode();
    }
}

/// A window's size as the options show it, e.g. `1280x720`.
fn size_of(window: &Window) -> UVec2 {
    window.size().round().as_uvec2()
}

/// Shows the window's size, and the others to choose from while picking:
/// those smaller than the screen, which leaves room for its frame.
fn show_resolution(
    mut commands: Commands,
    open: Res<Open>,
    holder: Single<(Entity, Ref<Resolution>)>,
    window: Query<(&Window, Option<&OnMonitor>), With<PrimaryWindow>>,
    monitors: Query<&Monitor>,
    primary: Query<&Monitor, With<PrimaryMonitor>>,
    mut shown: Local<Option<UVec2>>,
) {
    let (holder, added) = *holder;
    let window = window.single().ok();
    let size = window.map(|(w, _)| size_of(w));
    if *shown == size && !open.is_changed() && !added.is_added() {
        return;
    }
    *shown = size;
    commands.entity(holder).despawn_children();
    let list = Dropdown {
        id: "options.resolution".into(),
        row: holder,
        width: px(LIST_WIDTH),
        label: size
            .map_or(String::new(), |s| format!("{}x{}", s.x, s.y))
            .into(),
        open: *open == Open::Resolution,
    };
    // The screen the window's on, which the other modes take too.
    let on = window
        .and_then(|(_, on)| on)
        .and_then(|on| monitors.get(on.0).ok());
    let room = on.or(primary.single().ok()).map(|m| {
        let size = UVec2::new(m.physical_width, m.physical_height).as_dvec2();
        (size / m.scale_factor).as_uvec2()
    });
    let sizes = RESOLUTIONS.iter().map(|&(w, h)| UVec2::new(w, h));
    let sizes = sizes.filter(|s| room.is_none_or(|room| s.cmplt(room).all()));
    let choices = sizes.map(|s| {
        let size = format!("{}x{}", s.x, s.y);
        (size.clone(), (ResolutionChoice(s), InList), size)
    });
    list.spawn(
        &mut commands,
        (Opener(Open::Resolution), InList),
        open_list,
        choices,
        choose_resolution,
    );
}

fn choose_resolution(
    activate: On<Activate>,
    choices: Query<&ResolutionChoice>,
    mut window: Query<&mut Window, With<PrimaryWindow>>,
    mut open: ResMut<Open>,
) {
    let Ok(choice) = choices.get(activate.entity) else {
        return;
    };
    if let Ok(mut window) = window.single_mut() {
        // Neither a maximised window nor one that fills the screen takes a
        // size.
        window.mode = WindowMode::Windowed;
        window.set_maximized(false);
        let size = choice.0.as_vec2();
        window.resolution.set(size.x, size.y);
    }
    *open = Open::Nothing;
}

/// Shows the anti-aliasing in use, and the others while picking.
fn show_anti_aliasing(
    mut commands: Commands,
    chosen: Res<AntiAliasing>,
    open: Res<Open>,
    holder: Single<(Entity, Ref<AntiAliasingHolder>)>,
) {
    let (holder, added) = *holder;
    if !chosen.is_changed() && !open.is_changed() && !added.is_added() {
        return;
    }
    let choices = AntiAliasing::ALL.iter().copied();
    let current = Some(*chosen);
    listed(
        &mut commands,
        holder,
        *open,
        current,
        choices,
        choose_anti_aliasing,
    );
}

fn choose_anti_aliasing(
    activate: On<Activate>,
    choices: Query<&Pick<AntiAliasing>>,
    mut chosen: ResMut<AntiAliasing>,
    mut open: ResMut<Open>,
) {
    if let Ok(choice) = choices.get(activate.entity) {
        chosen.set_if_neq(choice.0);
        *open = Open::Nothing;
    }
}

/// Shows the interface's scale, and the others while picking.
fn show_ui_scale(
    mut commands: Commands,
    setting: Res<UiScaleSetting>,
    open: Res<Open>,
    holder: Single<(Entity, Ref<UiScaleHolder>)>,
) {
    let (holder, added) = *holder;
    if !setting.is_changed() && !open.is_changed() && !added.is_added() {
        return;
    }
    commands.entity(holder).despawn_children();
    let list = Dropdown {
        id: "options.ui-scale".into(),
        row: holder,
        width: px(LIST_WIDTH),
        label: percentage(setting.0).into(),
        open: *open == Open::UiScale,
    };
    let choices = UI_SCALE_CHOICES.iter().filter(|s| UI_SCALES.contains(s));
    let choices = choices.map(|&s| {
        let shown = percentage(s);
        (slug(&shown), (UiScaleChoice(s), InList), shown)
    });
    list.spawn(
        &mut commands,
        (Opener(Open::UiScale), InList),
        open_list,
        choices,
        choose_ui_scale,
    );
}

fn choose_ui_scale(
    activate: On<Activate>,
    choices: Query<&UiScaleChoice>,
    mut setting: ResMut<UiScaleSetting>,
    mut open: ResMut<Open>,
) {
    if let Ok(choice) = choices.get(activate.entity) {
        if setting.0 != choice.0 {
            setting.0 = choice.0;
        }
        *open = Open::Nothing;
    }
}

/// Gives the map's camera the anti-aliasing chosen, as it's chosen and as
/// each match spawns its own. The post-process kinds go without MSAA,
/// which they'd only add their cost to.
fn anti_alias(
    mut commands: Commands,
    chosen: Res<AntiAliasing>,
    cameras: Query<(Entity, Ref<RtsCamera>)>,
) {
    for (camera, added) in &cameras {
        if !chosen.is_changed() && !added.is_added() {
            continue;
        }
        let mut camera = commands.entity(camera);
        camera.remove::<(Fxaa, Smaa)>();
        match *chosen {
            AntiAliasing::Off => camera.insert(Msaa::Off),
            AntiAliasing::Msaa => camera.insert(Msaa::Sample4),
            AntiAliasing::Fxaa => camera.insert((Msaa::Off, Fxaa::default())),
            AntiAliasing::Smaa => camera.insert((Msaa::Off, Smaa::default())),
        };
    }
}

/// A button and what it chooses.
type Choice<'a> = (
    &'a mut Chosen,
    Option<&'a TabButton>,
    Option<&'a HotkeysChoice>,
    Option<&'a Pick<Display>>,
    Option<&'a ResolutionChoice>,
    Option<&'a Pick<AntiAliasing>>,
    Option<&'a UiScaleChoice>,
);

/// The buttons the options choose with.
type Choosing = Or<(
    With<TabButton>,
    With<HotkeysChoice>,
    With<Pick<Display>>,
    With<ResolutionChoice>,
    With<Pick<AntiAliasing>>,
    With<UiScaleChoice>,
)>;

fn mark_chosen(
    options: Res<Options>,
    tab: Res<Tab>,
    anti_aliasing: Res<AntiAliasing>,
    ui_scale: Res<UiScaleSetting>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut buttons: Query<Choice, Choosing>,
) {
    let window = window.single().ok();
    let size = window.map(size_of);
    let display = window.map(|w| Display::of(w.mode));
    for (mut chosen, shown, hotkeys, mode, resolution, aa, scale) in &mut buttons {
        let now = shown.is_some_and(|t| t.0 == *tab)
            || hotkeys.is_some_and(|h| options.hotkeys.as_ref() == Some(&h.0))
            || mode.is_some_and(|m| display == Some(m.0))
            || resolution.is_some_and(|r| size == Some(r.0))
            || aa.is_some_and(|a| *anti_aliasing == a.0)
            || scale.is_some_and(|s| ui_scale.0 == s.0);
        chosen.set_if_neq(Chosen(now));
    }
}

/// Closes whichever list is open on a press anywhere but on it.
fn close_list(
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    mut open: ResMut<Open>,
    list: Query<&Hovered, With<InList>>,
) {
    let pressed = mouse.is_some_and(|m| m.just_pressed(MouseButton::Left));
    if pressed && !list.iter().any(|h| h.get()) {
        open.set_if_neq(Open::Nothing);
    }
}

/// Says whether the supply border is showing.
fn show_border(
    options: Res<Options>,
    border: Res<Border>,
    toggles: Query<(&Children, Ref<BorderToggle>)>,
    mut texts: Query<&mut Text>,
) {
    for (children, added) in &toggles {
        if border.is_changed() || added.is_added() {
            say_switched(children, &mut texts, &options.border_label, border.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::state::app::StatesPlugin;

    use super::*;

    fn configuration(id: &str, name: &str) -> HotkeyConfiguration {
        HotkeyConfiguration {
            id: id.into(),
            name: name.into(),
            file: None,
        }
    }

    fn options(named: Option<&str>) -> Options {
        let configurations = vec![
            configuration("classic", "Standard"),
            configuration("visual", "Visual"),
        ];
        Options::of(
            configurations,
            named.map(Id::from),
            &aaa_data::Strings::default(),
        )
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin, OptionsPlugin))
            .insert_state(Screen::Options)
            .insert_resource(options(Some("classic")))
            .insert_resource(Volumes(aaa_data::Volumes {
                main: 0.8,
                fx: 0.8,
                speech: 0.95,
                music: 0.7,
            }))
            .insert_resource(Sounds::of(Default::default(), &[]))
            .init_resource::<Border>()
            .insert_resource(UiScaleSetting(1.0))
            .add_message::<WindowResized>();
        app.update();
        app
    }

    fn activate<C: Component>(app: &mut App, which: impl Fn(&C) -> bool) {
        let world = app.world_mut();
        let mut buttons = world.query::<(Entity, &C)>();
        let found = buttons.iter(world).find(|(_, c)| which(c));
        let (entity, _) = found.expect("no such button");
        world.trigger(Activate { entity });
        app.update();
    }

    fn choices(app: &mut App) -> usize {
        let world = app.world_mut();
        world.query::<&HotkeysChoice>().iter(world).count()
    }

    #[test]
    fn starts_on_the_set_named_or_the_first() {
        let chosen = |named| options(named).hotkeys().map(|c| c.name.clone());
        assert_eq!(chosen(Some("visual")).as_deref(), Some("Visual"));
        assert_eq!(chosen(Some("missing")).as_deref(), Some("Standard"));
        assert_eq!(chosen(None).as_deref(), Some("Standard"));
        let none = Options::of(
            Vec::new(),
            Some("classic".into()),
            &aaa_data::Strings::default(),
        );
        assert!(none.hotkeys().is_none());
    }

    #[test]
    fn the_list_opens_and_a_choice_from_it_is_kept() {
        let mut app = app();
        assert_eq!(choices(&mut app), 0);
        activate::<Opener>(&mut app, |o| o.0 == Open::Hotkeys);
        assert_eq!(choices(&mut app), 2);
        activate::<HotkeysChoice>(&mut app, |c| c.0.as_str() == "visual");
        assert_eq!(choices(&mut app), 0);
        let options = app.world().resource::<Options>();
        assert_eq!(options.hotkeys().map(|c| c.id.as_str()), Some("visual"));
    }

    #[test]
    fn a_resolution_chosen_sizes_the_window() {
        let mut app = app();
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        activate::<TabButton>(&mut app, |t| t.0 == Tab::Video);
        activate::<Opener>(&mut app, |o| o.0 == Open::Resolution);
        activate::<ResolutionChoice>(&mut app, |r| r.0 == UVec2::new(1600, 900));
        let world = app.world_mut();
        let window = world.query::<&Window>().single(world).unwrap();
        assert_eq!(window.size(), Vec2::new(1600.0, 900.0));
        assert_eq!(*world.resource::<Open>(), Open::Nothing);
    }

    #[test]
    fn a_display_chosen_sets_the_window_mode_and_a_resolution_unsets_it() {
        let mut app = app();
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        activate::<TabButton>(&mut app, |t| t.0 == Tab::Video);
        activate::<Opener>(&mut app, |o| o.0 == Open::Display);
        activate::<Pick<Display>>(&mut app, |d| d.0 == Display::Borderless);
        let mode = |app: &mut App| {
            let world = app.world_mut();
            Display::of(world.query::<&Window>().single(world).unwrap().mode)
        };
        assert_eq!(mode(&mut app), Display::Borderless);
        activate::<Opener>(&mut app, |o| o.0 == Open::Resolution);
        activate::<ResolutionChoice>(&mut app, |r| r.0 == UVec2::new(1600, 900));
        assert_eq!(mode(&mut app), Display::Windowed);
    }

    #[test]
    fn each_display_is_the_mode_it_sets() {
        for &display in Display::ALL {
            assert_eq!(Display::of(display.mode()), display);
        }
    }

    #[test]
    fn the_display_and_anti_aliasing_chosen_are_marked() {
        let mut app = app();
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        activate::<TabButton>(&mut app, |t| t.0 == Tab::Video);
        activate::<Opener>(&mut app, |o| o.0 == Open::Display);
        activate::<Pick<Display>>(&mut app, |d| d.0 == Display::Fullscreen);
        activate::<Opener>(&mut app, |o| o.0 == Open::Display);
        app.update();
        assert_eq!(marked::<Display>(&mut app), vec![Display::Fullscreen]);
        activate::<Opener>(&mut app, |o| o.0 == Open::AntiAliasing);
        activate::<Pick<AntiAliasing>>(&mut app, |a| a.0 == AntiAliasing::Fxaa);
        activate::<Opener>(&mut app, |o| o.0 == Open::AntiAliasing);
        app.update();
        assert_eq!(marked::<AntiAliasing>(&mut app), vec![AntiAliasing::Fxaa]);
    }

    #[test]
    fn a_ui_scale_chosen_is_set_and_marked() {
        let mut app = app();
        activate::<TabButton>(&mut app, |t| t.0 == Tab::Video);
        activate::<Opener>(&mut app, |o| o.0 == Open::UiScale);
        activate::<UiScaleChoice>(&mut app, |s| s.0 == 1.5);
        assert_eq!(app.world().resource::<UiScaleSetting>().0, 1.5);
        assert_eq!(*app.world().resource::<Open>(), Open::Nothing);
        activate::<Opener>(&mut app, |o| o.0 == Open::UiScale);
        app.update();
        let world = app.world_mut();
        let mut choices = world.query::<(&UiScaleChoice, &Chosen)>();
        let marked: Vec<_> = choices
            .iter(world)
            .filter(|(_, c)| c.0)
            .map(|(s, _)| s.0)
            .collect();
        assert_eq!(marked, vec![1.5]);
    }

    fn marked<T: Listed>(app: &mut App) -> Vec<T> {
        let world = app.world_mut();
        let mut picks = world.query::<(&Pick<T>, &Chosen)>();
        picks
            .iter(world)
            .filter(|(_, c)| c.0)
            .map(|(p, _)| p.0)
            .collect()
    }

    #[test]
    fn anti_aliasing_chosen_reaches_cameras_now_and_later() {
        let mut app = app();
        let camera = || (Camera3d::default(), RtsCamera::new(Vec3::ZERO));
        let first = app.world_mut().spawn(camera()).id();
        app.update();
        assert_eq!(app.world().get::<Msaa>(first), Some(&Msaa::Sample4));
        activate::<TabButton>(&mut app, |t| t.0 == Tab::Video);
        activate::<Opener>(&mut app, |o| o.0 == Open::AntiAliasing);
        activate::<Pick<AntiAliasing>>(&mut app, |a| a.0 == AntiAliasing::Smaa);
        let world = app.world();
        assert_eq!(world.get::<Msaa>(first), Some(&Msaa::Off));
        assert!(world.get::<Smaa>(first).is_some());
        activate::<Opener>(&mut app, |o| o.0 == Open::AntiAliasing);
        activate::<Pick<AntiAliasing>>(&mut app, |a| a.0 == AntiAliasing::Fxaa);
        let later = app.world_mut().spawn(camera()).id();
        app.update();
        for camera in [first, later] {
            let world = app.world();
            assert!(world.get::<Smaa>(camera).is_none());
            assert!(world.get::<Fxaa>(camera).is_some());
        }
    }

    #[test]
    fn a_volume_slider_moved_sets_its_volume_and_says_it() {
        let mut app = app();
        activate::<TabButton>(&mut app, |t| t.0 == Tab::Audio);
        let world = app.world_mut();
        let mut sliders = world.query::<(Entity, &VolumeSlider, &SliderValue)>();
        let found = sliders.iter(world).find(|(_, s, _)| s.0 == Channel::Music);
        let (source, _, value) = found.expect("no music slider");
        assert_eq!(value.0, 0.7);
        world.trigger(ValueChange {
            source,
            value: 0.25f32,
            is_final: true,
        });
        app.update();
        let world = app.world_mut();
        assert_eq!(world.resource::<Volumes>().get(Channel::Music), 0.25);
        assert_eq!(world.get::<SliderValue>(source), Some(&SliderValue(0.25)));
        let mut levels = world.query::<(&VolumeLevel, &Text)>();
        let said: Vec<_> = levels
            .iter(world)
            .map(|(l, t)| (l.0, t.0.clone()))
            .collect();
        assert!(said.contains(&(Channel::Music, "25%".into())));
        assert!(said.contains(&(Channel::Speech, "95%".into())));
    }

    #[test]
    fn a_sound_switch_turns_its_sound_off_and_says_so() {
        let mut app = app();
        activate::<TabButton>(&mut app, |t| t.0 == Tab::Audio);
        let said = |app: &mut App, sound: Sound| {
            let world = app.world_mut();
            let mut toggles = world.query::<(&SoundToggle, &Children)>();
            let (_, children) = toggles.iter(world).find(|(t, _)| t.0 == sound).unwrap();
            world.get::<Text>(children[0]).unwrap().0.clone()
        };
        assert_eq!(said(&mut app, Sound::Replies), "Unit Acknowledgement: On");
        activate::<SoundToggle>(&mut app, |t| t.0 == Sound::Replies);
        assert_eq!(said(&mut app, Sound::Replies), "Unit Acknowledgement: Off");
        assert_eq!(said(&mut app, Sound::Chat), "Chat Notification: On");
        let switches = app.world().resource::<Sounds>().switches;
        assert!(!switches.replies && switches.chat);
    }

    #[test]
    fn the_border_toggle_says_which_it_is() {
        let mut app = app();
        let said = |app: &mut App| {
            let world = app.world_mut();
            let mut toggles = world.query_filtered::<&Children, With<BorderToggle>>();
            let child = toggles.single(world).unwrap()[0];
            world.get::<Text>(child).unwrap().0.clone()
        };
        let before = said(&mut app);
        activate::<BorderToggle>(&mut app, |_| true);
        assert_ne!(said(&mut app), before);
        assert!(said(&mut app).starts_with("Supply Border: "));
    }
}
