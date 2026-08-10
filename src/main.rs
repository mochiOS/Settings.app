mod accounts;
mod preferences;

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use mochios_user_database::UserRecord;
use preferences::Preferences;
use viewkit::prelude::*;

const WINDOW_WIDTH: f32 = 980.0;
const WINDOW_HEIGHT: f32 = 650.0;
const TOOLBAR_HEIGHT: f32 = 54.0;
const STATUS_HEIGHT: f32 = 28.0;
const SIDEBAR_WIDTH: f32 = 210.0;
const CONTROL_WIDTH: f32 = 260.0;
const GRANTS_PATH: &str = "/system/policy/capability-grants.db";

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
    BUILD_METADATA.split('\n').nth(index).unwrap_or("unavailable")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Account,
    General,
    Appearance,
    Input,
    Network,
    Security,
    Applications,
}

impl Section {
    const PRIMARY: [Self; 5] = [
        Self::Account,
        Self::General,
        Self::Appearance,
        Self::Input,
        Self::Network,
    ];
    const SYSTEM: [Self; 2] = [Self::Security, Self::Applications];

    const fn index(self) -> usize {
        match self {
            Self::Account => 0,
            Self::General => 1,
            Self::Appearance => 2,
            Self::Input => 3,
            Self::Network => 4,
            Self::Security => 5,
            Self::Applications => 6,
        }
    }

    const fn from_index(index: usize) -> Self {
        match index {
            0 => Self::Account,
            1 => Self::General,
            2 => Self::Appearance,
            3 => Self::Input,
            4 => Self::Network,
            5 => Self::Security,
            6 => Self::Applications,
            _ => Self::General,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Account => "Account",
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Input => "Input",
            Self::Network => "Network",
            Self::Security => "Security",
            Self::Applications => "Applications",
        }
    }

    const fn description(self) -> &'static str {
        match self {
            Self::Account => "Users, passwords, and sign-in options",
            Self::General => "Device, language, region, date, and system information",
            Self::Appearance => "Theme, accent, wallpaper, and interface sizing",
            Self::Input => "Keyboard, mouse, touchpad, and shortcuts",
            Self::Network => "Ethernet, Wi-Fi, addressing, DNS, and proxy",
            Self::Security => "Certificates, trust, execution policy, and events",
            Self::Applications => "Review and revoke application capabilities",
        }
    }

    const fn icon(self) -> IconName {
        match self {
            Self::Account => IconName::House,
            Self::General => IconName::Settings,
            Self::Appearance => IconName::Eye,
            Self::Input => IconName::AppWindow,
            Self::Network => IconName::HardDrive,
            Self::Security => IconName::FileText,
            Self::Applications => IconName::LayoutGrid,
        }
    }
}

#[derive(Clone)]
struct ApplicationInfo {
    name: String,
    bundle_id: String,
    developer: String,
    executable: String,
    icon: Option<ImageData>,
}

struct SettingsApp {
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
    network_mode: State<usize>,
    ip_address: State<String>,
    dns_server: State<String>,
    proxy_enabled: State<bool>,
    proxy: State<String>,
    unsigned_policy: State<usize>,
    applications: State<Vec<ApplicationInfo>>,
    selected_application: State<usize>,
    page_scroll: ScrollState,
}

impl SettingsApp {
    fn secondary(text: impl Into<String>) -> Text {
        Text::new(text.into())
            .font_size(11.0)
            .line_height(18.0)
            .color(Theme::DEFAULT.colors.text_secondary)
    }

    fn page_header(section: Section) -> StackChild {
        VStack::new()
            .alignment(StackAlignment::Stretch)
            .gap(StackGap::ExtraSmall)
            .child(
                Text::new(section.label())
                    .font_size(26.0)
                    .line_height(34.0)
                    .weight(700),
            )
            .child(
                Text::new(section.description())
                    .font_size(12.0)
                    .line_height(19.0)
                    .color(Theme::DEFAULT.colors.text_secondary),
            )
            .into_stack_child()
            .flex_shrink(0.0)
    }

    fn setting_row<C>(
        title: impl Into<String>,
        description: impl Into<String>,
        control: C,
    ) -> StackChild
    where
        C: IntoStackChild,
    {
        let description = description.into();
        let has_description = !description.is_empty();
        let mut labels = VStack::new()
            .alignment(StackAlignment::Stretch)
            .gap(StackGap::None)
            .child(
                Text::new(title.into())
                    .font_size(13.0)
                    .line_height(20.0)
                    .weight(600),
            );
        if has_description {
            labels = labels.child(Self::secondary(description));
        }
        Padding::symmetric(0.0, 7.0)
            .content(
                HStack::new()
                    .alignment(StackAlignment::Center)
                    .distribution(StackDistribution::SpaceBetween)
                    .gap(StackGap::Large)
                    .child(labels.layout().flex_grow(1.0))
                    .child(control),
            )
            .height(if has_description { 58.0 } else { 48.0 })
            .flex_shrink(0.0)
    }

    fn value_row(
        title: impl Into<String>,
        description: impl Into<String>,
        value: impl Into<String>,
    ) -> StackChild {
        Self::setting_row(
            title,
            description,
            Text::new(value.into())
                .font_size(12.0)
                .line_height(20.0)
                .alignment(TextAlignment::End)
                .color(Theme::DEFAULT.colors.text_secondary),
        )
    }

    fn group(title: impl Into<String>, rows: Vec<StackChild>) -> StackChild {
        let count = rows.len();
        let mut body = VStack::new()
            .alignment(StackAlignment::Stretch)
            .gap(StackGap::None);
        for (index, row) in rows.into_iter().enumerate() {
            body = body.child(row);
            if index + 1 != count {
                body = body.child(Divider::new());
            }
        }
        VStack::new()
            .alignment(StackAlignment::Stretch)
            .gap(StackGap::Small)
            .child(
                Text::new(title.into())
                    .font_size(11.0)
                    .line_height(18.0)
                    .weight(600)
                    .color(Theme::DEFAULT.colors.text_secondary),
            )
            .child(body)
            .into_stack_child()
            .flex_shrink(0.0)
    }

    fn page(section: Section, groups: Vec<StackChild>) -> Box<dyn View + 'static> {
        let mut content = VStack::new()
            .alignment(StackAlignment::Stretch)
            .gap(StackGap::ExtraLarge)
            .child(Self::page_header(section));
        for group in groups {
            content = content.child(group);
        }
        Box::new(content)
    }

    fn field(value: State<String>, placeholder: &'static str) -> StackChild {
        TextField::new(value.binding())
            .placeholder(placeholder)
            .size(TextFieldSize::Medium)
            .frame(CONTROL_WIDTH, 36.0)
    }

    fn secure_field(value: State<String>, placeholder: &'static str) -> StackChild {
        TextField::new(value.binding())
            .placeholder(placeholder)
            .size(TextFieldSize::Medium)
            .secure(true)
            .frame(190.0, 36.0)
    }

    fn navigation_button(&self, section: Section) -> StackChild {
        let selected = Section::from_index(self.section.get()) == section;
        let section_state = self.section.clone();
        let search_state = self.search.clone();
        let page_scroll = self.page_scroll.clone();
        let foreground = if selected {
            Theme::DEFAULT.colors.text_primary
        } else {
            Theme::DEFAULT.colors.text_secondary
        };
        Button::new(section.label())
            .content(
                HStack::new()
                    .alignment(StackAlignment::Center)
                    .gap(StackGap::Small)
                    .child(Icon::new(section.icon()).size(18.0).color(foreground))
                    .child(
                        Text::new(section.label())
                            .font_size(13.0)
                            .line_height(20.0)
                            .weight(if selected { 600 } else { 500 })
                            .color(foreground),
                    ),
            )
            .style(if selected {
                ButtonStyle::Standard
            } else {
                ButtonStyle::Ghost
            })
            .alignment(ZStackAlignment::Leading)
            .radius(CornerRadius::Custom(7.0))
            .on_click(move || {
                section_state.set(section.index());
                search_state.set(String::new());
                page_scroll.reset();
            })
            .height(32.0)
    }

    fn navigation_group(&self, title: &'static str, sections: &[Section]) -> StackChild {
        let mut rows = VStack::new()
            .alignment(StackAlignment::Stretch)
            .gap(StackGap::ExtraSmall);
        for section in sections.iter().copied() {
            rows = rows.child(self.navigation_button(section));
        }
        VStack::new()
            .alignment(StackAlignment::Stretch)
            .gap(StackGap::ExtraSmall)
            .child(
                Text::new(title)
                    .font_size(10.0)
                    .line_height(16.0)
                    .color(Theme::DEFAULT.colors.text_secondary),
            )
            .child(rows)
            .into_stack_child()
    }

    fn sidebar(&self) -> StackChild {
        Background::new()
            .background(
                Rectangle::new()
                    .color(RectangleColor::Custom(Theme::DEFAULT.colors.surface_subtle)),
            )
            .content(
                Padding::all(14.0).content(
                    VStack::new()
                        .alignment(StackAlignment::Stretch)
                        .gap(StackGap::Large)
                        .child(self.navigation_group("Settings", &Section::PRIMARY))
                        .child(self.navigation_group("System", &Section::SYSTEM))
                        .child(Spacer::new())
                        .child(
                            Text::new(format!("mochiOS {}", build_metadata(0)))
                                .font_size(10.0)
                                .line_height(16.0)
                                .color(Theme::DEFAULT.colors.text_secondary),
                        ),
                ),
            )
            .width(SIDEBAR_WIDTH)
    }

    fn toolbar(&self) -> StackChild {
        let current = Section::from_index(self.section.get());
        let current_index = current.index();
        let previous_section = self.section.clone();
        let previous_search = self.search.clone();
        let previous_scroll = self.page_scroll.clone();
        let next_section = self.section.clone();
        let next_search = self.search.clone();
        let next_scroll = self.page_scroll.clone();
        let preferences = self.current_preferences();
        let save_status = self.status.clone();
        let left = HStack::new()
            .alignment(StackAlignment::Center)
            .gap(StackGap::ExtraSmall)
            .child(
                Button::new("")
                    .content(Icon::new(IconName::ChevronLeft).size(18.0))
                    .style(ButtonStyle::Ghost)
                    .enabled(current_index > 0)
                    .on_click(move || {
                        previous_search.set(String::new());
                        previous_section.set(current_index.saturating_sub(1));
                        previous_scroll.reset();
                    })
                    .frame(32.0, 32.0),
            )
            .child(
                Button::new("")
                    .content(Icon::new(IconName::ChevronRight).size(18.0))
                    .style(ButtonStyle::Ghost)
                    .enabled(current_index < Section::Applications.index())
                    .on_click(move || {
                        next_search.set(String::new());
                        next_section.set((current_index + 1).min(Section::Applications.index()));
                        next_scroll.reset();
                    })
                    .frame(32.0, 32.0),
            )
            .width(150.0)
            .flex_shrink(0.0);
        let right = HStack::new()
            .alignment(StackAlignment::Center)
            .gap(StackGap::Small)
            .child(
                TextField::new(self.search.binding())
                    .placeholder("Search")
                    .size(TextFieldSize::Small)
                    .frame(190.0, 32.0),
            )
            .child(
                Button::new("Save")
                    .style(ButtonStyle::Accent)
                    .on_click(move || {
                        save_status.set(match preferences.save() {
                            Ok(()) => String::from(
                                "Settings saved. Sign out or restart to apply system changes.",
                            ),
                            Err(error) => format!("Unable to save settings: {error}"),
                        });
                    })
                    .frame(64.0, 32.0),
            );
        Background::new()
            .background(
                Rectangle::new()
                    .color(RectangleColor::Custom(Theme::DEFAULT.colors.surface_subtle)),
            )
            .content(
                Padding::symmetric(14.0, 10.0).content(
                    HStack::new()
                        .alignment(StackAlignment::Center)
                        .gap(StackGap::Medium)
                        .child(left)
                        .child(
                            Text::new(current.label())
                                .font_size(14.0)
                                .line_height(20.0)
                                .weight(600)
                                .alignment(TextAlignment::Center)
                                .layout()
                                .flex_grow(1.0),
                        )
                        .child(right),
                ),
            )
            .height(TOOLBAR_HEIGHT)
    }

    fn status_bar(&self) -> StackChild {
        let status = self.status.get();
        let left = if status.is_empty() {
            String::from("7 settings categories")
        } else {
            status
        };
        Background::new()
            .background(
                Rectangle::new()
                    .color(RectangleColor::Custom(Theme::DEFAULT.colors.surface_subtle)),
            )
            .content(
                Padding::symmetric(12.0, 4.0).content(
                    HStack::new()
                        .alignment(StackAlignment::Center)
                        .distribution(StackDistribution::SpaceBetween)
                        .child(Self::secondary(left))
                        .child(Self::secondary("mochiOS")),
                ),
            )
            .height(STATUS_HEIGHT)
    }

    fn account_page(&self) -> Box<dyn View + 'static> {
        let users = self.users.get();
        let selected = self.selected_user.get().min(users.len().saturating_sub(1));
        let selected_name = users.get(selected).map(|user| user.name.clone());
        let mut user_rows = Vec::new();
        for (index, user) in users.iter().enumerate() {
            let selection = self.selected_user.clone();
            user_rows.push(
                Button::new(user.display_name.clone())
                    .content(
                        Padding::symmetric(0.0, 7.0).content(
                            HStack::new()
                                .alignment(StackAlignment::Center)
                                .gap(StackGap::Medium)
                                .child(Icon::new(IconName::House).size(20.0).frame(24.0, 24.0))
                                .child(
                                    VStack::new()
                                        .alignment(StackAlignment::Stretch)
                                        .gap(StackGap::None)
                                        .child(
                                            Text::new(user.display_name.clone())
                                                .font_size(13.0)
                                                .line_height(20.0)
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
                    .height(54.0),
            );
        }
        if user_rows.is_empty() {
            user_rows.push(Self::value_row("Users", "", "No users available"));
        }

        let change_name = selected_name.clone();
        let change_password = self.password.clone();
        let change_status = self.status.clone();
        let change_button = Button::new("Change")
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
            })
            .frame(78.0, 32.0);

        let remove_name = selected_name.clone();
        let remove_users = self.users.clone();
        let remove_status = self.status.clone();
        let remove_button = Button::new("Delete User")
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
            })
            .frame(104.0, 32.0);

        let add_users = self.users.clone();
        let add_name = self.new_name.clone();
        let add_display = self.new_display_name.clone();
        let add_password = self.new_user_password.clone();
        let add_status = self.status.clone();
        let add_button = Button::new("Add User")
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
            })
            .frame(88.0, 32.0);

        Self::page(
            Section::Account,
            vec![
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
                            Switch::new(self.auto_login.binding()),
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
            ],
        )
    }

    fn general_page(&self) -> Box<dyn View + 'static> {
        Self::page(
            Section::General,
            vec![
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
                            Self::field(self.language.clone(), "Language"),
                        ),
                        Self::setting_row(
                            "Region",
                            "Date, number, and measurement formats",
                            Self::field(self.region.clone(), "Region"),
                        ),
                    ],
                ),
                Self::group(
                    "Date & Time",
                    vec![
                        Self::setting_row(
                            "Set Automatically",
                            "Synchronize date and time over the network",
                            Switch::new(self.automatic_time.binding()),
                        ),
                        Self::setting_row(
                            "Time Zone",
                            "Current system time zone",
                            Self::field(self.timezone.clone(), "Time zone"),
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
            ],
        )
    }

    fn appearance_page(&self) -> Box<dyn View + 'static> {
        Self::page(
            Section::Appearance,
            vec![
                Self::group(
                    "Appearance",
                    vec![
                        Self::setting_row(
                            "Theme",
                            "Choose how windows and controls are displayed",
                            SegmentedControl::new(self.appearance.binding())
                                .item(0, "Light")
                                .item(1, "Dark")
                                .item(2, "System")
                                .frame(280.0, 34.0),
                        ),
                        Self::setting_row(
                            "Accent Color",
                            "Color used for selected controls",
                            SegmentedControl::new(self.accent.binding())
                                .item(0, "Blue")
                                .item(1, "Purple")
                                .item(2, "Pink")
                                .item(3, "Red")
                                .item(4, "Green")
                                .item(5, "Graphite")
                                .frame(340.0, 34.0),
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
                            Slider::new(self.ui_scale.binding())
                                .range(0.75..=2.0)
                                .step(0.05)
                                .frame(CONTROL_WIDTH, 32.0),
                        ),
                        Self::setting_row(
                            "Font Size",
                            "Default interface text size",
                            Slider::new(self.font_size.binding())
                                .range(10.0..=24.0)
                                .step(1.0)
                                .frame(CONTROL_WIDTH, 32.0),
                        ),
                    ],
                ),
            ],
        )
    }

    fn input_page(&self) -> Box<dyn View + 'static> {
        let shortcut_status = self.status.clone();
        Self::page(
            Section::Input,
            vec![
                Self::group(
                    "Keyboard",
                    vec![
                        Self::setting_row(
                            "Keyboard Layout",
                            "Layout used for physical keyboard input",
                            SegmentedControl::new(self.keyboard_layout.binding())
                                .item(0, "US")
                                .item(1, "Japanese")
                                .item(2, "British")
                                .frame(280.0, 34.0),
                        ),
                        Self::setting_row(
                            "Repeat Delay",
                            "Delay before a held key starts repeating",
                            Slider::new(self.repeat_delay.binding())
                                .range(0.2..=1.5)
                                .step(0.1)
                                .frame(CONTROL_WIDTH, 32.0),
                        ),
                        Self::setting_row(
                            "Repeat Rate",
                            "Speed of repeated key input",
                            Slider::new(self.repeat_rate.binding())
                                .range(5.0..=60.0)
                                .step(1.0)
                                .frame(CONTROL_WIDTH, 32.0),
                        ),
                        Self::setting_row(
                            "Shortcuts",
                            "Configure system keyboard shortcuts",
                            Button::new("Open")
                                .style(ButtonStyle::Standard)
                                .on_click(move || {
                                    shortcut_status.set(String::from(
                                        "Shortcut editing is not available yet.",
                                    ));
                                })
                                .frame(72.0, 32.0),
                        ),
                    ],
                ),
                Self::group(
                    "Pointer",
                    vec![
                        Self::setting_row(
                            "Mouse Speed",
                            "Pointer movement speed",
                            Slider::new(self.mouse_speed.binding())
                                .range(0.25..=3.0)
                                .step(0.05)
                                .frame(CONTROL_WIDTH, 32.0),
                        ),
                        Self::setting_row(
                            "Natural Scrolling",
                            "Move content in the same direction as your fingers",
                            Switch::new(self.natural_scrolling.binding()),
                        ),
                        Self::setting_row(
                            "Tap to Click",
                            "Use a touchpad tap as a primary click",
                            Switch::new(self.touchpad_tap.binding()),
                        ),
                    ],
                ),
            ],
        )
    }

    fn network_page(&self) -> Box<dyn View + 'static> {
        let service_available = network_service_available();
        let static_enabled = self.network_mode.get() == 1;
        let proxy_enabled = self.proxy_enabled.get();
        Self::page(
            Section::Network,
            vec![
                Self::group(
                    "Connections",
                    vec![
                        Self::setting_row(
                            "Ethernet",
                            "Wired network connection",
                            Switch::new(self.ethernet_enabled.binding()),
                        ),
                        Self::setting_row(
                            "Wi-Fi",
                            "Wireless network connection",
                            Switch::new(self.wifi_enabled.binding()).enabled(false),
                        ),
                        Self::value_row(
                            "Connection Status",
                            "",
                            if service_available {
                                "Connected"
                            } else {
                                "Unavailable"
                            },
                        ),
                        Self::value_row("Interface", "", "virtio-net0"),
                        Self::value_row("MAC / IP Details", "", "Not reported by network.service"),
                    ],
                ),
                Self::group(
                    "IP & DNS",
                    vec![
                        Self::setting_row(
                            "Configuration",
                            "Obtain settings automatically or enter them manually",
                            SegmentedControl::new(self.network_mode.binding())
                                .item(0, "DHCP")
                                .item(1, "Static")
                                .frame(220.0, 34.0),
                        ),
                        Self::setting_row(
                            "IP Address",
                            "IPv4 address for this interface",
                            TextField::new(self.ip_address.binding())
                                .placeholder("0.0.0.0")
                                .enabled(static_enabled)
                                .frame(CONTROL_WIDTH, 36.0),
                        ),
                        Self::setting_row(
                            "DNS Server",
                            "Resolver used for domain names",
                            TextField::new(self.dns_server.binding())
                                .placeholder("0.0.0.0")
                                .enabled(static_enabled)
                                .frame(CONTROL_WIDTH, 36.0),
                        ),
                    ],
                ),
                Self::group(
                    "Proxy",
                    vec![
                        Self::setting_row(
                            "Use Proxy",
                            "Route HTTP and HTTPS traffic through a proxy",
                            Switch::new(self.proxy_enabled.binding()),
                        ),
                        Self::setting_row(
                            "Proxy Address",
                            "Host and port",
                            TextField::new(self.proxy.binding())
                                .placeholder("proxy.example:8080")
                                .enabled(proxy_enabled)
                                .frame(CONTROL_WIDTH, 36.0),
                        ),
                    ],
                ),
            ],
        )
    }

    fn security_page(&self) -> Box<dyn View + 'static> {
        let trust = Path::new("/libraries/certificate/trust-a.json").exists()
            || Path::new("/libraries/certificate/trust-b.json").exists();
        let revocations = Path::new("/libraries/certificate/revocations-a.json").exists()
            || Path::new("/libraries/certificate/revocations-b.json").exists();
        let grants = persistent_grant_count();
        let events = fs::read_to_string("/system/logs/audit.log")
            .map(|text| text.lines().count())
            .unwrap_or(0);
        Self::page(
            Section::Security,
            vec![
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
                            SegmentedControl::new(self.unsigned_policy.binding())
                                .item(0, "Deny")
                                .disabled_item(1, "Ask")
                                .frame(220.0, 34.0),
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
            ],
        )
    }

    fn application_icon(icon: Option<ImageData>, size: f32) -> StackChild {
        if let Some(icon) = icon {
            Image::new(icon)
                .content_mode(ImageContentMode::Fit)
                .radius(CornerRadius::Custom(8.0))
                .frame(size, size)
        } else {
            Icon::new(IconName::AppWindow)
                .size(size * 0.65)
                .frame(size, size)
        }
    }

    fn applications_page(&self) -> Box<dyn View + 'static> {
        let applications = self.applications.get();
        let selected_index = self
            .selected_application
            .get()
            .min(applications.len().saturating_sub(1));
        let query = self.search.get().trim().to_ascii_lowercase();
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
            let labels = VStack::new()
                .alignment(StackAlignment::Stretch)
                .gap(StackGap::None)
                .child(
                    Text::new(application.name.clone())
                        .font_size(13.0)
                        .line_height(20.0)
                        .weight(600),
                )
                .child(Self::secondary(application.developer.clone()));
            rows = rows.child(
                Button::new(application.name.clone())
                    .content(
                        Padding::symmetric(10.0, 7.0).content(
                            HStack::new()
                                .alignment(StackAlignment::Center)
                                .gap(StackGap::Small)
                                .child(Self::application_icon(application.icon.clone(), 34.0))
                                .child(labels.layout().flex_grow(1.0))
                                .child(Self::secondary(format!(
                                    "{} grants",
                                    application_grants(&application.executable).len()
                                ))),
                        ),
                    )
                    .style(if index == selected_index {
                        ButtonStyle::Standard
                    } else {
                        ButtonStyle::Ghost
                    })
                    .alignment(ZStackAlignment::Leading)
                    .on_click(move || selection.set(index))
                    .height(58.0)
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
                .style(ButtonStyle::Standard)
                .enabled(!grants.is_empty())
                .on_click(move || {
                    status.set(match revoke_application_grants(&executable) {
                        Ok(count) => format!("Revoked {count} grants for {app_name}."),
                        Err(error) => format!("Unable to revoke capabilities: {error}"),
                    });
                })
                .frame(96.0, 32.0);
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
                        .child(Self::application_icon(application.icon.clone(), 44.0))
                        .child(
                            VStack::new()
                                .alignment(StackAlignment::Stretch)
                                .gap(StackGap::None)
                                .child(
                                    Text::new(application.name.clone())
                                        .font_size(17.0)
                                        .line_height(24.0)
                                        .weight(600),
                                )
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

        Box::new(
            VStack::new()
                .alignment(StackAlignment::Stretch)
                .gap(StackGap::ExtraLarge)
                .child(Self::page_header(Section::Applications))
                .child(
                    HStack::new()
                        .alignment(StackAlignment::Stretch)
                        .gap(StackGap::Large)
                        .child(
                            VStack::new()
                                .alignment(StackAlignment::Stretch)
                                .gap(StackGap::Small)
                                .child(
                                    Text::new("Installed Applications")
                                        .font_size(11.0)
                                        .line_height(18.0)
                                        .weight(600)
                                        .color(Theme::DEFAULT.colors.text_secondary),
                                )
                                .child(rows)
                                .width(250.0)
                                .flex_shrink(0.0),
                        )
                        .child(Divider::new())
                        .child(details.flex_grow(1.0))
                        .layout()
                        .flex_shrink(0.0),
                ),
        )
    }

    fn current_preferences(&self) -> Preferences {
        let users = self.users.get();
        let auto_login_user = if self.auto_login.get() {
            users
                .get(self.selected_user.get().min(users.len().saturating_sub(1)))
                .map(|user| user.name.clone())
                .unwrap_or_default()
        } else {
            String::new()
        };
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
            auto_login_user,
            unsigned_policy: self.unsigned_policy.get(),
        }
    }
}

impl App for SettingsApp {
    type Body = Box<dyn View + 'static>;

    fn new() -> Self {
        let preferences = Preferences::load();
        let (users, status) = match accounts::load() {
            Ok(database) => (database.users().to_vec(), String::new()),
            Err(error) => (Vec::new(), format!("Unable to load users: {error}")),
        };
        Self {
            section: State::new(Section::General.index()),
            search: State::new(String::new()),
            status: State::new(status),
            users: State::new(users),
            selected_user: State::new(0),
            new_name: State::new(String::new()),
            new_display_name: State::new(String::new()),
            new_user_password: State::new(String::new()),
            password: State::new(String::new()),
            auto_login: State::new(preferences.auto_login),
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
            network_mode: State::new(preferences.network_mode),
            ip_address: State::new(preferences.ip_address),
            dns_server: State::new(preferences.dns_server),
            proxy_enabled: State::new(preferences.proxy_enabled),
            proxy: State::new(preferences.proxy),
            unsigned_policy: State::new(preferences.unsigned_policy),
            applications: State::new(load_applications()),
            selected_application: State::new(0),
            page_scroll: ScrollState::new(),
        }
    }

    fn window(&self) -> WindowOptions {
        WindowOptions::new("Settings")
            .size(WINDOW_WIDTH, WINDOW_HEIGHT)
            .resizable(true)
    }

    fn body(&self, _context: &ViewContext) -> Self::Body {
        let page = match Section::from_index(self.section.get()) {
            Section::Account => self.account_page(),
            Section::General => self.general_page(),
            Section::Appearance => self.appearance_page(),
            Section::Input => self.input_page(),
            Section::Network => self.network_page(),
            Section::Security => self.security_page(),
            Section::Applications => self.applications_page(),
        };
        Box::new(
            Background::new()
                .background(Rectangle::new().color(RectangleColor::Background))
                .content(
                    VStack::new()
                        .alignment(StackAlignment::Stretch)
                        .gap(StackGap::None)
                        .child(self.toolbar())
                        .child(Divider::new())
                        .child(
                            HStack::new()
                                .alignment(StackAlignment::Stretch)
                                .gap(StackGap::None)
                                .child(self.sidebar().flex_shrink(0.0))
                                .child(Divider::new())
                                .child(
                                    Background::new()
                                        .background(Rectangle::new().color(RectangleColor::Surface))
                                        .content(
                                            Scroll::new(self.page_scroll.clone())
                                                .axis(ScrollAxis::Vertical)
                                                .scrollbar(ScrollBarVisibility::Always)
                                                .content(
                                                    Padding::only(28.0, 32.0, 48.0, 32.0)
                                                        .content(page),
                                                ),
                                        )
                                        .layout()
                                        .flex_grow(1.0)
                                        .flex_shrink(1.0),
                                )
                                .layout()
                                .flex_grow(1.0)
                                .flex_shrink(1.0),
                        )
                        .child(Divider::new())
                        .child(self.status_bar()),
                ),
        )
    }
}

fn load_applications() -> Vec<ApplicationInfo> {
    let root = Path::new("/applications");
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut applications = Vec::new();
    for entry in entries.flatten() {
        let app_root = entry.path();
        if !app_root.is_dir() {
            continue;
        }
        let Ok(content) = fs::read_to_string(app_root.join("about.toml")) else {
            continue;
        };
        let Some(name) = parse_string_field(&content, "name") else {
            continue;
        };
        let Some(bundle_id) = parse_string_field(&content, "bundle_id") else {
            continue;
        };
        let Some(entry_name) = parse_string_field(&content, "entry") else {
            continue;
        };
        let developer = parse_string_field(&content, "developer")
            .or_else(|| parse_string_field(&content, "vendor"))
            .unwrap_or_else(|| String::from("Unknown developer"));
        let icon = parse_string_field(&content, "icon")
            .and_then(|icon_name| load_application_icon(&app_root.join(icon_name)));
        applications.push(ApplicationInfo {
            name,
            bundle_id,
            developer,
            executable: app_root.join(entry_name).to_string_lossy().into_owned(),
            icon,
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
    use super::{civil_date, parse_string_field};

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
