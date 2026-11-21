// Copyright 2023 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

use crate::{
    fl,
    layout::{LayoutTransition, WorkspaceLayoutMode, derive_layout_mode, plan_layout_transition},
    scrolling_settings::{WidthChoice, percent, snap_threshold, snap_width, width_choice},
    wayland::AppRequest,
    wayland_subscription,
    wayland_subscription::WorkspacesUpdate,
};
use cctk::sctk::reexports::calloop::channel::SyncSender;
use cosmic::{
    Element, Task,
    app::{self, Core},
    applet::{menu_button, padded_control},
    cosmic_config::{Config, ConfigSet, CosmicConfigEntry},
    cosmic_theme::Spacing,
    iced::widget::{column, row},
    iced::{
        Length, Limits, Subscription,
        platform_specific::shell::wayland::commands::popup::destroy_popup, window::Id,
    },
    theme,
    widget::{
        button, container, divider, icon,
        segmented_button::{self, Entity, SingleSelectModel},
        segmented_control, slider, text, toggler,
    },
};
use cosmic_comp_config::{
    CenteringPolicy, CosmicCompConfig, NewWindowPosition, ScrollingConfig, TileBehavior,
    TilingEngine,
};
use cosmic_protocols::workspace::v2::client::zcosmic_workspace_handle_v2::TilingState;
use std::thread;
use tracing::error;

#[cfg(not(feature = "parallel-test-install"))]
const ID: &str = "com.system76.CosmicAppletTiling";
#[cfg(feature = "parallel-test-install")]
const ID: &str = "com.system76.CosmicAppletWindowLayoutTest";

#[cfg(not(feature = "parallel-test-install"))]
const ON: &str = "com.system76.CosmicAppletTiling.On";
#[cfg(feature = "parallel-test-install")]
const ON: &str = "com.system76.CosmicAppletWindowLayoutTest.On";

#[cfg(not(feature = "parallel-test-install"))]
const OFF: &str = "com.system76.CosmicAppletTiling.Off";
#[cfg(feature = "parallel-test-install")]
const OFF: &str = "com.system76.CosmicAppletWindowLayoutTest.Off";

#[cfg(not(feature = "parallel-test-install"))]
const SCROLLING: &str = "com.system76.CosmicAppletTiling.Scrolling";
#[cfg(feature = "parallel-test-install")]
const SCROLLING: &str = "com.system76.CosmicAppletWindowLayoutTest.Scrolling";

#[derive(Clone, Copy)]
struct LayoutEntities {
    floating: Entity,
    tiling: Entity,
    scrolling: Entity,
}

impl LayoutEntities {
    fn mode(self, entity: Entity) -> Option<WorkspaceLayoutMode> {
        if entity == self.floating {
            Some(WorkspaceLayoutMode::Floating)
        } else if entity == self.tiling {
            Some(WorkspaceLayoutMode::Tiling)
        } else if entity == self.scrolling {
            Some(WorkspaceLayoutMode::Scrolling)
        } else {
            None
        }
    }

    fn entity(self, mode: WorkspaceLayoutMode) -> Entity {
        match mode {
            WorkspaceLayoutMode::Floating => self.floating,
            WorkspaceLayoutMode::Tiling => self.tiling,
            WorkspaceLayoutMode::Scrolling => self.scrolling,
        }
    }
}

pub struct Window {
    core: Core,
    popup: Option<Id>,
    config: CosmicCompConfig,
    config_helper: Config,
    current_workspace_layout_model: segmented_button::SingleSelectModel,
    current_workspace_layout_entities: LayoutEntities,
    new_workspace_behavior_model: segmented_button::SingleSelectModel,
    /// Orbit: Floating, Tiling and Scrolling choices for new workspaces.
    new_workspace_entities: LayoutEntities,
    /// Orbit: 25/50/75/90%/Custom choices for a new window's width.
    width_model: segmented_button::SingleSelectModel,
    width_entities: Vec<Entity>,
    width_custom_entity: Entity,
    /// Custom is picked but the stored width still matches a preset.
    width_custom_selected: bool,
    /// Orbit: centering policy and new-window position choices.
    centering_model: segmented_button::SingleSelectModel,
    centering_entities: Vec<(Entity, CenteringPolicy)>,
    position_model: segmented_button::SingleSelectModel,
    position_entities: Vec<(Entity, NewWindowPosition)>,
    /// Orbit: whether the Scrolling settings section is open.
    scrolling_expanded: bool,
    /// may not match the config value if behavior is per-workspace
    autotiled: bool,
    layout_change_pending: bool,
    workspace_tx: Option<SyncSender<AppRequest>>,
}

#[derive(Clone, Debug)]
pub enum Message {
    TogglePopup,
    PopupClosed(Id),
    CurrentWorkspaceLayout(Entity),
    LayoutChanged(Result<LayoutTransition, String>),
    ToggleActiveHint(bool),
    MyConfigUpdate(Box<CosmicCompConfig>),
    WorkspaceUpdate(WorkspacesUpdate),
    NewWorkspace(Entity),
    OpenSettings,
    // Orbit: Scrolling settings
    NewWindowWidth(Entity),
    CustomWidthChanged(f64),
    ToggleFocusFollowsScroll(bool),
    FocusThresholdChanged(f64),
    ToggleBackdropBlur(bool),
    Centering(Entity),
    NewWindowPosition(Entity),
    ToggleCenterSingle(bool),
    ToggleDimOffscreen(bool),
    TogglePositionIndicator(bool),
    ToggleScrollingSection,
    RemoveWidthRule(usize),
    SaveScrolling,
}

impl cosmic::Application for Window {
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
        let config_helper =
            Config::new("com.system76.CosmicComp", CosmicCompConfig::VERSION).unwrap();
        let mut config = CosmicCompConfig::get_entry(&config_helper).unwrap_or_else(|(errs, c)| {
            for err in errs {
                error!(?err, "Error loading config");
            }
            c
        });

        // Global is removed in favor of per-workspace
        if let Err(err) = config.set_autotile_behavior(&config_helper, TileBehavior::PerWorkspace) {
            error!(?err, "Failed to set autotile behavior to PerWorkspace");
        }

        let mut current_workspace_layout_model = SingleSelectModel::default();
        let current_workspace_layout_entities = LayoutEntities {
            floating: current_workspace_layout_model
                .insert()
                .text(fl!("floating"))
                .id(),
            tiling: current_workspace_layout_model
                .insert()
                .text(fl!("tiling"))
                .id(),
            scrolling: current_workspace_layout_model
                .insert()
                .text(fl!("scrolling"))
                .id(),
        };
        current_workspace_layout_model.activate(
            current_workspace_layout_entities
                .entity(derive_layout_mode(config.autotile, config.tiling_engine)),
        );

        let mut new_workspace_behavior_model = SingleSelectModel::default();
        let new_workspace_entities = LayoutEntities {
            floating: new_workspace_behavior_model
                .insert()
                .text(fl!("floating"))
                .id(),
            tiling: new_workspace_behavior_model
                .insert()
                .text(fl!("tiling"))
                .id(),
            scrolling: new_workspace_behavior_model
                .insert()
                .text(fl!("scrolling"))
                .id(),
        };
        new_workspace_behavior_model.activate(
            new_workspace_entities
                .entity(derive_layout_mode(config.autotile, config.tiling_engine)),
        );

        let mut width_model = SingleSelectModel::default();
        let width_entities = ScrollingConfig::NEW_COLUMN_WIDTH_PRESETS
            .iter()
            .map(|preset| width_model.insert().text(percent(*preset)).id())
            .collect::<Vec<_>>();
        let width_custom_entity = width_model.insert().text(fl!("custom")).id();

        let mut centering_model = SingleSelectModel::default();
        let centering_entities = vec![
            (
                centering_model.insert().text(fl!("centering-lazy")).id(),
                CenteringPolicy::Lazy,
            ),
            (
                centering_model.insert().text(fl!("centering-always")).id(),
                CenteringPolicy::Always,
            ),
            (
                centering_model
                    .insert()
                    .text(fl!("centering-on-overflow"))
                    .id(),
                CenteringPolicy::OnOverflow,
            ),
        ];
        let mut position_model = SingleSelectModel::default();
        let position_entities = vec![
            (
                position_model
                    .insert()
                    .text(fl!("position-after-focused"))
                    .id(),
                NewWindowPosition::AfterFocused,
            ),
            (
                position_model.insert().text(fl!("position-end")).id(),
                NewWindowPosition::End,
            ),
        ];

        let mut window = Self {
            core,
            popup: None,
            autotiled: config.autotile,
            layout_change_pending: false,
            config,
            config_helper,
            current_workspace_layout_model,
            current_workspace_layout_entities,
            new_workspace_behavior_model,
            new_workspace_entities,
            width_model,
            width_entities,
            width_custom_entity,
            width_custom_selected: false,
            centering_model,
            centering_entities,
            position_model,
            position_entities,
            scrolling_expanded: false,
            workspace_tx: None,
        };
        window.sync_width_model();
        (window, Task::none())
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        Subscription::batch([
            self.core
                .watch_config::<CosmicCompConfig>("com.system76.CosmicComp")
                .map(|u| Message::MyConfigUpdate(Box::new(u.config))),
            wayland_subscription::workspaces().map(Message::WorkspaceUpdate),
        ])
    }

    fn update(&mut self, message: Self::Message) -> app::Task<Self::Message> {
        match message {
            Message::WorkspaceUpdate(msg) => match msg {
                WorkspacesUpdate::State(state) => {
                    self.autotiled = matches!(state, TilingState::TilingEnabled);
                    self.sync_current_workspace_layout();
                }
                WorkspacesUpdate::Started(tx) => {
                    self.workspace_tx = Some(tx);
                }
                WorkspacesUpdate::Errored => {
                    self.workspace_tx = None;
                    error!("Workspaces subscription failed...");
                }
            },
            Message::TogglePopup => {
                return if let Some(p) = self.popup.take() {
                    destroy_popup(p)
                } else {
                    return cosmic::surface::surface_task(cosmic::surface::action::app_popup(
                        |_| Default::default(),
                        |app: &mut Self| {
                            let new_id = Id::unique();
                            app.popup = Some(new_id);
                            let mut settings = app.core.applet.get_popup_settings(
                                app.core.main_window_id().unwrap(),
                                new_id,
                                Some((1, 1)),
                                None,
                                None,
                            );
                            settings.positioner.size_limits = layout_popup_limits();
                            settings
                        },
                        None,
                    ));
                };
            }
            Message::PopupClosed(id) => {
                if self.popup.as_ref() == Some(&id) {
                    self.popup = None;
                }
            }
            Message::CurrentWorkspaceLayout(entity) => {
                let Some(mode) = self.current_workspace_layout_entities.mode(entity) else {
                    error!("Unknown current-workspace layout entity");
                    return Task::none();
                };
                return self.set_current_workspace_layout(mode);
            }
            Message::LayoutChanged(result) => {
                self.layout_change_pending = false;
                match result {
                    Ok(transition) => {
                        if let Some(engine) = transition.tiling_engine {
                            self.config.tiling_engine = engine;
                        }
                        if let Some(tiled) = transition.workspace_tiled {
                            let state = if tiled {
                                TilingState::TilingEnabled
                            } else {
                                TilingState::FloatingOnly
                            };
                            if let Some(tx) = &self.workspace_tx
                                && let Err(err) = tx.try_send(AppRequest::TilingState(state))
                            {
                                error!(?err, "Failed to request workspace layout");
                            }
                        }
                    }
                    Err(err) => error!(%err, "Failed to change tiling engine"),
                }
                // Workspace state is confirmed by the protocol subscription.
                // A failed write must never leave an optimistic selection behind.
                self.sync_current_workspace_layout();
            }
            Message::ToggleActiveHint(toggled) => {
                self.config.active_hint = toggled;

                let helper = self.config_helper.clone();
                thread::spawn(move || {
                    if let Err(err) = helper.set("active_hint", toggled) {
                        error!(?err, "Failed to set active_hint {toggled}");
                    }
                });
            }
            Message::MyConfigUpdate(c) => {
                self.config = *c;
                self.new_workspace_behavior_model
                    .activate(self.new_workspace_entities.entity(derive_layout_mode(
                        self.config.autotile,
                        self.config.tiling_engine,
                    )));
                self.sync_width_model();
                self.sync_current_workspace_layout();
            }
            Message::NewWorkspace(e) => {
                let Some(mode) = self.new_workspace_entities.mode(e) else {
                    error!("Unknown new-workspace layout entity");
                    return Task::none();
                };
                let autotile_new = mode != WorkspaceLayoutMode::Floating;
                // Orbit: Tiling and Scrolling share the one global engine.
                let engine = match mode {
                    WorkspaceLayoutMode::Floating => None,
                    WorkspaceLayoutMode::Tiling => Some(TilingEngine::Classic),
                    WorkspaceLayoutMode::Scrolling => Some(TilingEngine::Scrolling),
                }
                .filter(|engine| *engine != self.config.tiling_engine);
                if let Some(engine) = engine {
                    self.config.tiling_engine = engine;
                    let helper = self.config_helper.clone();
                    thread::spawn(move || {
                        if let Err(err) = helper.set("tiling_engine", engine) {
                            error!(?err, "Failed to set tiling_engine {engine:?}");
                        }
                    });
                }
                self.config.autotile = autotile_new;
                self.new_workspace_behavior_model.activate(e);
                // set the config autotile behavior
                let helper = self.config_helper.clone();

                if let Some(tx) = self.workspace_tx.as_ref() {
                    let state = if autotile_new {
                        TilingState::TilingEnabled
                    } else {
                        TilingState::FloatingOnly
                    };

                    if let Err(err) = tx.try_send(AppRequest::DefaultBehavior(state)) {
                        error!("Failed to send the tiling state update. {err:?}");
                    }
                }

                thread::spawn(move || {
                    if let Err(err) = helper.set("autotile", autotile_new) {
                        error!(?err, "Failed to set autotile {autotile_new:?}");
                    }
                });
            }
            Message::NewWindowWidth(entity) => {
                if entity == self.width_custom_entity {
                    self.width_custom_selected = true;
                    self.width_model.activate(entity);
                } else if let Some(index) = self.width_entities.iter().position(|e| *e == entity) {
                    self.width_custom_selected = false;
                    self.config.scrolling.new_column_width =
                        ScrollingConfig::NEW_COLUMN_WIDTH_PRESETS[index];
                    self.width_model.activate(entity);
                    self.save_scrolling();
                }
            }
            Message::CustomWidthChanged(value) => {
                self.config.scrolling.new_column_width = snap_width(value);
            }
            Message::ToggleFocusFollowsScroll(enabled) => {
                self.config.scrolling.focus_follows_scroll = enabled;
                self.save_scrolling();
            }
            Message::FocusThresholdChanged(value) => {
                self.config.scrolling.focus_scroll_threshold = snap_threshold(value);
            }
            Message::ToggleBackdropBlur(enabled) => {
                self.config.scrolling.backdrop_blur = enabled;
                self.save_scrolling();
            }
            Message::Centering(entity) => {
                if let Some((_, policy)) =
                    self.centering_entities.iter().find(|(e, _)| *e == entity)
                {
                    self.config.scrolling.centering = *policy;
                    self.centering_model.activate(entity);
                    self.save_scrolling();
                }
            }
            Message::NewWindowPosition(entity) => {
                if let Some((_, position)) =
                    self.position_entities.iter().find(|(e, _)| *e == entity)
                {
                    self.config.scrolling.new_window_position = *position;
                    self.position_model.activate(entity);
                    self.save_scrolling();
                }
            }
            Message::ToggleCenterSingle(enabled) => {
                self.config.scrolling.center_single_column = enabled;
                self.save_scrolling();
            }
            Message::ToggleDimOffscreen(enabled) => {
                self.config.scrolling.offscreen_opacity =
                    if enabled { DIMMED_OPACITY } else { 1.0 };
                self.save_scrolling();
            }
            Message::TogglePositionIndicator(enabled) => {
                self.config.scrolling.position_indicator = enabled;
                self.save_scrolling();
            }
            Message::ToggleScrollingSection => {
                self.scrolling_expanded = !self.scrolling_expanded;
            }
            Message::RemoveWidthRule(index) => {
                if index < self.config.scrolling.width_rules.len() {
                    self.config.scrolling.width_rules.remove(index);
                    self.save_scrolling();
                }
            }
            Message::SaveScrolling => self.save_scrolling(),
            Message::OpenSettings => {
                let mut cmd = std::process::Command::new("cosmic-settings");
                cmd.arg("window-management");
                tokio::spawn(cosmic::process::spawn(cmd));
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let icon = match self.current_workspace_layout() {
            WorkspaceLayoutMode::Floating => OFF,
            WorkspaceLayoutMode::Tiling => ON,
            WorkspaceLayoutMode::Scrolling => SCROLLING,
        };
        self.core
            .applet
            .icon_button(icon)
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

        let new_workspace_behavior_button =
            segmented_control::horizontal(&self.new_workspace_behavior_model)
                .on_activate(Message::NewWorkspace);
        let mut current_workspace_layout_button =
            segmented_control::horizontal(&self.current_workspace_layout_model);
        if !self.layout_change_pending && self.workspace_tx.is_some() {
            current_workspace_layout_button =
                current_workspace_layout_button.on_activate(Message::CurrentWorkspaceLayout);
        }
        let content_list = column![
            padded_control(container(
                column![
                    text::body(fl!("current-workspace-layout")),
                    current_workspace_layout_button,
                    text::caption(fl!("tiling-engine-global")),
                ]
                .spacing(space_xxxs)
            )),
            padded_control(divider::horizontal::default()).padding([space_xxs, space_s]),
            padded_control(
                column![
                    text::body(fl!("new-workspace")),
                    new_workspace_behavior_button,
                ]
                .spacing(space_xxxs)
            ),
            self.scrolling_settings(space_xxxs, space_xxs, space_s),
            padded_control(divider::horizontal::default()).padding([space_xxs, space_s]),
            padded_control(row!(
                text::body(fl!("navigate-windows")).width(Length::Fill),
                text::body(format!("{} + {}", fl!("super"), fl!("arrow-keys"))),
            )),
            padded_control(row!(
                text::body(fl!("move-window")).width(Length::Fill),
                text::body(format!(
                    "{} + {} + {}",
                    fl!("shift"),
                    fl!("super"),
                    fl!("arrow-keys")
                )),
            )),
            padded_control(row!(
                text::body(fl!("toggle-floating-window")).width(Length::Fill),
                text::body(format!("{} + G", fl!("super"))),
            )),
            padded_control(divider::horizontal::default()).padding([space_xxs, space_s]),
            padded_control(
                toggler(self.config.active_hint)
                    .on_toggle(Message::ToggleActiveHint)
                    .label(fl!("active-hint"))
                    .text_size(14)
                    .width(Length::Fill),
            ),
            padded_control(divider::horizontal::default()).padding([space_xxs, space_s]),
            menu_button(text::body(fl!("window-management-settings")))
                .on_press(Message::OpenSettings)
        ]
        .padding([8, 0]);

        self.core
            .applet
            .popup_container(content_list)
            .limits(layout_popup_limits())
            .into()
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}

impl Window {
    /// Orbit: settings that only apply to the Scrolling engine. Hidden while
    /// the engine is Classic, so the popup stays as short as upstream's.
    fn scrolling_settings(
        &self,
        space_xxxs: u16,
        space_xxs: u16,
        space_s: u16,
    ) -> Element<'_, Message> {
        if self.config.tiling_engine != TilingEngine::Scrolling {
            return column![].into();
        }
        let chevron = if self.scrolling_expanded {
            "go-up-symbolic"
        } else {
            "go-down-symbolic"
        };
        let header = column![
            padded_control(divider::horizontal::default()).padding([space_xxs, space_s]),
            menu_button(row!(
                text::body(fl!("scrolling-settings")).width(Length::Fill),
                icon::from_name(chevron).size(16).icon(),
            ))
            .on_press(Message::ToggleScrollingSection),
        ];
        if !self.scrolling_expanded {
            return header.into();
        }
        let scrolling = &self.config.scrolling;
        let custom = self.width_custom_selected
            || width_choice(scrolling.new_column_width) == WidthChoice::Custom;

        let mut width = column![
            row!(
                text::body(fl!("new-window-width")).width(Length::Fill),
                text::body(percent(scrolling.clamped_new_column_width())),
            ),
            segmented_control::horizontal(&self.width_model).on_activate(Message::NewWindowWidth),
        ]
        .spacing(space_xxxs);
        if custom {
            width = width.push(
                slider(
                    ScrollingConfig::MIN_COLUMN_WIDTH..=1.0,
                    scrolling.clamped_new_column_width(),
                    Message::CustomWidthChanged,
                )
                .step(0.01)
                .on_release(Message::SaveScrolling),
            );
        }

        let mut focus = column![
            toggler(scrolling.focus_follows_scroll)
                .on_toggle(Message::ToggleFocusFollowsScroll)
                .label(fl!("focus-follows-scroll"))
                .text_size(14)
                .width(Length::Fill),
        ]
        .spacing(space_xxxs);
        if scrolling.focus_follows_scroll {
            focus = focus.push(row!(
                text::caption(fl!("focus-scroll-threshold")).width(Length::Fill),
                text::caption(percent(scrolling.clamped_focus_scroll_threshold())),
            ));
            focus = focus.push(
                slider(
                    0.1..=1.0,
                    scrolling.clamped_focus_scroll_threshold(),
                    Message::FocusThresholdChanged,
                )
                .step(0.05)
                .on_release(Message::SaveScrolling),
            );
        }

        let mut rules = column![text::body(fl!("width-rules"))].spacing(space_xxxs);
        if scrolling.width_rules.is_empty() {
            rules = rules.push(text::caption(fl!("width-rules-empty")));
        }
        for (index, rule) in scrolling.width_rules.iter().enumerate() {
            rules = rules.push(
                row!(
                    text::body(rule.app_id.clone()).width(Length::Fill),
                    text::body(percent(rule.width)),
                    button::text(fl!("remove")).on_press(Message::RemoveWidthRule(index)),
                )
                .spacing(space_xxs)
                .align_y(cosmic::iced::Alignment::Center),
            );
        }

        column![
            header,
            padded_control(width),
            padded_control(rules),
            padded_control(focus),
            padded_control(
                column![
                    text::body(fl!("centering")),
                    segmented_control::horizontal(&self.centering_model)
                        .on_activate(Message::Centering),
                ]
                .spacing(space_xxxs)
            ),
            padded_control(
                column![
                    text::body(fl!("new-window-position")),
                    segmented_control::horizontal(&self.position_model)
                        .on_activate(Message::NewWindowPosition),
                ]
                .spacing(space_xxxs)
            ),
            padded_control(
                toggler(scrolling.center_single_column)
                    .on_toggle(Message::ToggleCenterSingle)
                    .label(fl!("center-single-column"))
                    .text_size(14)
                    .width(Length::Fill),
            ),
            padded_control(
                toggler(scrolling.backdrop_blur)
                    .on_toggle(Message::ToggleBackdropBlur)
                    .label(fl!("backdrop-blur"))
                    .text_size(14)
                    .width(Length::Fill),
            ),
            padded_control(
                toggler(scrolling.clamped_offscreen_opacity() < 1.0)
                    .on_toggle(Message::ToggleDimOffscreen)
                    .label(fl!("dim-offscreen"))
                    .text_size(14)
                    .width(Length::Fill),
            ),
            padded_control(
                toggler(scrolling.position_indicator)
                    .on_toggle(Message::TogglePositionIndicator)
                    .label(fl!("position-indicator"))
                    .text_size(14)
                    .width(Length::Fill),
            ),
            padded_control(row!(
                text::body(fl!("strip-overview")).width(Length::Fill),
                text::body(format!("{} + {} + O", fl!("shift"), fl!("super"))),
            )),
            padded_control(row!(
                text::body(fl!("scroll-view")).width(Length::Fill),
                text::body(format!("{} + PgUp / PgDn", fl!("super"))),
            )),
            padded_control(row!(
                text::body(fl!("first-last-column")).width(Length::Fill),
                text::body(format!("{} + Home / End", fl!("super"))),
            )),
        ]
        .into()
    }

    /// Orbit: select the width, centering and position choices that match
    /// the stored settings.
    fn sync_width_model(&mut self) {
        let scrolling = &self.config.scrolling;
        if let Some((entity, _)) = self
            .centering_entities
            .iter()
            .find(|(_, p)| *p == scrolling.centering)
        {
            self.centering_model.activate(*entity);
        }
        if let Some((entity, _)) = self
            .position_entities
            .iter()
            .find(|(_, p)| *p == scrolling.new_window_position)
        {
            self.position_model.activate(*entity);
        }
        let entity = match width_choice(self.config.scrolling.new_column_width) {
            WidthChoice::Preset(index) if !self.width_custom_selected => self.width_entities[index],
            _ => self.width_custom_entity,
        };
        self.width_model.activate(entity);
    }

    /// Orbit: write the Scrolling settings off the UI thread.
    fn save_scrolling(&self) {
        let helper = self.config_helper.clone();
        let scrolling = self.config.scrolling.clone();
        thread::spawn(move || {
            if let Err(err) = helper.set("scrolling", scrolling) {
                error!(?err, "Failed to save scrolling settings");
            }
        });
    }

    fn current_workspace_layout(&self) -> WorkspaceLayoutMode {
        derive_layout_mode(self.autotiled, self.config.tiling_engine)
    }

    fn sync_current_workspace_layout(&mut self) {
        let entity = self
            .current_workspace_layout_entities
            .entity(self.current_workspace_layout());
        self.current_workspace_layout_model.activate(entity);
    }

    fn set_current_workspace_layout(
        &mut self,
        requested: WorkspaceLayoutMode,
    ) -> app::Task<Message> {
        // Serialize writes: detached threads can finish in reverse click order.
        if self.layout_change_pending || self.workspace_tx.is_none() {
            return Task::none();
        }
        let transition =
            plan_layout_transition(self.autotiled, self.config.tiling_engine, requested);
        if transition == LayoutTransition::default() {
            self.sync_current_workspace_layout();
            return Task::none();
        }

        self.layout_change_pending = true;
        let helper = self.config_helper.clone();
        cosmic::task::future(async move {
            let result = tokio::task::spawn_blocking(move || {
                if let Some(engine) = transition.tiling_engine {
                    helper
                        .set("tiling_engine", engine)
                        .map_err(|err| err.to_string())?;
                }
                Ok(transition)
            })
            .await
            .unwrap_or_else(|err| Err(err.to_string()));
            Message::LayoutChanged(result)
        })
    }
}

/// Opacity the "dim windows partly off screen" switch uses.
const DIMMED_OPACITY: f64 = 0.6;

fn layout_popup_limits() -> Limits {
    // Three choices plus the selected checkmark need more room than the
    // standard two-choice applet popup; otherwise “Scrolling” is clipped.
    Limits::NONE
        .min_width(420.0)
        .max_width(420.0)
        .min_height(1.0)
        .max_height(1000.0)
}
