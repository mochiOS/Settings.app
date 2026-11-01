mod accounts;
mod preferences;

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mochi_user_platform::mboot_wifi as wifi;
use mochios_user_database::UserRecord;
use preferences::Preferences;
use viewkit::{
    event::{EventContext, EventResult, ViewEvent},
    prelude::*,
    view::{Constraints, MeasureContext, PaintContext},
};

const AUTOSAVE_DELAY: Duration = Duration::from_millis(450);

struct AutosaveState {
    persisted: Preferences,
    pending: Option<(Preferences, Instant)>,
    failed: Option<Preferences>,
}

struct AutosaveLayer<C> {
    content: C,
    snapshot: Preferences,
    state: Rc<RefCell<AutosaveState>>,
    status: State<String>,
    consent: State<bool>,
}

struct AutoLoginSwitch {
    control: Switch,
    enabled: State<bool>,
    user: State<String>,
    selected_name: Option<String>,
}

enum NotificationSwitchTarget {
    Focus,
    Application {
        bundle_id: String,
        disabled_bundle_ids: State<Vec<String>>,
    },
}

struct NotificationPreferenceSwitch {
    control: Switch,
    enabled: State<bool>,
    target: NotificationSwitchTarget,
    status: State<String>,
}

impl View for NotificationPreferenceSwitch {
    fn measure(&self, constraints: Constraints, context: &mut MeasureContext<'_>) -> Size {
        self.control.measure(constraints, context)
    }

    fn paint(&self, bounds: Rect, context: &mut PaintContext<'_>) {
        self.control.paint(bounds, context);
    }

    fn handle_event(
        &self,
        bounds: Rect,
        event: &ViewEvent,
        context: &mut EventContext<'_>,
    ) -> EventResult {
        let previous = self.enabled.get();
        let result = self.control.handle_event(bounds, event, context);
        let enabled = self.enabled.get();
        if enabled == previous {
            return result;
        }
        let outcome = match &self.target {
            NotificationSwitchTarget::Focus => {
                mochi_user_platform::workspace::set_notification_focus_enabled(enabled)
            }
            NotificationSwitchTarget::Application { bundle_id, .. } => {
                mochi_user_platform::workspace::set_application_notifications_enabled(
                    bundle_id, enabled,
                )
            }
        };
        match outcome {
            Ok(()) => {
                if let NotificationSwitchTarget::Application {
                    bundle_id,
                    disabled_bundle_ids,
                } = &self.target
                {
                    let mut disabled = disabled_bundle_ids.get();
                    disabled.retain(|item| item != bundle_id);
                    if !enabled {
                        disabled.push(bundle_id.clone());
                        disabled.sort();
                        disabled.dedup();
                    }
                    disabled_bundle_ids.set(disabled);
                }
                self.status.set(String::new());
            }
            Err(error) => {
                self.enabled.set(previous);
                self.status
                    .set(format!("Unable to change notification settings: {error:?}"));
            }
        }
        result
    }
}

impl View for AutoLoginSwitch {
    fn measure(&self, constraints: Constraints, context: &mut MeasureContext<'_>) -> Size {
        self.control.measure(constraints, context)
    }

    fn paint(&self, bounds: Rect, context: &mut PaintContext<'_>) {
        self.control.paint(bounds, context);
    }

    fn handle_event(
        &self,
        bounds: Rect,
        event: &ViewEvent,
        context: &mut EventContext<'_>,
    ) -> EventResult {
        let was_enabled = self.enabled.get();
        let result = self.control.handle_event(bounds, event, context);
        if self.enabled.get() != was_enabled {
            self.user.set(if self.enabled.get() {
                self.selected_name.clone().unwrap_or_default()
            } else {
                String::new()
            });
        }
        result
    }
}

impl<C: View> View for AutosaveLayer<C> {
    fn measure(&self, constraints: Constraints, context: &mut MeasureContext<'_>) -> Size {
        self.content.measure(constraints, context)
    }

    fn paint(&self, bounds: Rect, context: &mut PaintContext<'_>) {
        self.content.paint(bounds, context);
        let now = Instant::now();
        let mut state = self.state.borrow_mut();
        if self.snapshot == state.persisted || state.failed.as_ref() == Some(&self.snapshot) {
            return;
        }
        if state
            .pending
            .as_ref()
            .is_none_or(|(pending, _)| pending != &self.snapshot)
        {
            state.pending = Some((self.snapshot.clone(), now + AUTOSAVE_DELAY));
        }
        let deadline = state
            .pending
            .as_ref()
            .map(|(_, deadline)| *deadline)
            .unwrap_or(now);
        if now < deadline {
            context.request_redraw_in_at(bounds, deadline);
            return;
        }
        let result = self.snapshot.save_changed(&state.persisted);
        match result {
            Ok(()) => {
                state.persisted = self.snapshot.clone();
                state.pending = None;
                state.failed = None;
                if !self.snapshot.diagnostics_enabled {
                    self.consent.set_if_changed(false);
                }
                if self.status.get().starts_with("Unable to save settings:") {
                    self.status.set(String::new());
                }
                let _ = viewkit::appearance::notify_changed();
            }
            Err(error) => {
                state.pending = None;
                state.failed = Some(self.snapshot.clone());
                self.status.set(format!("Unable to save settings: {error}"));
            }
        }
    }

    fn handle_event(
        &self,
        bounds: Rect,
        event: &ViewEvent,
        context: &mut EventContext<'_>,
    ) -> EventResult {
        self.content.handle_event(bounds, event, context)
    }
}

const GRANTS_PATH: &str = "/var/lib/security/capability-grants.db";

const BUILD_METADATA: &str = concat!(
    env!("MOCHIOS_VERSION"),
    "\n",
    env!("MNU_VERSION"),
    "\n",
    env!("MBOOT_VERSION"),
    "\n",
    env!("MOCHIOS_BUILD_NUMBER"),
);

fn build_metadata(index: usize) -> &'static str {
    BUILD_METADATA
        .split('\n')
        .nth(index)
        .unwrap_or("unavailable")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Account,
    General,
    Appearance,
    Input,
    Notifications,
    Network,
    Security,
    DefaultApps,
    Applications,
}

impl Section {
    const PRIMARY: [Self; 6] = [
        Self::Account,
        Self::General,
        Self::Appearance,
        Self::Input,
        Self::Notifications,
        Self::Network,
    ];
    const SYSTEM: [Self; 3] = [Self::Security, Self::DefaultApps, Self::Applications];

    const fn index(self) -> usize {
        match self {
            Self::Account => 0,
            Self::General => 1,
            Self::Appearance => 2,
            Self::Input => 3,
            Self::Notifications => 4,
            Self::Network => 5,
            Self::Security => 6,
            Self::DefaultApps => 7,
            Self::Applications => 8,
        }
    }

    const fn from_index(index: usize) -> Self {
        match index {
            0 => Self::Account,
            1 => Self::General,
            2 => Self::Appearance,
            3 => Self::Input,
            4 => Self::Notifications,
            5 => Self::Network,
            6 => Self::Security,
            7 => Self::DefaultApps,
            8 => Self::Applications,
            _ => Self::General,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Account => "Account",
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Input => "Input",
            Self::Notifications => "Notifications",
            Self::Network => "Network",
            Self::Security => "Security",
            Self::DefaultApps => "Default Apps",
            Self::Applications => "Applications",
        }
    }

    const fn description(self) -> &'static str {
        match self {
            Self::Account => "Users, passwords, and sign-in options",
            Self::General => "Device, language, region, date, and system information",
            Self::Appearance => "Theme, accent, wallpaper, and interface sizing",
            Self::Input => "Keyboard, mouse, touchpad, and shortcuts",
            Self::Notifications => "Focus mode and application notification delivery",
            Self::Network => "Ethernet, Wi-Fi, addressing, DNS, and proxy",
            Self::Security => "Certificates, trust, execution policy, and events",
            Self::DefaultApps => "Choose which application opens each file type",
            Self::Applications => "Review and revoke application capabilities",
        }
    }

    const fn symbol(self) -> Option<SymbolName> {
        match self {
            Self::General => Some(SymbolName::Info),
            Self::Appearance => Some(SymbolName::Paintbrush),
            Self::Input => Some(SymbolName::Keyboard),
            Self::Notifications => Some(SymbolName::Bell),
            Self::Network => Some(SymbolName::Network),
            Self::Applications => Some(SymbolName::Grid),
            Self::Account => None,
            Self::Security => Some(SymbolName::Lock),
            Self::DefaultApps => Some(SymbolName::File),
        }
    }
}

fn section_matches_search(section: Section, query: &str) -> bool {
    let query = query.trim().to_ascii_lowercase();
    query.is_empty()
        || section.label().to_ascii_lowercase().contains(&query)
        || section.description().to_ascii_lowercase().contains(&query)
}

#[derive(Clone)]
struct ApplicationInfo {
    name: String,
    bundle_id: String,
    developer: String,
    executable: String,
    icon: Option<ImageData>,
    document_extensions: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileAssociationSetting {
    extension: String,
    handlers: Vec<AssociationApplication>,
    default_bundle_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AssociationApplication {
    name: String,
    bundle_id: String,
}

struct SettingsApp {
    switch_interactions: RefCell<BTreeMap<String, SwitchInteractionState>>,
    slider_interactions: RefCell<BTreeMap<String, SliderInteractionState>>,
    segment_interactions: RefCell<BTreeMap<String, SegmentedControlInteractionState>>,
    autosave: Rc<RefCell<AutosaveState>>,
    users_loaded: Cell<bool>,
    network_loaded: Cell<bool>,
    applications_loaded: Cell<bool>,
    associations_loaded: Cell<bool>,
    notifications_loaded: Cell<bool>,
    section: State<usize>,
    search: State<String>,
    status: State<String>,
    users: State<Vec<UserRecord>>,
    selected_user: State<usize>,
    new_name: State<String>,
    new_display_name: State<String>,
    new_user_password: State<String>,
    password: State<String>,
    auto_login: State<bool>,
    auto_login_user: State<String>,
    device_name: State<String>,
    language: State<String>,
    region: State<String>,
    timezone: State<String>,
    automatic_time: State<bool>,
    appearance: State<usize>,
    accent: State<usize>,
    wallpaper: State<String>,
    ui_scale: State<f32>,
    font_size: State<f32>,
    keyboard_layout: State<usize>,
    repeat_delay: State<f32>,
    repeat_rate: State<f32>,
    mouse_speed: State<f32>,
    natural_scrolling: State<bool>,
    touchpad_tap: State<bool>,
    ethernet_enabled: State<bool>,
    wifi_enabled: State<bool>,
    wifi_status: State<wifi::WifiStatus>,
    wifi_networks: State<Vec<wifi::WifiNetwork>>,
    wifi_selected: State<usize>,
    wifi_password: State<String>,
    network_mode: State<usize>,
    ip_address: State<String>,
    dns_server: State<String>,
    proxy_enabled: State<bool>,
    proxy: State<String>,
    unsigned_policy: State<usize>,
    diagnostics_enabled: State<bool>,
    diagnostics_consent: State<bool>,
    notification_focus_enabled: State<bool>,
    notification_disabled_bundle_ids: State<Vec<String>>,
    applications: State<Vec<ApplicationInfo>>,
    selected_application: State<usize>,
    application_detail_open: State<bool>,
    application_search: State<String>,
    file_associations: State<Vec<FileAssociationSetting>>,
    page_scroll: ScrollState,
    sidebar_scroll: ScrollState,
}

impl SettingsApp {
    fn switch(&self, key: impl Into<String>, value: Binding<bool>) -> Switch {
        let interaction = self
            .switch_interactions
            .borrow_mut()
            .entry(key.into())
            .or_default()
            .clone();
        Switch::with_interaction(value, interaction)
    }
    fn slider(&self, key: &str, value: Binding<f32>) -> Slider {
        let interaction = self
            .slider_interactions
            .borrow_mut()
            .entry(key.to_owned())
            .or_default()
            .clone();
        Slider::with_interaction(value, interaction)
    }
    fn segments(&self, key: &str, value: Binding<usize>) -> SegmentedControl {
        let interaction = self
            .segment_interactions
            .borrow_mut()
            .entry(key.to_owned())
            .or_default()
            .clone();
        SegmentedControl::with_interaction(value, interaction)
    }

    fn secondary(text: impl Into<String>) -> Text {
        Text::styled(text.into(), TextRole::Caption).color(Theme::current().colors.text_secondary)
    }

    fn setting_row<C>(
        title: impl Into<String>,
        description: impl Into<String>,
        control: C,
    ) -> StackChild
    where
        C: IntoStackChild,
    {
        SettingsRow::new(title, control)
            .description(description)
            .compact()
            .into_stack_child()
            .flex_shrink(0.0)
    }

    fn value_row(
        title: impl Into<String>,
        description: impl Into<String>,
        value: impl Into<String>,
    ) -> StackChild {
        settings_value(title, description, value)
            .compact()
            .into_stack_child()
            .flex_shrink(0.0)
    }

    fn group(title: impl Into<String>, rows: Vec<StackChild>) -> StackChild {
        SettingsSection::new(title)
            .grouped()
            .rows(rows)
            .into_stack_child()
            .flex_shrink(0.0)
    }

    fn page(groups: Vec<StackChild>) -> Box<dyn View + 'static> {
        Box::new(
            VStack::new()
                .alignment(StackAlignment::Stretch)
                .gap(StackGap::ExtraLarge)
                .children(groups),
        )
    }

    fn field(value: State<String>, placeholder: &'static str) -> StackChild {
        TextField::new(value.binding())
            .placeholder(placeholder)
            .size(TextFieldSize::Medium)
            .radius(CornerRadius::Small)
            .frame(
                Theme::current().layout.form_control_width,
                Theme::current().controls.height,
            )
    }

    fn secure_field(value: State<String>, placeholder: &'static str) -> StackChild {
        TextField::new(value.binding())
            .placeholder(placeholder)
            .size(TextFieldSize::Medium)
            .radius(CornerRadius::Small)
            .secure(true)
            .frame(
                Theme::current().layout.compact_form_control_width,
                Theme::current().controls.height,
            )
    }

    fn navigation_group(&self, title: &'static str, sections: &[Section]) -> StackChild {
        let mut rows = SidebarSection::new(title);
        for section in sections.iter().copied() {
            let selected = Section::from_index(self.section.get()) == section;
            let section_state = self.section.clone();
            let search_state = self.search.clone();
            let application_detail = self.application_detail_open.clone();
            let page_scroll = self.page_scroll.clone();
            let mut item = SidebarItem::new(section.label())
                .reserve_symbol_space()
                .selected(selected);
            if let Some(symbol) = section.symbol() {
                item = item.symbol(symbol);
            }
            rows = rows.item(item.on_select(move || {
                application_detail.set(false);
                section_state.set(section.index());
                search_state.set(String::new());
                page_scroll.reset();
            }));
        }
        rows.into_stack_child()
    }

    fn sidebar(&self) -> StackChild {
        let query = self.search.get();
        let query = query.trim();
        let primary: Vec<_> = Section::PRIMARY
            .into_iter()
            .filter(|section| section_matches_search(*section, query))
            .collect();
        let system: Vec<_> = Section::SYSTEM
            .into_iter()
            .filter(|section| section_matches_search(*section, query))
            .collect();
        let mut content = VStack::new()
            .alignment(StackAlignment::Stretch)
            .gap(StackGap::ExtraLarge)
            .child(
                Padding::only(0.0, 0.0, 0.0, Theme::current().spacing.small)
                    .content(Text::styled("Settings", TextRole::TitleSmall)),
            )
            .child(
                TextField::new(self.search.binding())
                    .placeholder("Search Settings")
                    .size(TextFieldSize::Small)
                    .leading_symbol(SymbolName::Search),
            );
        if !primary.is_empty() {
            content = content.child(self.navigation_group("Personal", &primary));
        }
        if !system.is_empty() {
            content = content.child(self.navigation_group("System", &system));
        }
        if primary.is_empty() && system.is_empty() {
            content = content.child(Self::secondary("No matching settings"));
        }
        content.into_stack_child()
    }

    fn detail_header(&self) -> StackChild {
        let section = Section::from_index(self.section.get());
        VStack::new()
            .alignment(StackAlignment::Stretch)
            .gap(StackGap::ExtraSmall)
            .child(Text::styled(section.label(), TextRole::TitleSmall))
            .child(Self::secondary(section.description()))
            .into_stack_child()
            .flex_shrink(0.0)
    }

    fn status_bar(&self) -> StackChild {
        let status = self.status.get();
        let left = status;
        Background::new()
            .background(Rectangle::new().color(RectangleColor::Surface))
            .content(
                Padding::symmetric(
                    Theme::current().spacing.medium,
                    Theme::current().spacing.extra_small,
                )
                .content(
                    HStack::new()
                        .alignment(StackAlignment::Center)
                        .distribution(StackDistribution::SpaceBetween)
                        .child(Self::secondary(left))
                        .child(Spacer::new()),
                ),
            )
            .height(Theme::current().layout.status_bar_height)
    }

    fn account_page(&self) -> Box<dyn View + 'static> {
        if !self.users_loaded.replace(true) {
            match accounts::load() {
                Ok(database) => {
                    let users = database.users().to_vec();
                    if let Some(index) = users
                        .iter()
                        .position(|user| user.name == self.auto_login_user.get())
                    {
                        self.selected_user.set(index);
                    }
                    self.users.set(users);
                }
                Err(error) => self.status.set(format!("Unable to load users: {error}")),
            }
        }
        let users = self.users.get();
        let selected = self.selected_user.get().min(users.len().saturating_sub(1));
        let selected_name = users.get(selected).map(|user| user.name.clone());
        let mut user_rows = Vec::new();
        for (index, user) in users.iter().enumerate() {
            let selection = self.selected_user.clone();
            user_rows.push(
                Button::new(user.display_name.clone())
                    .content(
                        Padding::symmetric(0.0, Theme::current().spacing.small).content(
                            HStack::new()
                                .alignment(StackAlignment::Center)
                                .gap(StackGap::Medium)
                                .child(
                                    Icon::new(SymbolName::House)
                                        .size(Theme::current().layout.stepper_icon_size)
                                        .frame(
                                            Theme::current().layout.icon_button_size,
                                            Theme::current().layout.icon_button_size,
                                        ),
                                )
                                .child(
                                    VStack::new()
                                        .alignment(StackAlignment::Stretch)
                                        .gap(StackGap::None)
                                        .child(
                                            Text::styled(
                                                user.display_name.clone(),
                                                TextRole::Label,
                                            )
                                            .weight(600),
                                        )
                                        .child(Self::secondary(format!(
                                            "{} · {}",
                                            user.name,
                                            if user.uid == 0 {
                                                "Administrator"
                                            } else {
                                                "User"
                                            }
                                        ))),
                                ),
                        ),
                    )
                    .style(if index == selected {
                        ButtonStyle::Standard
                    } else {
                        ButtonStyle::Ghost
                    })
                    .alignment(ZStackAlignment::Leading)
                    .on_click(move || selection.set(index))
                    .height(Theme::current().layout.top_bar_height),
            );
        }
        if user_rows.is_empty() {
            user_rows.push(Self::value_row("Users", "", "No users available"));
        }

        let change_name = selected_name.clone();
        let change_password = self.password.clone();
        let change_status = self.status.clone();
        let change_button = Button::new("Change")
            .size(ButtonSize::Small)
            .style(ButtonStyle::Standard)
            .enabled(change_name.is_some())
            .on_click(move || {
                let Some(name) = change_name.as_deref() else {
                    return;
                };
                let mut password = change_password.get();
                let result = accounts::set_password(name, password.as_bytes());
                password.clear();
                change_password.set(String::new());
                change_status.set(match result {
                    Ok(()) => String::from("Password changed."),
                    Err(error) => format!("Unable to change password: {error}"),
                });
            });

        let remove_name = selected_name.clone();
        let remove_users = self.users.clone();
        let remove_status = self.status.clone();
        let remove_button = Button::new("Delete User")
            .size(ButtonSize::Small)
            .style(ButtonStyle::Danger)
            .enabled(remove_name.as_deref().is_some_and(|name| name != "root"))
            .on_click(move || {
                let Some(name) = remove_name.as_deref() else {
                    return;
                };
                match accounts::remove(name) {
                    Ok(()) => {
                        remove_users.set(
                            accounts::load()
                                .map(|database| database.users().to_vec())
                                .unwrap_or_default(),
                        );
                        remove_status.set(String::from("User deleted. Home data was kept."));
                    }
                    Err(error) => remove_status.set(format!("Unable to delete user: {error}")),
                }
            });

        let add_users = self.users.clone();
        let add_name = self.new_name.clone();
        let add_display = self.new_display_name.clone();
        let add_password = self.new_user_password.clone();
        let add_status = self.status.clone();
        let add_button = Button::new("Add User")
            .size(ButtonSize::Small)
            .style(ButtonStyle::Standard)
            .on_click(move || {
                let name = add_name.get();
                let display = add_display.get();
                let mut password = add_password.get();
                let result = accounts::add(&name, &display, password.as_bytes());
                password.clear();
                add_password.set(String::new());
                match result {
                    Ok(()) => {
                        add_users.set(
                            accounts::load()
                                .map(|database| database.users().to_vec())
                                .unwrap_or_default(),
                        );
                        add_name.set(String::new());
                        add_display.set(String::new());
                        add_status.set(String::from("User added."));
                    }
                    Err(error) => add_status.set(format!("Unable to add user: {error}")),
                }
            });

        Self::page(vec![
            Self::group("Users", user_rows),
            Self::group(
                "Selected User",
                vec![
                    Self::setting_row(
                        "Password",
                        "Change the selected user's password",
                        HStack::new()
                            .alignment(StackAlignment::Center)
                            .gap(StackGap::Small)
                            .child(Self::secure_field(self.password.clone(), "New password"))
                            .child(change_button),
                    ),
                    Self::setting_row(
                        "Automatic Login",
                        "Sign in to this account when the device starts",
                        AutoLoginSwitch {
                            control: self
                                .switch("auto_login", self.auto_login.binding())
                                .enabled(selected_name.is_some()),
                            enabled: self.auto_login.clone(),
                            user: self.auto_login_user.clone(),
                            selected_name: selected_name.clone(),
                        },
                    ),
                    Self::setting_row(
                        "Delete User",
                        "Remove the account while keeping its home data",
                        remove_button,
                    ),
                ],
            ),
            Self::group(
                "Add User",
                vec![
                    Self::setting_row(
                        "Account Name",
                        "Used for the home directory and sign-in",
                        Self::field(self.new_name.clone(), "account"),
                    ),
                    Self::setting_row(
                        "Display Name",
                        "Name shown in the interface",
                        Self::field(self.new_display_name.clone(), "Display name"),
                    ),
                    Self::setting_row(
                        "Password",
                        "Initial password; an empty password is allowed",
                        Self::secure_field(self.new_user_password.clone(), "Password"),
                    ),
                    Self::setting_row("Create User", "", add_button),
                ],
            ),
            Self::group(
                "mochiOS ID",
                vec![Self::value_row(
                    "Account Linking",
                    "Connect this local user to a mochiOS ID",
                    "Coming later",
                )],
            ),
        ])
    }

    fn general_page(&self) -> Box<dyn View + 'static> {
        let language_japanese = self.language.clone();
        let language_english = self.language.clone();
        let region_japan = self.region.clone();
        let region_united_states = self.region.clone();
        let timezone_tokyo = self.timezone.clone();
        let timezone_utc = self.timezone.clone();
        let timezone_los_angeles = self.timezone.clone();
        Self::page(vec![
            Self::group(
                "Device",
                vec![Self::setting_row(
                    "Device Name",
                    "Name shown to nearby devices",
                    Self::field(self.device_name.clone(), "Device name"),
                )],
            ),
            Self::group(
                "Language & Region",
                vec![
                    Self::setting_row(
                        "Language",
                        "Primary system language",
                        Picker::new(self.language.get())
                            .option("Japanese", move || {
                                language_japanese.set(String::from("Japanese"));
                            })
                            .option("English", move || {
                                language_english.set(String::from("English"));
                            })
                            .radius(CornerRadius::Small)
                            .frame(
                                Theme::current().layout.form_control_width,
                                Theme::current().controls.height,
                            ),
                    ),
                    Self::setting_row(
                        "Region",
                        "Date, number, and measurement formats",
                        Picker::new(self.region.get())
                            .option("Japan", move || {
                                region_japan.set(String::from("Japan"));
                            })
                            .option("United States", move || {
                                region_united_states.set(String::from("United States"));
                            })
                            .radius(CornerRadius::Small)
                            .frame(
                                Theme::current().layout.form_control_width,
                                Theme::current().controls.height,
                            ),
                    ),
                ],
            ),
            Self::group(
                "Date & Time",
                vec![
                    Self::setting_row(
                        "Set Automatically",
                        "Synchronize date and time over the network",
                        self.switch("automatic_time", self.automatic_time.binding()),
                    ),
                    Self::setting_row(
                        "Time Zone",
                        "Current system time zone",
                        Picker::new(self.timezone.get())
                            .option("Asia/Tokyo", move || {
                                timezone_tokyo.set(String::from("Asia/Tokyo"));
                            })
                            .option("UTC", move || {
                                timezone_utc.set(String::from("UTC"));
                            })
                            .option("America/Los_Angeles", move || {
                                timezone_los_angeles.set(String::from("America/Los_Angeles"));
                            })
                            .radius(CornerRadius::Small)
                            .frame(
                                Theme::current().layout.form_control_width,
                                Theme::current().controls.height,
                            ),
                    ),
                    Self::value_row("Date & Time", "Current value", current_datetime()),
                ],
            ),
            Self::group(
                "About",
                vec![
                    Self::value_row("mochiOS Version", "", build_metadata(0)),
                    Self::value_row("Kernel Version", "", build_metadata(1)),
                    Self::value_row("mBoot Version", "", build_metadata(2)),
                    Self::value_row("Build Number", "", build_metadata(3)),
                    Self::value_row("Architecture", "", std::env::consts::ARCH),
                ],
            ),
        ])
    }

    fn appearance_page(&self) -> Box<dyn View + 'static> {
        Self::page(vec![
            Self::group(
                "Appearance",
                vec![
                    Self::setting_row(
                        "Theme",
                        "Choose how windows and controls are displayed",
                        self.segments("appearance", self.appearance.binding())
                            .item(0, "Light")
                            .item(1, "Dark")
                            .item(2, "System")
                            .frame(
                                Theme::current().layout.form_control_width,
                                Theme::current().controls.height,
                            ),
                    ),
                    Self::setting_row(
                        "Accent Color",
                        "Color used for selected controls",
                        Picker::new(
                            ["Blue", "Purple", "Pink", "Red", "Green", "Graphite"]
                                .get(self.accent.get())
                                .copied()
                                .unwrap_or("Blue"),
                        )
                        .option("Blue", {
                            let value = self.accent.clone();
                            move || value.set(0)
                        })
                        .option("Purple", {
                            let value = self.accent.clone();
                            move || value.set(1)
                        })
                        .option("Pink", {
                            let value = self.accent.clone();
                            move || value.set(2)
                        })
                        .option("Red", {
                            let value = self.accent.clone();
                            move || value.set(3)
                        })
                        .option("Green", {
                            let value = self.accent.clone();
                            move || value.set(4)
                        })
                        .option("Graphite", {
                            let value = self.accent.clone();
                            move || value.set(5)
                        })
                        .frame(
                            Theme::current().layout.form_control_width,
                            Theme::current().controls.height,
                        ),
                    ),
                ],
            ),
            Self::group(
                "Desktop",
                vec![Self::setting_row(
                    "Wallpaper",
                    "Desktop background image",
                    Self::field(self.wallpaper.clone(), "Wallpaper path"),
                )],
            ),
            Self::group(
                "Interface",
                vec![
                    Self::setting_row(
                        "UI Scale",
                        "Scale interface elements",
                        self.slider("ui_scale", self.ui_scale.binding())
                            .range(0.75..=2.0)
                            .step(0.05)
                            .frame(
                                Theme::current().layout.form_control_width,
                                Theme::current().controls.height,
                            ),
                    ),
                    Self::setting_row(
                        "Font Size",
                        "Default interface text size",
                        self.slider("font_size", self.font_size.binding())
                            .range(10.0..=24.0)
                            .step(1.0)
                            .frame(
                                Theme::current().layout.form_control_width,
                                Theme::current().controls.height,
                            ),
                    ),
                ],
            ),
        ])
    }

    fn input_page(&self) -> Box<dyn View + 'static> {
        let shortcut_status = self.status.clone();
        Self::page(vec![
            Self::group(
                "Keyboard",
                vec![
                    Self::setting_row(
                        "Keyboard Layout",
                        "Layout used for physical keyboard input",
                        self.segments("keyboard_layout", self.keyboard_layout.binding())
                            .item(0, "US")
                            .item(1, "Japanese")
                            .item(2, "British")
                            .frame(
                                Theme::current().layout.form_control_width,
                                Theme::current().controls.height,
                            ),
                    ),
                    Self::setting_row(
                        "Repeat Delay",
                        "Delay before a held key starts repeating",
                        self.slider("repeat_delay", self.repeat_delay.binding())
                            .range(0.2..=1.5)
                            .step(0.1)
                            .frame(
                                Theme::current().layout.form_control_width,
                                Theme::current().controls.height,
                            ),
                    ),
                    Self::setting_row(
                        "Repeat Rate",
                        "Speed of repeated key input",
                        self.slider("repeat_rate", self.repeat_rate.binding())
                            .range(5.0..=60.0)
                            .step(1.0)
                            .frame(
                                Theme::current().layout.form_control_width,
                                Theme::current().controls.height,
                            ),
                    ),
                    Self::setting_row(
                        "Shortcuts",
                        "Configure system keyboard shortcuts",
                        Button::new("Open")
                            .size(ButtonSize::Small)
                            .style(ButtonStyle::Standard)
                            .on_click(move || {
                                shortcut_status
                                    .set(String::from("Shortcut editing is not available yet."));
                            }),
                    ),
                ],
            ),
            Self::group(
                "Pointer",
                vec![
                    Self::setting_row(
                        "Mouse Speed",
                        "Pointer movement speed",
                        self.slider("mouse_speed", self.mouse_speed.binding())
                            .range(0.25..=3.0)
                            .step(0.05)
                            .frame(
                                Theme::current().layout.form_control_width,
                                Theme::current().controls.height,
                            ),
                    ),
                    Self::setting_row(
                        "Natural Scrolling",
                        "Move content in the same direction as your fingers",
                        self.switch("natural_scrolling", self.natural_scrolling.binding()),
                    ),
                    Self::setting_row(
                        "Tap to Click",
                        "Use a touchpad tap as a primary click",
                        self.switch("touchpad_tap", self.touchpad_tap.binding()),
                    ),
                ],
            ),
        ])
    }

    fn notifications_page(&self) -> Box<dyn View + 'static> {
        if !self.notifications_loaded.replace(true) {
            match mochi_user_platform::workspace::notification_settings() {
                Ok(settings) => {
                    self.notification_focus_enabled
                        .set_if_changed(settings.focus_enabled);
                    self.notification_disabled_bundle_ids
                        .set_if_changed(settings.disabled_bundle_ids);
                }
                Err(error) => self
                    .status
                    .set(format!("Unable to load notification settings: {error:?}")),
            }
        }
        if !self.applications_loaded.replace(true) {
            self.applications.set(load_applications());
        }

        let focus_enabled = self.notification_focus_enabled.clone();
        let focus_switch = NotificationPreferenceSwitch {
            control: self.switch("notification-focus", focus_enabled.binding()),
            enabled: focus_enabled,
            target: NotificationSwitchTarget::Focus,
            status: self.status.clone(),
        };
        let mut application_rows = Vec::new();
        let disabled = self.notification_disabled_bundle_ids.get();
        for application in self.applications.get() {
            let enabled = State::new(!disabled.contains(&application.bundle_id));
            application_rows.push(Self::setting_row(
                application.name,
                application.bundle_id.clone(),
                NotificationPreferenceSwitch {
                    control: self.switch(
                        format!("notification:{}", application.bundle_id),
                        enabled.binding(),
                    ),
                    enabled,
                    target: NotificationSwitchTarget::Application {
                        bundle_id: application.bundle_id,
                        disabled_bundle_ids: self.notification_disabled_bundle_ids.clone(),
                    },
                    status: self.status.clone(),
                },
            ));
        }
        if application_rows.is_empty() {
            application_rows.push(Self::value_row(
                "Applications",
                "Installed applications will appear here",
                "No applications installed",
            ));
        }

        Self::page(vec![
            Self::group(
                "Focus",
                vec![Self::setting_row(
                    "Focus Mode",
                    "Keep notifications in Notification Center without showing banners",
                    focus_switch,
                )],
            ),
            Self::group("Application Notifications", application_rows),
        ])
    }

    fn network_page(&self) -> Box<dyn View + 'static> {
        if !self.network_loaded.replace(true) {
            let host = wifi::status().unwrap_or_default();
            self.wifi_enabled.set(host.enabled);
            self.wifi_status.set(host);
        }
        let service_available = network_service_available();
        let static_enabled = self.network_mode.get() == 1;
        let proxy_enabled = self.proxy_enabled.get();
        let host = self.wifi_status.get();
        let networks = self.wifi_networks.get();
        let selected = self
            .wifi_selected
            .get()
            .min(networks.len().saturating_sub(1));
        let was_enabled = host.enabled;

        let enabled_state = self.wifi_enabled.clone();
        let host_state = self.wifi_status.clone();
        let operation_status = self.status.clone();
        let enable_wifi = Button::new(if was_enabled { "Turn Off" } else { "Turn On" })
            .size(ButtonSize::Small)
            .style(ButtonStyle::Standard)
            .enabled(host.available)
            .on_click(move || {
                let enabled = !was_enabled;
                match wifi::set_enabled(enabled).and_then(|()| wifi::status()) {
                    Ok(updated) => {
                        enabled_state.set(updated.enabled);
                        host_state.set(updated);
                        operation_status.set(if enabled {
                            String::from("Wi-Fi enabled.")
                        } else {
                            String::from("Wi-Fi disabled.")
                        });
                    }
                    Err(error) => operation_status.set(format!("Unable to change Wi-Fi: {error}")),
                }
            });

        let networks_state = self.wifi_networks.clone();
        let refreshed_host = self.wifi_status.clone();
        let refresh_status = self.status.clone();
        let refresh = Button::new("Scan")
            .size(ButtonSize::Small)
            .style(ButtonStyle::Standard)
            .enabled(host.available && host.enabled)
            .on_click(move || match wifi::scan() {
                Ok(found) => {
                    let count = found.len();
                    networks_state.set(found);
                    if let Ok(status) = wifi::status() {
                        refreshed_host.set(status);
                    }
                    refresh_status.set(format!("Found {count} Wi-Fi networks."));
                }
                Err(error) => refresh_status.set(format!("Unable to scan Wi-Fi: {error}")),
            });

        let mut network_rows = Vec::new();
        if networks.is_empty() {
            network_rows.push(Self::value_row(
                "No networks",
                "Select Scan to search for nearby Wi-Fi networks",
                "",
            ));
        } else {
            for (index, network) in networks.iter().enumerate() {
                let selection = self.wifi_selected.clone();
                let password = self.wifi_password.clone();
                let detail = format!(
                    "{} · signal {} dBm",
                    if network.secured { "Secured" } else { "Open" },
                    network.signal
                );
                network_rows.push(
                    Button::new(network.ssid.clone())
                        .content(
                            Padding::symmetric(0.0, Theme::current().spacing.small).content(
                                HStack::new()
                                    .alignment(StackAlignment::Center)
                                    .distribution(StackDistribution::SpaceBetween)
                                    .child(
                                        VStack::new()
                                            .alignment(StackAlignment::Stretch)
                                            .gap(StackGap::None)
                                            .child(
                                                Text::styled(network.ssid.clone(), TextRole::Label)
                                                    .weight(600),
                                            )
                                            .child(Self::secondary(detail)),
                                    )
                                    .child(
                                        Text::styled(
                                            if network.secured { "Protected" } else { "Open" },
                                            TextRole::Caption,
                                        )
                                        .color(Theme::current().colors.text_secondary),
                                    ),
                            ),
                        )
                        .style(if index == selected {
                            ButtonStyle::Standard
                        } else {
                            ButtonStyle::Ghost
                        })
                        .alignment(ZStackAlignment::Leading)
                        .on_click(move || {
                            selection.set(index);
                            password.set(String::new());
                        })
                        .height(Theme::current().layout.top_bar_height),
                );
            }
        }

        let selected_network = networks.get(selected).cloned();
        let connect_networks = self.wifi_networks.clone();
        let connect_selection = self.wifi_selected.clone();
        let connect_password = self.wifi_password.clone();
        let connected_host = self.wifi_status.clone();
        let connect_status = self.status.clone();
        let connect = Button::new("Connect")
            .size(ButtonSize::Small)
            .style(ButtonStyle::Standard)
            .enabled(selected_network.is_some())
            .on_click(move || {
                let available = connect_networks.get();
                let index = connect_selection.get();
                let Some(network) = available.get(index) else {
                    connect_status.set(String::from("Select a Wi-Fi network."));
                    return;
                };
                let password = connect_password.get();
                if network.secured && !(8..=63).contains(&password.len()) {
                    connect_status.set(String::from("Wi-Fi passwords must be 8 to 63 bytes."));
                    return;
                }
                match wifi::connect(network, &password) {
                    Ok(()) => {
                        connect_password.set(String::new());
                        if let Ok(status) = wifi::status() {
                            connected_host.set(status);
                        }
                        connect_status.set(format!("Connecting to {}.", network.ssid));
                    }
                    Err(error) => connect_status.set(format!("Unable to connect: {error}")),
                }
            });

        let disconnected_host = self.wifi_status.clone();
        let disconnect_status = self.status.clone();
        let disconnect = Button::new("Disconnect")
            .size(ButtonSize::Small)
            .style(ButtonStyle::Standard)
            .enabled(host.connected)
            .on_click(move || match wifi::disconnect() {
                Ok(()) => {
                    if let Ok(status) = wifi::status() {
                        disconnected_host.set(status);
                    }
                    disconnect_status.set(String::from("Wi-Fi disconnected."));
                }
                Err(error) => disconnect_status.set(format!("Unable to disconnect: {error}")),
            });

        let password_control: StackChild = if selected_network
            .as_ref()
            .is_some_and(|network| network.secured)
        {
            TextField::new(self.wifi_password.binding())
                .placeholder("Wi-Fi password")
                .secure(true)
                .frame(
                    Theme::current().layout.form_control_width,
                    Theme::current().layout.large_control_height,
                )
        } else {
            Self::secondary("No password required").into_stack_child()
        };
        Self::page(vec![
            Self::group(
                "Connections",
                vec![
                    Self::setting_row(
                        "Ethernet",
                        "Wired network connection",
                        self.switch("ethernet_enabled", self.ethernet_enabled.binding()),
                    ),
                    Self::setting_row(
                        "Wi-Fi",
                        if host.available {
                            "Wireless networking is managed by mBoot"
                        } else {
                            "No supported wireless adapter was detected"
                        },
                        HStack::new()
                            .alignment(StackAlignment::Center)
                            .gap(StackGap::Small)
                            .child(refresh)
                            .child(enable_wifi),
                    ),
                    Self::value_row(
                        "Connection Status",
                        "",
                        if host.connected {
                            format!("Connected to {}", host.ssid)
                        } else if host.available && host.enabled {
                            String::from("Not connected")
                        } else {
                            String::from("Unavailable")
                        },
                    ),
                    Self::value_row(
                        "Host Interface",
                        "mBoot wireless interface",
                        if host.interface.is_empty() {
                            "Unavailable"
                        } else {
                            &host.interface
                        },
                    ),
                    Self::value_row(
                        "Host IP Address",
                        "Address used by QEMU user networking",
                        if host.address.is_empty() {
                            "Not assigned"
                        } else {
                            &host.address
                        },
                    ),
                ],
            ),
            Self::group("Wi-Fi Networks", network_rows),
            Self::group(
                "Selected Network",
                vec![
                    Self::setting_row(
                        "Network",
                        "Access point selected above",
                        Text::styled(
                            selected_network
                                .as_ref()
                                .map(|network| network.ssid.as_str())
                                .unwrap_or("None"),
                            TextRole::Caption,
                        ),
                    ),
                    Self::setting_row("Password", "Stored only by mBoot", password_control),
                    Self::setting_row(
                        "Connection",
                        "Connect or disconnect the selected network",
                        HStack::new()
                            .alignment(StackAlignment::Center)
                            .gap(StackGap::Small)
                            .child(disconnect)
                            .child(connect),
                    ),
                ],
            ),
            Self::group(
                "IP & DNS",
                vec![
                    Self::setting_row(
                        "Configuration",
                        if service_available {
                            "mochiOS guest network configuration"
                        } else {
                            "network.service is unavailable"
                        },
                        self.segments("network_mode", self.network_mode.binding())
                            .item(0, "DHCP")
                            .item(1, "Static")
                            .frame(
                                Theme::current().layout.form_control_width,
                                Theme::current().controls.height,
                            ),
                    ),
                    Self::setting_row(
                        "IP Address",
                        "IPv4 address for this interface",
                        TextField::new(self.ip_address.binding())
                            .placeholder("0.0.0.0")
                            .enabled(static_enabled)
                            .frame(
                                Theme::current().layout.form_control_width,
                                Theme::current().layout.large_control_height,
                            ),
                    ),
                    Self::setting_row(
                        "DNS Server",
                        "Resolver used for domain names",
                        TextField::new(self.dns_server.binding())
                            .placeholder("0.0.0.0")
                            .enabled(static_enabled)
                            .frame(
                                Theme::current().layout.form_control_width,
                                Theme::current().layout.large_control_height,
                            ),
                    ),
                ],
            ),
            Self::group(
                "Proxy",
                vec![
                    Self::setting_row(
                        "Use Proxy",
                        "Route HTTP and HTTPS traffic through a proxy",
                        self.switch("proxy_enabled", self.proxy_enabled.binding()),
                    ),
                    Self::setting_row(
                        "Proxy Address",
                        "Host and port",
                        TextField::new(self.proxy.binding())
                            .placeholder("proxy.example:8080")
                            .enabled(proxy_enabled)
                            .frame(
                                Theme::current().layout.form_control_width,
                                Theme::current().layout.large_control_height,
                            ),
                    ),
                ],
            ),
        ])
    }

    fn security_page(&self) -> Box<dyn View + 'static> {
        let trust = Path::new("/var/lib/certificate/trust-a.json").exists()
            || Path::new("/var/lib/certificate/trust-b.json").exists();
        let revocations = Path::new("/var/lib/certificate/revocations-a.json").exists()
            || Path::new("/var/lib/certificate/revocations-b.json").exists();
        let grants = persistent_grant_count();
        let events = fs::read_to_string("/var/log/audit.log")
            .map(|text| text.lines().count())
            .unwrap_or(0);
        Self::page(vec![
            Self::group(
                "Certificates",
                vec![
                    Self::value_row(
                        "Developer Certificate",
                        "Certificate used to sign applications",
                        "Managed by Kome",
                    ),
                    Self::value_row(
                        "Installed Certificates",
                        "",
                        if trust {
                            "Trust database installed"
                        } else {
                            "No trust snapshot"
                        },
                    ),
                    Self::value_row(
                        "Revocation Status",
                        "",
                        if revocations {
                            "Database available"
                        } else {
                            "Unavailable"
                        },
                    ),
                    Self::value_row(
                        "Trusted Root",
                        "",
                        if trust {
                            "mochiOS Root"
                        } else {
                            "Not installed"
                        },
                    ),
                ],
            ),
            Self::group(
                "Application Security",
                vec![
                    Self::value_row(
                        "Persistent Capabilities",
                        "User-approved application capabilities",
                        format!("{grants} grants"),
                    ),
                    Self::setting_row(
                        "Unsigned Applications",
                        "Choose how unsigned applications are handled",
                        self.segments("unsigned_policy", self.unsigned_policy.binding())
                            .item(0, "Deny")
                            .disabled_item(1, "Ask")
                            .frame(
                                Theme::current().layout.form_control_width,
                                Theme::current().controls.height,
                            ),
                    ),
                ],
            ),
            Self::group(
                "Security Events",
                vec![Self::value_row(
                    "Event History",
                    "Recorded security and policy decisions",
                    format!("{events} events"),
                )],
            ),
            Self::group(
                "Diagnostics & Privacy",
                vec![
                    Self::setting_row(
                        "Share Basic Diagnostics",
                        "Enabled by default; nothing is sent until you explicitly agree",
                        self.switch("diagnostics_enabled", self.diagnostics_enabled.binding()),
                    ),
                    Self::value_row(
                        "Consent",
                        "Crash reports also require confirmation for each crash",
                        if self.diagnostics_enabled.get() && self.diagnostics_consent.get() {
                            "Granted"
                        } else {
                            "Not sending"
                        },
                    ),
                    Self::setting_row(
                        "Diagnostic Consent",
                        "You can change this at any time",
                        Button::new(if self.diagnostics_consent.get() {
                            "Withdraw Consent"
                        } else {
                            "Allow Sharing"
                        })
                        .size(ButtonSize::Small)
                        .style(ButtonStyle::Standard)
                        .enabled(self.diagnostics_enabled.get())
                        .on_click({
                            let consent = self.diagnostics_consent.clone();
                            move || consent.set(!consent.get())
                        }),
                    ),
                ],
            ),
        ])
    }

    fn application_icon(name: &str, icon: Option<ImageData>, size: f32) -> StackChild {
        if let Some(icon) = icon {
            Image::new(icon)
                .content_mode(ImageContentMode::Fit)
                .radius(CornerRadius::Small)
                .frame(size, size)
        } else {
            ApplicationPlaceholder::new(name).frame(size, size)
        }
    }

    fn applications_page(&self) -> Box<dyn View + 'static> {
        if !self.applications_loaded.replace(true) {
            self.applications.set(load_applications());
        }
        let applications = self.applications.get();
        let selected_index = self
            .selected_application
            .get()
            .min(applications.len().saturating_sub(1));
        let query = self.application_search.get().trim().to_ascii_lowercase();
        let mut rows = VStack::new()
            .alignment(StackAlignment::Stretch)
            .gap(StackGap::None);
        let mut visible = 0usize;
        for (index, application) in applications.iter().enumerate() {
            if !query.is_empty()
                && !application.name.to_ascii_lowercase().contains(&query)
                && !application.bundle_id.to_ascii_lowercase().contains(&query)
            {
                continue;
            }
            let selection = self.selected_application.clone();
            let detail_open = self.application_detail_open.clone();
            let page_scroll = self.page_scroll.clone();
            let labels = VStack::new()
                .alignment(StackAlignment::Stretch)
                .gap(StackGap::None)
                .child(Text::styled(application.name.clone(), TextRole::Label).weight(600))
                .child(Self::secondary(application.developer.clone()));
            rows = rows.child(
                Button::new(application.name.clone())
                    .content_padding(false)
                    .content(
                        Padding::symmetric(
                            Theme::current().spacing.medium,
                            Theme::current().spacing.small,
                        )
                        .content(
                            HStack::new()
                                .alignment(StackAlignment::Center)
                                .gap(StackGap::Small)
                                .child(Self::application_icon(
                                    &application.name,
                                    application.icon.clone(),
                                    Theme::current().controls.height,
                                ))
                                .child(labels.layout().flex_grow(1.0))
                                .child(Self::secondary(format!(
                                    "{} grants",
                                    application_grants(&application.executable).len()
                                )))
                                .child(
                                    Icon::new(SymbolName::ChevronRight)
                                        .size(Theme::current().layout.compact_icon_size),
                                ),
                        ),
                    )
                    .style(ButtonStyle::Ghost)
                    .alignment(ZStackAlignment::Leading)
                    .on_click(move || {
                        selection.set(index);
                        detail_open.set(true);
                        page_scroll.reset();
                    })
                    .height(Theme::current().layout.settings_description_row_height)
                    .flex_shrink(0.0),
            );
            visible += 1;
        }
        if visible == 0 {
            rows = rows.child(Self::secondary("No matching applications"));
        }

        let details: StackChild = if let Some(application) = applications.get(selected_index) {
            let grants = application_grants(&application.executable);
            let executable = application.executable.clone();
            let app_name = application.name.clone();
            let status = self.status.clone();
            let revoke = Button::new("Revoke All")
                .size(ButtonSize::Small)
                .style(ButtonStyle::Standard)
                .enabled(!grants.is_empty())
                .on_click(move || {
                    status.set(match revoke_application_grants(&executable) {
                        Ok(count) => format!("Revoked {count} grants for {app_name}."),
                        Err(error) => format!("Unable to revoke capabilities: {error}"),
                    });
                });
            let mut grant_rows = Vec::new();
            if grants.is_empty() {
                grant_rows.push(Self::value_row("Capabilities", "", "No persistent grants"));
            } else {
                for capability in grants {
                    grant_rows.push(Self::value_row(capability, "", "Granted"));
                }
            }
            grant_rows.push(Self::setting_row(
                "Revoke Capabilities",
                "Remove every persistent grant for this application",
                revoke,
            ));
            VStack::new()
                .alignment(StackAlignment::Stretch)
                .gap(StackGap::ExtraLarge)
                .child(
                    HStack::new()
                        .alignment(StackAlignment::Center)
                        .gap(StackGap::Medium)
                        .child(Self::application_icon(
                            &application.name,
                            application.icon.clone(),
                            Theme::current().spacing.triple_extra_large,
                        ))
                        .child(
                            VStack::new()
                                .alignment(StackAlignment::Stretch)
                                .gap(StackGap::None)
                                .child(Text::styled(application.name.clone(), TextRole::TitleSmall))
                                .child(Self::secondary(application.bundle_id.clone()))
                                .child(Self::secondary(application.developer.clone())),
                        ),
                )
                .child(Self::group("Capabilities", grant_rows))
                .into_stack_child()
                .flex_shrink(0.0)
        } else {
            Self::secondary("No applications installed").into_stack_child()
        };

        if self.application_detail_open.get() {
            let open = self.application_detail_open.clone();
            let scroll = self.page_scroll.clone();
            return Box::new(
                VStack::new()
                    .alignment(StackAlignment::Stretch)
                    .gap(StackGap::Large)
                    .child(
                        HStack::new().child(
                            Button::new("All Applications")
                                .content(
                                    HStack::new()
                                        .gap(StackGap::Small)
                                        .child(
                                            Icon::new(SymbolName::ChevronLeft)
                                                .size(Theme::current().layout.compact_icon_size),
                                        )
                                        .child(Text::label("All Applications")),
                                )
                                .style(ButtonStyle::Ghost)
                                .size(ButtonSize::Small)
                                .on_click(move || {
                                    open.set(false);
                                    scroll.reset();
                                }),
                        ),
                    )
                    .child(details),
            );
        }
        Box::new(
            VStack::new()
                .alignment(StackAlignment::Stretch)
                .gap(StackGap::Large)
                .child(
                    TextField::new(self.application_search.binding())
                        .placeholder("Search Applications")
                        .leading_symbol(SymbolName::Search)
                        .height(Theme::current().controls.height),
                )
                .child(Self::group(
                    "Installed Applications",
                    vec![rows.into_stack_child()],
                )),
        )
    }

    fn default_apps_page(&self) -> Box<dyn View + 'static> {
        if !self.associations_loaded.replace(true) {
            let applications = load_applications();
            self.file_associations
                .set(load_file_associations(&applications));
        }

        let mut rows = Vec::new();
        for association in self.file_associations.get() {
            let current = association
                .handlers
                .iter()
                .find(|handler| handler.bundle_id == association.default_bundle_id)
                .map(|handler| handler.name.clone())
                .unwrap_or_else(|| String::from("Not set"));
            let mut picker = Picker::new(current).radius(CornerRadius::Small);
            for handler in association.handlers {
                let extension = association.extension.clone();
                let bundle_id = handler.bundle_id.clone();
                let associations = self.file_associations.clone();
                let status = self.status.clone();
                picker = picker.option(handler.name, move || {
                    match set_default_application(&extension, &bundle_id) {
                        Ok(()) => {
                            let mut updated = associations.get();
                            if let Some(item) =
                                updated.iter_mut().find(|item| item.extension == extension)
                            {
                                item.default_bundle_id = bundle_id.clone();
                            }
                            associations.set(updated);
                            status.set(format!(
                                ".{extension} files will now open with the selected application."
                            ));
                        }
                        Err(error) => {
                            status.set(format!("Unable to change the default application: {error}"))
                        }
                    }
                });
            }
            rows.push(Self::setting_row(
                format!(".{}", association.extension),
                "Default application",
                picker.frame(
                    Theme::current().layout.form_control_width,
                    Theme::current().controls.height,
                ),
            ));
        }
        if rows.is_empty() {
            rows.push(Self::value_row(
                "File Associations",
                "Install an application that declares supported document types",
                "No supported file types",
            ));
        }
        Self::page(vec![Self::group("File Types", rows)])
    }

    fn current_preferences(&self) -> Preferences {
        Preferences {
            device_name: self.device_name.get(),
            language: self.language.get(),
            region: self.region.get(),
            timezone: self.timezone.get(),
            automatic_time: self.automatic_time.get(),
            appearance: self.appearance.get(),
            accent: self.accent.get(),
            wallpaper: self.wallpaper.get(),
            ui_scale: self.ui_scale.get(),
            font_size: self.font_size.get(),
            keyboard_layout: self.keyboard_layout.get(),
            repeat_delay: self.repeat_delay.get(),
            repeat_rate: self.repeat_rate.get(),
            mouse_speed: self.mouse_speed.get(),
            natural_scrolling: self.natural_scrolling.get(),
            touchpad_tap: self.touchpad_tap.get(),
            ethernet_enabled: self.ethernet_enabled.get(),
            wifi_enabled: self.wifi_enabled.get(),
            network_mode: self.network_mode.get(),
            ip_address: self.ip_address.get(),
            dns_server: self.dns_server.get(),
            proxy: self.proxy.get(),
            proxy_enabled: self.proxy_enabled.get(),
            auto_login: self.auto_login.get(),
            auto_login_user: self.auto_login_user.get(),
            unsigned_policy: self.unsigned_policy.get(),
            diagnostics_enabled: self.diagnostics_enabled.get(),
            diagnostics_consent: self.diagnostics_enabled.get() && self.diagnostics_consent.get(),
        }
    }
}

impl Drop for SettingsApp {
    fn drop(&mut self) {
        let snapshot = self.current_preferences();
        let persisted = self.autosave.borrow().persisted.clone();
        if snapshot != persisted {
            if let Err(error) = snapshot.save_changed(&persisted) {
                eprintln!("Unable to save settings on close: {error}");
            }
        }
    }
}

impl SettingsApp {
    fn from_preferences(preferences: Preferences) -> Self {
        Self {
            switch_interactions: RefCell::new(BTreeMap::new()),
            slider_interactions: RefCell::new(BTreeMap::new()),
            segment_interactions: RefCell::new(BTreeMap::new()),
            autosave: Rc::new(RefCell::new(AutosaveState {
                persisted: preferences.clone(),
                pending: None,
                failed: None,
            })),
            users_loaded: Cell::new(false),
            network_loaded: Cell::new(false),
            applications_loaded: Cell::new(false),
            associations_loaded: Cell::new(false),
            notifications_loaded: Cell::new(false),
            section: State::new(Section::General.index()),
            search: State::new(String::new()),
            status: State::new(String::new()),
            users: State::new(Vec::new()),
            selected_user: State::new(0),
            new_name: State::new(String::new()),
            new_display_name: State::new(String::new()),
            new_user_password: State::new(String::new()),
            password: State::new(String::new()),
            auto_login: State::new(preferences.auto_login),
            auto_login_user: State::new(preferences.auto_login_user),
            device_name: State::new(preferences.device_name),
            language: State::new(preferences.language),
            region: State::new(preferences.region),
            timezone: State::new(preferences.timezone),
            automatic_time: State::new(preferences.automatic_time),
            appearance: State::new(preferences.appearance),
            accent: State::new(preferences.accent),
            wallpaper: State::new(preferences.wallpaper),
            ui_scale: State::new(preferences.ui_scale),
            font_size: State::new(preferences.font_size),
            keyboard_layout: State::new(preferences.keyboard_layout),
            repeat_delay: State::new(preferences.repeat_delay),
            repeat_rate: State::new(preferences.repeat_rate),
            mouse_speed: State::new(preferences.mouse_speed),
            natural_scrolling: State::new(preferences.natural_scrolling),
            touchpad_tap: State::new(preferences.touchpad_tap),
            ethernet_enabled: State::new(preferences.ethernet_enabled),
            wifi_enabled: State::new(preferences.wifi_enabled),
            wifi_status: State::new(wifi::WifiStatus::default()),
            wifi_networks: State::new(Vec::new()),
            wifi_selected: State::new(0),
            wifi_password: State::new(String::new()),
            network_mode: State::new(preferences.network_mode),
            ip_address: State::new(preferences.ip_address),
            dns_server: State::new(preferences.dns_server),
            proxy_enabled: State::new(preferences.proxy_enabled),
            proxy: State::new(preferences.proxy),
            unsigned_policy: State::new(preferences.unsigned_policy),
            diagnostics_enabled: State::new(preferences.diagnostics_enabled),
            diagnostics_consent: State::new(preferences.diagnostics_consent),
            notification_focus_enabled: State::new(false),
            notification_disabled_bundle_ids: State::new(Vec::new()),
            applications: State::new(Vec::new()),
            selected_application: State::new(0),
            application_detail_open: State::new(false),
            application_search: State::new(String::new()),
            file_associations: State::new(Vec::new()),
            page_scroll: ScrollState::new(),
            sidebar_scroll: ScrollState::new(),
        }
    }
}

impl App for SettingsApp {
    type Body = Box<dyn View + 'static>;

    fn new() -> Self {
        Self::from_preferences(Preferences::load())
    }

    fn window(&self) -> WindowOptions {
        WindowOptions::new("Settings")
            .size(1040.0, 700.0)
            .resizable(true)
    }

    fn body(&self, _context: &ViewContext) -> Self::Body {
        self.settings_view()
    }
}

impl SettingsApp {
    fn settings_view(&self) -> Box<dyn View> {
        let page = match Section::from_index(self.section.get()) {
            Section::Account => self.account_page(),
            Section::General => self.general_page(),
            Section::Appearance => self.appearance_page(),
            Section::Input => self.input_page(),
            Section::Notifications => self.notifications_page(),
            Section::Network => self.network_page(),
            Section::Security => self.security_page(),
            Section::DefaultApps => self.default_apps_page(),
            Section::Applications => self.applications_page(),
        };
        let mut detail = VStack::new()
            .alignment(StackAlignment::Stretch)
            .gap(StackGap::None)
            .child(
                Scroll::new(self.page_scroll.clone())
                    .axis(ScrollAxis::Vertical)
                    .scrollbar(ScrollBarVisibility::Automatic)
                    .content(
                        ContentArea::pane(
                            VStack::new()
                                .alignment(StackAlignment::Stretch)
                                .gap(StackGap::ExtraLarge)
                                .child(self.detail_header())
                                .child(page),
                        ),
                    )
                    .layout()
                    .flex_grow(1.0)
                    .flex_shrink(1.0),
            );
        if !self.status.get().is_empty() {
            detail = detail.child(self.status_bar());
        }
        Box::new(AutosaveLayer {
            content: NavigationSplitView::new(
                Scroll::new(self.sidebar_scroll.clone())
                    .axis(ScrollAxis::Vertical)
                    .content(self.sidebar()),
                detail,
            )
            .flexible_sidebar(
                Theme::current().layout.compact_form_control_width,
                Theme::current().layout.navigation_sidebar_width,
                Theme::current().layout.navigation_sidebar_width,
            )
            .minimum_detail_width(Theme::current().layout.form_width)
            .shows_divider(false),
            snapshot: self.current_preferences(),
            state: Rc::clone(&self.autosave),
            status: self.status.clone(),
            consent: self.diagnostics_consent.clone(),
        })
    }
}

fn load_applications() -> Vec<ApplicationInfo> {
    let root = Path::new("/applications");
    #[cfg(target_os = "linux")]
    let preview_root = std::env::var_os("MOCHIOS_PREVIEW_APPS").map(std::path::PathBuf::from);
    #[cfg(target_os = "linux")]
    let root = preview_root.as_deref().unwrap_or(root);
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut applications = Vec::new();
    for entry in entries.flatten() {
        let app_root = entry.path();
        if !app_root.is_dir() {
            continue;
        }
        let Ok(content) = fs::read_to_string(app_root.join("manifest.toml")) else {
            continue;
        };
        let Some(name) = parse_string_field(&content, "name") else {
            continue;
        };
        let Some(bundle_id) = parse_string_field(&content, "id") else {
            continue;
        };
        let Some(entry_name) = parse_string_field(&content, "entry") else {
            continue;
        };
        let developer = parse_string_field(&content, "vendor")
            .unwrap_or_else(|| String::from("Unknown developer"));
        let icon = parse_string_field(&content, "icon")
            .and_then(|icon_name| load_application_icon(&app_root.join(icon_name)));
        let document_extensions = parse_string_array_field(&content, "document_extensions")
            .into_iter()
            .map(|extension| extension.trim_start_matches('.').to_ascii_lowercase())
            .filter(|extension| valid_extension(extension))
            .collect();
        applications.push(ApplicationInfo {
            name,
            bundle_id,
            developer,
            executable: app_root.join(entry_name).to_string_lossy().into_owned(),
            icon,
            document_extensions,
        });
    }
    applications.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
            .then_with(|| left.bundle_id.cmp(&right.bundle_id))
    });
    applications
}

fn load_file_associations(applications: &[ApplicationInfo]) -> Vec<FileAssociationSetting> {
    let mut extensions = applications
        .iter()
        .flat_map(|application| application.document_extensions.iter().cloned())
        .collect::<Vec<_>>();
    extensions.sort();
    extensions.dedup();
    extensions
        .into_iter()
        .filter_map(|extension| {
            let handlers = applications
                .iter()
                .filter(|application| application.document_extensions.contains(&extension))
                .map(|application| AssociationApplication {
                    name: application.name.clone(),
                    bundle_id: application.bundle_id.clone(),
                })
                .collect::<Vec<_>>();
            if handlers.is_empty() {
                return None;
            }
            let default_bundle_id = resolve_default_application(&extension)
                .filter(|bundle_id| {
                    handlers
                        .iter()
                        .any(|handler| &handler.bundle_id == bundle_id)
                })
                .unwrap_or_else(|| handlers[0].bundle_id.clone());
            Some(FileAssociationSetting {
                extension,
                handlers,
                default_bundle_id,
            })
        })
        .collect()
}

fn content_type_for_extension(extension: &str) -> &'static str {
    match extension {
        "json" => "application/json",
        "toml" => "application/toml",
        "xml" => "application/xml",
        "csv" => "text/csv",
        "md" | "markdown" => "text/markdown",
        "c" | "h" => "text/x-c",
        "cc" | "cpp" | "cxx" | "hh" | "hpp" => "text/x-c++",
        "rs" => "text/x-rust",
        "sh" => "text/x-shellscript",
        "yaml" | "yml" => "text/yaml",
        _ => "text/plain",
    }
}

#[cfg(target_os = "mochios")]
fn resolve_default_application(extension: &str) -> Option<String> {
    mochi_user_platform::workspace::resolve_association(
        extension,
        content_type_for_extension(extension),
        mochi_user_platform::workspace::ASSOCIATION_ROLE_EDIT,
    )
    .ok()
}

#[cfg(not(target_os = "mochios"))]
fn resolve_default_application(_extension: &str) -> Option<String> {
    None
}

#[cfg(target_os = "mochios")]
fn set_default_application(extension: &str, bundle_id: &str) -> Result<(), String> {
    mochi_user_platform::workspace::set_association(
        extension,
        content_type_for_extension(extension),
        bundle_id,
        mochi_user_platform::workspace::ASSOCIATION_ROLE_EDIT,
    )
    .map_err(|error| format!("{error:?}"))
}

#[cfg(not(target_os = "mochios"))]
fn set_default_application(_extension: &str, _bundle_id: &str) -> Result<(), String> {
    Ok(())
}

fn load_application_icon(path: &Path) -> Option<ImageData> {
    if path.extension().and_then(|extension| extension.to_str()) == Some("svg") {
        let svg = SvgData::from_path(path).ok()?;
        return ImageData::from_svg(&svg, 72, 72).ok();
    }
    ImageData::thumbnail_from_path(path, 72, 72).ok()
}

fn parse_string_field(content: &str, key: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let (candidate, value) = line.split_once('=')?;
        if candidate.trim() != key {
            return None;
        }
        let value = value.trim();
        value
            .strip_prefix('"')?
            .strip_suffix('"')
            .map(ToOwned::to_owned)
    })
}

fn parse_string_array_field(content: &str, key: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut collecting = false;
    for line in content.lines().map(str::trim) {
        if !collecting {
            let Some((candidate, remainder)) = line.split_once('=') else {
                continue;
            };
            if candidate.trim() != key || !remainder.contains('[') {
                continue;
            }
            collecting = true;
        }
        let mut remaining = line;
        while let Some(start) = remaining.find('"') {
            let after = &remaining[start + 1..];
            let Some(end) = after.find('"') else {
                break;
            };
            values.push(after[..end].to_owned());
            remaining = &after[end + 1..];
        }
        if collecting && line.contains(']') {
            break;
        }
    }
    values
}

fn valid_extension(extension: &str) -> bool {
    !extension.is_empty()
        && extension.len() <= 63
        && extension
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn application_grants(executable: &str) -> Vec<String> {
    let Ok(text) = fs::read_to_string(GRANTS_PATH) else {
        return Vec::new();
    };
    let mut grants = Vec::new();
    for line in text.lines() {
        let mut fields = line.split('\t');
        if fields.next() != Some(executable) {
            continue;
        }
        let _digest = fields.next();
        let Some(capability) = fields.next() else {
            continue;
        };
        if !grants.iter().any(|candidate| candidate == capability) {
            grants.push(capability.to_owned());
        }
    }
    grants.sort();
    grants
}

fn persistent_grant_count() -> usize {
    fs::read_to_string(GRANTS_PATH)
        .map(|text| text.lines().filter(|line| !line.is_empty()).count())
        .unwrap_or(0)
}

fn revoke_application_grants(executable: &str) -> std::io::Result<usize> {
    let text = fs::read_to_string(GRANTS_PATH)?;
    let mut kept = String::new();
    let mut removed = 0usize;
    for line in text.lines() {
        if line.split('\t').next() == Some(executable) {
            removed += 1;
            continue;
        }
        kept.push_str(line);
        kept.push('\n');
    }
    if removed != 0 {
        fs::write(GRANTS_PATH, kept)?;
    }
    Ok(removed)
}

fn current_datetime() -> String {
    let Ok(elapsed) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return String::from("Unavailable");
    };
    let Ok(seconds) = i64::try_from(elapsed.as_secs()) else {
        return String::from("Unavailable");
    };
    let days = seconds.div_euclid(86_400);
    let seconds_in_day = seconds.rem_euclid(86_400);
    let Some((year, month, day)) = civil_date(days) else {
        return String::from("Unavailable");
    };
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
        seconds_in_day / 3_600,
        (seconds_in_day % 3_600) / 60,
    )
}

fn civil_date(days: i64) -> Option<(i64, i64, i64)> {
    let shifted = days.checked_add(719_468)?;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_phase = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_phase + 2) / 5 + 1;
    let month = month_phase + if month_phase < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    Some((year, month, day))
}

fn network_service_available() -> bool {
    #[cfg(target_os = "mochios")]
    {
        return mochi_user_platform::process::find_by_name("network.service")
            .is_ok_and(|endpoint| endpoint != 0);
    }
    #[cfg(not(target_os = "mochios"))]
    false
}

fn main() -> Result<(), ViewKitError> {
    run::<SettingsApp>()
}

#[cfg(test)]
mod tests {
    use super::{Section, civil_date, parse_string_field, section_matches_search};

    #[test]
    fn settings_search_matches_category_names_and_descriptions() {
        assert!(section_matches_search(Section::General, "general"));
        assert!(section_matches_search(Section::General, "DEVICE"));
        assert!(section_matches_search(Section::Security, "execution"));
        assert!(!section_matches_search(Section::Appearance, "network"));
    }

    #[test]
    fn civil_date_handles_epoch_and_leap_day() {
        assert_eq!(civil_date(0), Some((1970, 1, 1)));
        assert_eq!(civil_date(19_782), Some((2024, 2, 29)));
    }

    #[test]
    fn application_metadata_parser_requires_quoted_values() {
        assert_eq!(
            parse_string_field("name = \"Settings\"\nversion = 1", "name"),
            Some(String::from("Settings"))
        );
        assert_eq!(parse_string_field("name = Settings", "name"), None);
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    use viewkit::{
        accessibility::{AccessibilityNode, AccessibilityRole},
        draw_command::{DisplayList, DrawCommand},
        typography::TextMeasurer,
    };

    fn paint(app: &SettingsApp, width: f32, theme: &Theme) -> (DisplayList, Vec<AccessibilityNode>) {
        let bounds = Rect::new(0.0, 0.0, width, 460.0);
        let mut list = DisplayList::new();
        let mut nodes = Vec::new();
        let mut text = TextMeasurer::new();
        Surface::app().content(app.settings_view()).paint(
            bounds,
            &mut PaintContext::new(&mut list, theme, &theme.typography, &mut text)
                .with_viewport_bounds(bounds)
                .with_accessibility_nodes(&mut nodes),
        );
        (list, nodes)
    }

    fn text_bounds(list: &DisplayList, text: &str, minimum_x: f32) -> Rect {
        list.commands().iter().find_map(|cmd| match cmd {
            DrawCommand::DrawText { command }
                if command.text == text && command.bounds.origin.x >= minimum_x => Some(command.bounds),
            _ => None,
        }).expect("expected text to be present in the layout")
    }

    #[test]
    fn two_panes_start_at_the_top_without_toolbar_or_overlaid_background() {
        let app = SettingsApp::from_preferences(Preferences::default());
        for theme in [Theme::LIGHT, Theme::DARK] {
            for width in [800.0, 1040.0] {
                let (list, nodes) = paint(&app, width, &theme);
                assert!(!nodes.iter().any(|node| node.role == AccessibilityRole::Toolbar));
                let sidebar_title = text_bounds(&list, "Settings", 0.0);
                let heading = text_bounds(&list, "General", theme.layout.navigation_sidebar_width);
                assert!(sidebar_title.origin.y < theme.layout.top_bar_height);
                assert!(heading.origin.y < theme.layout.top_bar_height);
                // The pane is full-height, with no top chrome corner to round.
                let pane = list.commands().iter().find_map(|cmd| match cmd {
                    DrawCommand::FillRect { rect, color }
                        if rect.origin.x > 0.0 && rect.origin.y == 0.0
                            && rect.size.height == 460.0 && *color == theme.surface.pane_background => Some(*rect),
                    _ => None,
                }).expect("a standard detail pane must reach the top edge");
                assert_eq!(list.commands().iter().rev().find_map(|cmd| match cmd {
                    DrawCommand::FillRect { rect, color } if *rect == pane => Some(*color),
                    _ => None,
                }), Some(theme.surface.pane_background));
                assert_eq!(heading.origin.x, pane.origin.x + theme.spacing.extra_large);
            }
        }
    }

    #[test]
    fn page_heading_scrolls_with_settings_while_sidebar_stays_in_place() {
        let app = SettingsApp::from_preferences(Preferences::default());
        let theme = Theme::LIGHT;
        let (before, _) = paint(&app, 1040.0, &theme);
        app.page_scroll.set_offset(0.0, 12.0);
        let (after, _) = paint(&app, 1040.0, &theme);
        let minimum_x = theme.layout.navigation_sidebar_width;
        let before_heading = text_bounds(&before, "General", minimum_x);
        let after_heading = text_bounds(&after, "General", minimum_x);
        assert_eq!(before_heading.origin.y - after_heading.origin.y, 12.0);
        assert_eq!(text_bounds(&before, "Settings", 0.0), text_bounds(&after, "Settings", 0.0));
        assert_eq!(app.current_preferences(), app.autosave.borrow().persisted);
    }
}
