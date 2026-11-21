// SPDX-License-Identifier: GPL-3.0-only

use crate::{
    config::{
        APPLET_CONFIG_ID, BarCorners, CornersConfig, DEFAULT_RADIUS, PILL, SLIDER_MARKS,
        SLIDER_MAX, THEME_CONFIG_IDS, Target, ThemeCorners, from_slider, is_pill, reapply,
        set_token, slider_pos, slider_top, token,
    },
    fl,
    number_box::{self, NumberBox, number_box},
};
use cosmic::{
    Element, Task,
    app::{self, Core},
    applet::{menu_button, padded_control},
    cosmic_config::{Config, ConfigGet, ConfigSet, CosmicConfigEntry},
    cosmic_theme::{CornerRadii, Spacing},
    iced::widget::{column, row},
    iced::widget::slider::{Catalog as _, Status as SliderStatus, Style as SliderStyle},
    iced::{
        Alignment, Background, Length, Limits, Subscription,
        platform_specific::shell::wayland::commands::popup::destroy_popup, window::Id,
    },
    theme,
    widget::{button, container, divider, icon, scrollable, slider, text, toggler},
};
use std::thread;
use tracing::error;

const ID: &str = "com.studio72f.OrbitCorners";
const ICON: &str = "com.studio72f.OrbitCorners-symbolic";
const ISSUES_URL: &str = "https://github.com/72F-Studio/orbit-comp/issues/new";

/// A user-visible failure. Each stage has its own stable code.
#[derive(Clone, Debug)]
struct Failure {
    code: &'static str,
    detail: String,
}

impl Failure {
    fn new(code: &'static str, source: Option<&str>, detail: impl std::fmt::Display) -> Self {
        let source = source.map_or(String::new(), |s| format!("{s}: "));
        Self {
            code,
            detail: redact(&format!("{source}{detail}")),
        }
    }

    fn texts(&self) -> (String, String, String) {
        match self.code {
            "ORB-C01" => (fl!("c01-headline"), fl!("c01-description"), fl!("c01-why")),
            "ORB-C02" => (fl!("c02-headline"), fl!("c02-description"), fl!("c02-why")),
            "ORB-C03" => (fl!("c03-headline"), fl!("c03-description"), fl!("c03-why")),
            "ORB-C04" => (fl!("c04-headline"), fl!("c04-description"), fl!("c04-why")),
            "ORB-C05" => (fl!("c05-headline"), fl!("c05-description"), fl!("c05-why")),
            _ => (fl!("c06-headline"), fl!("c06-description"), fl!("c06-why")),
        }
    }
}

/// Keep the home folder (and with it the user name) out of anything shown
/// or reported.
fn redact(s: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() => s.replace(&home, "~"),
        _ => s.to_owned(),
    }
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The popup's collapsible categories, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Bars,
    Theme,
}

impl Section {
    const ALL: [Section; 2] = [Section::Bars, Section::Theme];

    /// Stable key stored in the applet's own config.
    fn key(self) -> &'static str {
        match self {
            Section::Bars => "bars",
            Section::Theme => "theme",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == key)
    }

    fn title(self) -> String {
        match self {
            Section::Bars => fl!("section-bars"),
            Section::Theme => fl!("section-theme"),
        }
    }

    fn description(self) -> String {
        match self {
            Section::Bars => fl!("section-bars-note"),
            Section::Theme => fl!("theme-note"),
        }
    }

    fn targets(self) -> &'static [Target] {
        match self {
            Section::Bars => &Target::BARS,
            Section::Theme => &Target::THEME,
        }
    }
}

const OPEN_SECTIONS_KEY: &str = "open_sections";

/// Tallest the popup gets. The list scrolls inside it.
const POPUP_MAX_HEIGHT: f32 = 640.0;
const POPUP_WIDTH: f32 = 400.0;
/// Height of the pinned footer (divider and settings link) plus the popup's
/// vertical padding, kept free below the scroll area.
const POPUP_FOOTER_RESERVE: f32 = 72.0;

#[derive(Default)]
struct Item {
    /// What the config holds now (or, mid-drag, what the slider shows).
    current: u32,
    /// What's typed in the number box, until Enter or focus leaves it.
    draft: Option<String>,
}

impl Item {
    fn new(current: u32) -> Self {
        Self {
            current,
            draft: None,
        }
    }
}

/// The standard slider, faded, for an item that follows Roundness. The same
/// look for every state, so hovering doesn't suggest it can be dragged.
fn faded_slider(theme: &cosmic::Theme) -> SliderStyle {
    let fade = |b: Background| match b {
        Background::Color(c) => Background::Color(c.scale_alpha(0.35)),
        other => other,
    };
    let mut style = theme.style(&cosmic::theme::style::iced::Slider::Standard, SliderStatus::Active);
    style.rail.backgrounds = (fade(style.rail.backgrounds.0), fade(style.rail.backgrounds.1));
    style.handle.background = fade(style.handle.background);
    style.breakpoint.color = style.breakpoint.color.scale_alpha(0.35);
    style
}

pub struct Corners {
    core: Core,
    popup: Option<Id>,
    items: [Item; 7],
    bar_helpers: [Option<Config>; 2],
    theme_helpers: Vec<(&'static str, Config)>,
    prefs: CornersConfig,
    prefs_helper: Option<Config>,
    open_sections: Vec<Section>,
    failure: Option<Failure>,
    details_open: bool,
}

#[derive(Clone, Debug)]
pub enum Message {
    TogglePopup,
    PopupClosed(Id),
    BarUpdate(Target, u32),
    ThemeUpdate(CornerRadii),
    MatchRoundness(Target, bool),
    /// Slider moved: only the shown value changes. Writing waits for
    /// `Release`, because a bar write makes the panel rebuild its applets
    /// (this one included, which ends the drag) and a theme write restyles
    /// every open app.
    Drag(Target, u32),
    Release(Target),
    /// Arrows and typed values: written at once.
    Set(Target, u32),
    /// A message that does nothing, for controls shown but not usable.
    Noop,
    Draft(Target, String),
    Commit(Target),
    ToggleSection(Section),
    ToggleDetails,
    Report,
    Dismiss,
    OpenSettings,
}

impl Corners {
    fn item(&self, target: Target) -> &Item {
        &self.items[target.index()]
    }

    fn item_mut(&mut self, target: Target) -> &mut Item {
        &mut self.items[target.index()]
    }

    fn note_value(&mut self, target: Target, value: u32) {
        self.item_mut(target).current = value;
    }

    /// Pin `value` and write it to wherever `target` lives.
    fn apply(&mut self, target: Target, value: u32) {
        self.save_pin(target, Some(value));
        self.note_value(target, value);
        if target.bar_config_id().is_some() {
            self.write_bar(target, value);
        } else {
            self.write_theme(&[(target, value)]);
        }
    }

    fn write_bar(&mut self, target: Target, value: u32) {
        let Some(helper) = self.bar_helpers[target.index()].as_ref() else {
            return;
        };
        if let Err(err) = helper.set("border_radius", value) {
            error!(?err, ?target, "Failed to write border_radius");
            self.fail(Failure::new("ORB-C03", target.bar_config_id(), err));
        }
    }

    /// One read-modify-write per theme config, however many tokens change.
    fn write_theme(&mut self, changes: &[(Target, u32)]) {
        let mut failure = None;
        for (id, helper) in &self.theme_helpers {
            let mut radii = match helper.get::<CornerRadii>("corner_radii") {
                Ok(r) => r,
                Err(err) => {
                    error!(?err, id, "Failed to read corner_radii");
                    failure.get_or_insert(Failure::new("ORB-C06", Some(id), err));
                    continue;
                }
            };
            let before = radii;
            for &(target, value) in changes {
                set_token(&mut radii, target, value);
            }
            if radii == before {
                continue;
            }
            if let Err(err) = helper.set("corner_radii", radii) {
                error!(?err, id, "Failed to write corner_radii");
                failure.get_or_insert(Failure::new("ORB-C06", Some(id), err));
            }
        }
        if let Some(f) = failure {
            self.fail(f);
        }
    }

    fn save_pin(&mut self, target: Target, value: Option<u32>) {
        if self.prefs.pin(target) == value {
            return;
        }
        self.prefs.set_pin(target, value);
        let Some(helper) = self.prefs_helper.as_ref() else {
            return;
        };
        if let Err(err) = self.prefs.write_entry(helper) {
            error!(?err, "Failed to save the applet's settings");
            self.fail(Failure::new("ORB-C04", Some(APPLET_CONFIG_ID), err));
        }
    }

    fn fail(&mut self, failure: Failure) {
        self.failure = Some(failure);
        self.details_open = false;
    }

    fn title(target: Target) -> String {
        match target {
            Target::Dock => fl!("dock"),
            Target::Panel => fl!("panel"),
            Target::Buttons => fl!("buttons"),
            Target::Popups => fl!("popups"),
            Target::Windows => fl!("windows"),
            Target::AppletPopups => fl!("applet-popups"),
            Target::Cards => fl!("cards"),
        }
    }

    /// One control, laid out the same whether it follows Roundness or not so
    /// nothing jumps when the switch flips. Following, the slider and number
    /// box show the followed value, faded and inert.
    fn row_for(&self, target: Target) -> Element<'_, Message> {
        let space_xxxs = theme::active().cosmic().spacing.space_xxxs;
        let item = self.item(target);
        let own = self.prefs.pin(target).is_some();
        let value = item.current;
        let top = slider_top(target);
        let pos = slider_pos(value).min(top);

        let header = row!(
            column![
                text::body(Self::title(target)),
                text::caption(if own {
                    fl!("own-value")
                } else {
                    fl!("follows-roundness")
                }),
            ]
            .width(Length::Fill),
            toggler(own).on_toggle(move |on| Message::MatchRoundness(target, !on)),
        )
        .align_y(Alignment::Center);

        let typed = item.draft.clone().unwrap_or_else(|| {
            if is_pill(value) {
                fl!("pill")
            } else {
                value.to_string()
            }
        });
        let up = from_slider(target, (pos + 1).min(top));
        let down = from_slider(target, pos.saturating_sub(1));

        let mut bar = slider(0..=top, pos, move |p| {
            Message::Drag(target, from_slider(target, p))
        })
        .step(1u32)
        .breakpoints(&SLIDER_MARKS)
        .width(Length::Fill);
        if own {
            bar = bar.on_release(Message::Release(target));
        } else {
            bar = slider(0..=top, pos, |_| Message::Noop)
                .step(1u32)
                .breakpoints(&SLIDER_MARKS)
                .width(Length::Fill)
                .class(cosmic::theme::style::iced::Slider::Custom {
                    active: std::rc::Rc::new(faded_slider),
                    hovered: std::rc::Rc::new(faded_slider),
                    dragging: std::rc::Rc::new(faded_slider),
                });
        }

        let numbers = number_box(
            typed,
            NumberBox {
                on_input: own.then(|| {
                    Box::new(move |s| Message::Draft(target, s)) as Box<dyn Fn(String) -> Message>
                }),
                on_commit: Message::Commit(target),
                on_up: (own && up != value).then_some(Message::Set(target, up)),
                on_down: (own && down != value).then_some(Message::Set(target, down)),
            },
        );

        padded_control(
            column![
                header,
                row!(bar, numbers).spacing(12).align_y(Alignment::Center),
            ]
            .spacing(space_xxxs),
        )
        .into()
    }

    /// One collapsible category, like the Window Layout applet's: a heading
    /// row toggles it and the body shows only while it is open.
    fn section(&self, section: Section) -> Element<'_, Message> {
        let open = self.open_sections.contains(&section);
        let chevron = if open {
            "go-up-symbolic"
        } else {
            "go-down-symbolic"
        };
        let header = menu_button(
            row!(
                text::heading(section.title()).width(Length::Fill),
                icon::from_name(chevron).size(16).icon(),
            )
            .align_y(Alignment::Center),
        )
        .on_press(Message::ToggleSection(section));
        if !open {
            return header.into();
        }
        let mut body = column![header, padded_control(text::caption(section.description()))];
        for &target in section.targets() {
            body = body.push(self.row_for(target));
        }
        body.into()
    }

    /// Remember the open sections between popup openings and restarts.
    fn save_open_sections(&self) {
        let Some(helper) = self.prefs_helper.clone() else {
            return;
        };
        let keys: Vec<String> = Section::ALL
            .into_iter()
            .filter(|s| self.open_sections.contains(s))
            .map(|s| s.key().to_string())
            .collect();
        thread::spawn(move || {
            if let Err(err) = helper.set(OPEN_SECTIONS_KEY, keys) {
                error!(?err, "Failed to save open sections");
            }
        });
    }

    fn failure_section(&self, space_xxxs: u16) -> Option<Element<'_, Message>> {
        let failure = self.failure.as_ref()?;
        let (headline, description, why) = failure.texts();
        let (chevron, toggle_label) = if self.details_open {
            ("go-up-symbolic", fl!("view-less"))
        } else {
            ("go-down-symbolic", fl!("view-more"))
        };
        let mut section = column![
            padded_control(
                column![text::heading(headline), text::caption(description)].spacing(space_xxxs)
            ),
            menu_button(row!(
                text::body(toggle_label).width(Length::Fill),
                icon::from_name(chevron).size(16).icon(),
            ))
            .on_press(Message::ToggleDetails),
        ];
        if self.details_open {
            section = section.push(padded_control(
                column![
                    text::caption(why),
                    text::caption(fl!("error-code", code = failure.code)),
                    text::caption(failure.detail.clone()),
                ]
                .spacing(space_xxxs),
            ));
        }
        section = section.push(padded_control(
            column![
                row!(
                    button::standard(fl!("report")).on_press(Message::Report),
                    button::text(fl!("dismiss")).on_press(Message::Dismiss),
                )
                .spacing(8)
                .align_y(Alignment::Center),
                text::caption(fl!("report-note")),
            ]
            .spacing(space_xxxs),
        ));
        Some(section.into())
    }
}

impl cosmic::Application for Corners {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, app::Task<Self::Message>) {
        let mut failure = None;
        let mut items: [Item; 7] = Default::default();

        let bar_helpers = Target::BARS.map(|target| {
            let id = target.bar_config_id().unwrap();
            let helper = match Config::new(id, BarCorners::VERSION) {
                Ok(h) => h,
                Err(err) => {
                    error!(?err, id, "Failed to open bar config");
                    failure.get_or_insert(Failure::new("ORB-C01", Some(id), err));
                    return None;
                }
            };
            let current = BarCorners::get_entry(&helper).unwrap_or_else(|(errs, c)| {
                error!(?errs, id, "Failed to read bar config");
                if let Some(first) = errs.first() {
                    failure.get_or_insert(Failure::new("ORB-C02", Some(id), first));
                }
                c
            });
            items[target.index()] = Item::new(current.border_radius);
            Some(helper)
        });

        let mut theme_helpers = Vec::new();
        for id in THEME_CONFIG_IDS {
            match Config::new(id, ThemeCorners::VERSION) {
                Ok(h) => theme_helpers.push((id, h)),
                Err(err) => {
                    error!(?err, id, "Failed to open theme config");
                    failure.get_or_insert(Failure::new("ORB-C05", Some(id), err));
                }
            }
        }
        // Show the active theme's values; both normally match.
        let radii = cosmic::theme::active().cosmic().corner_radii;
        for target in Target::THEME {
            items[target.index()] = Item::new(token(&radii, target).unwrap_or(DEFAULT_RADIUS));
        }

        let prefs_helper = match Config::new(APPLET_CONFIG_ID, CornersConfig::VERSION) {
            Ok(h) => Some(h),
            Err(err) => {
                error!(?err, "Failed to open the applet's settings");
                failure.get_or_insert(Failure::new("ORB-C04", Some(APPLET_CONFIG_ID), err));
                None
            }
        };
        // A first run has no saved settings yet; that's not a failure.
        let prefs = prefs_helper.as_ref().map_or_else(CornersConfig::default, |h| {
            CornersConfig::get_entry(h).unwrap_or_else(|(_, c)| c)
        });

        // The applet's own config remembers which sections are open.
        let open_sections = prefs_helper
            .as_ref()
            .and_then(|h| h.get::<Vec<String>>(OPEN_SECTIONS_KEY).ok())
            .map(|keys| keys.iter().filter_map(|k| Section::from_key(k)).collect())
            .unwrap_or_else(|| vec![Section::Bars]);

        let mut app = Self {
            core,
            popup: None,
            items,
            bar_helpers,
            theme_helpers,
            prefs,
            prefs_helper,
            open_sections,
            failure,
            details_open: false,
        };

        // Put pinned values back in case Roundness changed while logged out.
        for target in Target::BARS {
            if let Some(v) = reapply(app.prefs.pin(target), app.item(target).current) {
                app.note_value(target, v);
                app.write_bar(target, v);
            }
        }
        let theme_changes: Vec<_> = Target::THEME
            .into_iter()
            .filter_map(|t| app.prefs.pin(t).map(|v| (t, v)))
            .collect();
        for &(t, v) in &theme_changes {
            app.note_value(t, v);
        }
        // write_theme skips configs that already match.
        app.write_theme(&theme_changes);

        (app, Task::none())
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        // Subscription::map can't capture, so each source gets its own closure.
        Subscription::batch([
            self.core
                .watch_config::<BarCorners>("com.system76.CosmicPanel.Dock")
                .map(|u| Message::BarUpdate(Target::Dock, u.config.border_radius)),
            self.core
                .watch_config::<BarCorners>("com.system76.CosmicPanel.Panel")
                .map(|u| Message::BarUpdate(Target::Panel, u.config.border_radius)),
            self.core
                .watch_config::<ThemeCorners>(cosmic::cosmic_theme::DARK_THEME_ID)
                .map(|u| Message::ThemeUpdate(u.config.corner_radii)),
            self.core
                .watch_config::<ThemeCorners>(cosmic::cosmic_theme::LIGHT_THEME_ID)
                .map(|u| Message::ThemeUpdate(u.config.corner_radii)),
        ])
    }

    fn update(&mut self, message: Self::Message) -> app::Task<Self::Message> {
        match message {
            Message::TogglePopup => {
                return if let Some(p) = self.popup.take() {
                    destroy_popup(p)
                } else {
                    cosmic::surface::surface_task(cosmic::surface::action::app_popup(
                        |_| Default::default(),
                        |app: &mut Self| {
                            let new_id = Id::unique();
                            app.popup = Some(new_id);
                            let mut settings = app.core.applet.get_popup_settings(
                                app.core.main_window_id().unwrap(),
                                new_id,
                                None,
                                None,
                                None,
                            );
                            settings.positioner.size_limits = popup_limits();
                            settings
                        },
                        None,
                    ))
                };
            }
            Message::PopupClosed(id) => {
                if self.popup.as_ref() == Some(&id) {
                    self.popup = None;
                }
            }
            Message::BarUpdate(target, value) => {
                self.note_value(target, value);
                // Settings rewrites both bars whenever Roundness changes.
                if let Some(v) = reapply(self.prefs.pin(target), value) {
                    self.note_value(target, v);
                    self.write_bar(target, v);
                }
            }
            Message::ThemeUpdate(radii) => {
                let mut changes = Vec::new();
                for target in Target::THEME {
                    let Some(value) = token(&radii, target) else {
                        continue;
                    };
                    match reapply(self.prefs.pin(target), value) {
                        Some(v) => changes.push((target, v)),
                        None => self.note_value(target, value),
                    }
                }
                if !changes.is_empty() {
                    for &(t, v) in &changes {
                        self.note_value(t, v);
                    }
                    self.write_theme(&changes);
                }
            }
            Message::MatchRoundness(target, on) => {
                // On: hand control back; the value stays until Roundness next
                // changes in Settings. Off: pin what's there now.
                let pin = (!on).then_some(self.item(target).current);
                self.save_pin(target, pin);
            }
            Message::Drag(target, value) => {
                let item = self.item_mut(target);
                item.draft = None;
                item.current = value;
            }
            Message::Release(target) => {
                let value = self.item(target).current;
                self.apply(target, value);
            }
            Message::Set(target, value) => {
                self.item_mut(target).draft = None;
                self.apply(target, value);
            }
            Message::Noop => {}
            Message::Draft(target, draft) => {
                // Held until Enter or focus leaves: writing a bar rebuilds the
                // panel's applets, which would take focus out of the box.
                self.item_mut(target).draft = Some(draft);
            }
            Message::Commit(target) => {
                if let Some(draft) = self.item_mut(target).draft.take()
                    && let Some(value) = number_box::parse(
                        &draft,
                        0,
                        SLIDER_MAX,
                        target.allows_pill().then_some(PILL),
                    )
                    && value != self.item(target).current
                {
                    self.apply(target, value);
                }
            }
            Message::ToggleSection(section) => {
                if let Some(i) = self.open_sections.iter().position(|s| *s == section) {
                    self.open_sections.remove(i);
                } else {
                    self.open_sections.push(section);
                }
                self.save_open_sections();
            }
            Message::ToggleDetails => self.details_open = !self.details_open,
            Message::Dismiss => {
                self.failure = None;
                self.details_open = false;
            }
            Message::Report => {
                if let Some(f) = self.failure.as_ref() {
                    let title = format!("Corners applet problem ({})", f.code);
                    let body = format!(
                        "Error code: {}\nApplet: orbit-applet-corners {}\nDetail: {}\n\nWhat were you doing when this happened?\n",
                        f.code,
                        env!("CARGO_PKG_VERSION"),
                        f.detail,
                    );
                    let url = format!(
                        "{ISSUES_URL}?title={}&body={}",
                        percent_encode(&title),
                        percent_encode(&body)
                    );
                    if let Err(err) = std::process::Command::new("xdg-open").arg(url).spawn() {
                        error!(?err, "Failed to open the browser");
                    }
                }
            }
            Message::OpenSettings => {
                if let Err(err) = std::process::Command::new("cosmic-settings")
                    .arg("appearance")
                    .spawn()
                {
                    error!(?err, "Failed to open Settings");
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        self.core
            .applet
            .icon_button(ICON)
            .on_press_down(Message::TogglePopup)
            .into()
    }

    fn view_window(&self, _id: Id) -> Element<'_, Self::Message> {
        let Spacing {
            space_xxxs,
            space_xxs,
            space_s,
            ..
        } = theme::active().cosmic().spacing;
        let sep = || padded_control(divider::horizontal::default()).padding([space_xxs, space_s]);

        let mut list = column![];
        if let Some(failure) = self.failure_section(space_xxxs) {
            list = list.push(failure).push(sep());
        }
        for (index, section) in Section::ALL.into_iter().enumerate() {
            if index > 0 {
                list = list.push(sep());
            }
            list = list.push(self.section(section));
        }
        list = list.push(padded_control(text::caption(fl!("match-roundness-note"))));

        // The list scrolls inside a capped popup while the settings link stays
        // pinned below it.
        let content = column![
            container(scrollable(list).height(Length::Shrink))
                .max_height(POPUP_MAX_HEIGHT - POPUP_FOOTER_RESERVE),
            sep(),
            menu_button(text::body(fl!("appearance-settings"))).on_press(Message::OpenSettings),
        ]
        .padding([8, 0]);

        self.core
            .applet
            .popup_container(content)
            .limits(popup_limits())
            .into()
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}

fn popup_limits() -> Limits {
    Limits::NONE
        .min_width(POPUP_WIDTH)
        .max_width(POPUP_WIDTH)
        .min_height(1.0)
        .max_height(POPUP_MAX_HEIGHT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_links_are_encoded() {
        assert_eq!(percent_encode("a b&c=(1)"), "a%20b%26c%3D%281%29");
    }

    #[test]
    fn home_is_redacted() {
        if let Ok(home) = std::env::var("HOME")
            && !home.is_empty()
        {
            assert_eq!(redact(&format!("{home}/.config/x")), "~/.config/x");
        }
    }
}
