mod accounts;
mod preferences;

use std::path::Path;

use mochios_user_database::UserRecord;
use preferences::Preferences;
use viewkit::prelude::*;

const CONTENT_WIDTH: f32 = 610.0;
const SIDEBAR_WIDTH: f32 = 220.0;
const MOCHIOS_VERSION: &str = match option_env!("MOCHIOS_VERSION") {
    Some(version) => version,
    None => "development",
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Account,
    General,
    Appearance,
    Input,
    Network,
    Security,
}

impl Section {
    const ALL: [Self; 6] = [
        Self::Account,
        Self::General,
        Self::Appearance,
        Self::Input,
        Self::Network,
        Self::Security,
    ];

    const fn index(self) -> usize {
        match self {
            Self::Account => 0,
            Self::General => 1,
            Self::Appearance => 2,
            Self::Input => 3,
            Self::Network => 4,
            Self::Security => 5,
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
        }
    }
}

struct SettingsApp {
    section: State<usize>,
    status: State<String>,
    users: State<Vec<UserRecord>>,
    selected_user: State<usize>,
    new_name: State<String>,
    new_display_name: State<String>,
    password: State<String>,
    device_name: State<String>,
    language: State<String>,
    region: State<String>,
    timezone: State<String>,
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
    network_mode: State<usize>,
    ip_address: State<String>,
    dns_server: State<String>,
    proxy: State<String>,
    auto_login: State<bool>,
    unsigned_policy: State<usize>,
}

impl SettingsApp {
    fn sidebar(&self) -> impl View + 'static {
        let mut items = VStack::new()
            .alignment(StackAlignment::Stretch)
            .gap(StackGap::ExtraSmall);
        for section in Section::ALL {
            let selection = self.section.clone();
            let selected = selection.get() == section.index();
            items = items.child(
                Button::new(section.label())
                    .content(
                        HStack::new()
                            .alignment(StackAlignment::Center)
                            .gap(StackGap::Small)
                            .child(Icon::new(section.icon()).size(18.0))
                            .child(
                                Text::new(section.label())
                                    .font_size(13.0)
                                    .line_height(22.0)
                                    .weight(if selected { 700 } else { 500 }),
                            ),
                    )
                    .style(if selected {
                        ButtonStyle::Custom {
                            background: Color::rgba(0, 122, 255, 34),
                            hovered_background: Color::rgba(0, 122, 255, 48),
                            border: Color::TRANSPARENT,
                            hovered_border: Color::TRANSPARENT,
                            foreground: Color::from_rgb_hex(0x1d1d1f),
                        }
                    } else {
                        ButtonStyle::Ghost
                    })
                    .alignment(ZStackAlignment::Leading)
                    .radius(CornerRadius::Custom(8.0))
                    .on_click(move || selection.set(section.index()))
                    .height(38.0),
            );
        }
        Background::new()
            .background(
                Rectangle::new().color(RectangleColor::Custom(Color::from_rgb_hex(0xededf0))),
            )
            .content(
                Padding::all(16.0).content(
                    VStack::new()
                        .alignment(StackAlignment::Stretch)
                        .gap(StackGap::Large)
                        .child(
                            Text::new("Settings")
                                .font_size(22.0)
                                .line_height(30.0)
                                .weight(700),
                        )
                        .child(items),
                ),
            )
    }

    fn page_header(title: &str, subtitle: &str) -> StackChild {
        VStack::new()
            .alignment(StackAlignment::Stretch)
            .gap(StackGap::ExtraSmall)
            .child(
                Text::new(title)
                    .font_size(26.0)
                    .line_height(34.0)
                    .weight(700),
            )
            .child(
                Text::new(subtitle)
                    .font_size(12.0)
                    .line_height(20.0)
                    .color(Color::from_rgb_hex(0x6e6e73)),
            )
            .into_stack_child()
    }

    fn card(content: impl View + 'static) -> StackChild {
        Card::new()
            .shadow(ShadowStyle::None)
            .content(Padding::all(18.0).content(content))
            .width(CONTENT_WIDTH)
            .into_stack_child()
    }

    fn row(label: impl Into<String>, value: impl Into<String>) -> StackChild {
        HStack::new()
            .alignment(StackAlignment::Center)
            .distribution(StackDistribution::SpaceBetween)
            .gap(StackGap::Medium)
            .child(
                Text::new(label.into())
                    .font_size(13.0)
                    .line_height(22.0)
                    .weight(600),
            )
            .child(
                Text::new(value.into())
                    .font_size(12.0)
                    .line_height(20.0)
                    .alignment(TextAlignment::End)
                    .color(Color::from_rgb_hex(0x6e6e73)),
            )
            .height(34.0)
            .into_stack_child()
    }

    fn field_row(label: &'static str, value: State<String>) -> StackChild {
        HStack::new()
            .alignment(StackAlignment::Center)
            .distribution(StackDistribution::SpaceBetween)
            .gap(StackGap::Medium)
            .child(
                Text::new(label)
                    .font_size(13.0)
                    .line_height(22.0)
                    .weight(600),
            )
            .child(TextField::new(value.binding()).frame(330.0, 38.0))
            .height(46.0)
            .into_stack_child()
    }

    fn account_page(&self) -> Box<dyn View + 'static> {
        let users = self.users.get();
        let selected = self.selected_user.get().min(users.len().saturating_sub(1));
        let selected_name = users.get(selected).map(|user| user.name.clone());
        let mut user_buttons = VStack::new()
            .alignment(StackAlignment::Stretch)
            .gap(StackGap::ExtraSmall);
        for (index, user) in users.iter().enumerate() {
            let selected_user = self.selected_user.clone();
            let label = format!("{}  ({})", user.display_name, user.name);
            user_buttons = user_buttons.child(
                Button::new(label)
                    .style(if index == selected {
                        ButtonStyle::Accent
                    } else {
                        ButtonStyle::Ghost
                    })
                    .alignment(ZStackAlignment::Leading)
                    .on_click(move || selected_user.set(index))
                    .height(36.0),
            );
        }

        let users_for_add = self.users.clone();
        let status_for_add = self.status.clone();
        let name_for_add = self.new_name.clone();
        let display_for_add = self.new_display_name.clone();
        let password_for_add = self.password.clone();
        let add_button = Button::new("Add User")
            .style(ButtonStyle::Accent)
            .on_click(move || {
                let name = name_for_add.get();
                let display = display_for_add.get();
                let mut password = password_for_add.get();
                let result = accounts::add(&name, &display, password.as_bytes());
                password.clear();
                password_for_add.set(String::new());
                match result {
                    Ok(()) => {
                        users_for_add.set(
                            accounts::load()
                                .map(|database| database.users().to_vec())
                                .unwrap_or_default(),
                        );
                        name_for_add.set(String::new());
                        display_for_add.set(String::new());
                        status_for_add.set(String::from("User added."));
                    }
                    Err(error) => status_for_add.set(format!("Unable to add user: {error}")),
                }
            });

        let status_for_password = self.status.clone();
        let password_for_change = self.password.clone();
        let password_name = selected_name.clone();
        let password_button = Button::new("Change Password")
            .enabled(password_name.is_some())
            .on_click(move || {
                let Some(name) = password_name.as_deref() else {
                    return;
                };
                let mut password = password_for_change.get();
                let result = accounts::set_password(name, password.as_bytes());
                password.clear();
                password_for_change.set(String::new());
                status_for_password.set(match result {
                    Ok(()) => String::from("Password changed."),
                    Err(error) => format!("Unable to change password: {error}"),
                });
            });

        let users_for_remove = self.users.clone();
        let status_for_remove = self.status.clone();
        let remove_name = selected_name.clone();
        let remove_button = Button::new("Delete User")
            .style(ButtonStyle::Danger)
            .enabled(remove_name.as_deref().is_some_and(|name| name != "root"))
            .on_click(move || {
                let Some(name) = remove_name.as_deref() else {
                    return;
                };
                match accounts::remove(name) {
                    Ok(()) => {
                        users_for_remove.set(
                            accounts::load()
                                .map(|database| database.users().to_vec())
                                .unwrap_or_default(),
                        );
                        status_for_remove.set(String::from("User deleted. Home data was kept."));
                    }
                    Err(error) => status_for_remove.set(format!("Unable to delete user: {error}")),
                }
            });

        Box::new(
            VStack::new()
                .alignment(StackAlignment::Stretch)
                .gap(StackGap::Large)
                .child(Self::page_header("Account", "Users and sign-in settings"))
                .child(Self::card(user_buttons))
                .child(Self::card(
                    VStack::new()
                        .alignment(StackAlignment::Stretch)
                        .gap(StackGap::Small)
                        .child(Self::field_row("Account name", self.new_name.clone()))
                        .child(Self::field_row(
                            "Display name",
                            self.new_display_name.clone(),
                        ))
                        .child(
                            HStack::new()
                                .alignment(StackAlignment::Center)
                                .distribution(StackDistribution::SpaceBetween)
                                .child(Text::new("Password").font_size(13.0).weight(600))
                                .child(
                                    TextField::new(self.password.binding())
                                        .secure(true)
                                        .frame(330.0, 38.0),
                                )
                                .height(46.0),
                        )
                        .child(
                            HStack::new()
                                .gap(StackGap::Small)
                                .child(add_button)
                                .child(password_button)
                                .child(remove_button),
                        )
                        .child(Switch::new(self.auto_login.binding()).label("Automatic login")),
                ))
                .child(
                    Text::new("mochiOS ID linking will be available in a later release.")
                        .font_size(11.0)
                        .color(Color::from_rgb_hex(0x6e6e73)),
                ),
        )
    }

    fn general_page(&self) -> Box<dyn View + 'static> {
        Box::new(
            VStack::new()
                .alignment(StackAlignment::Stretch)
                .gap(StackGap::Large)
                .child(Self::page_header(
                    "General",
                    "Device, locale, date, and system information",
                ))
                .child(Self::card(
                    VStack::new()
                        .alignment(StackAlignment::Stretch)
                        .child(Self::field_row("Device name", self.device_name.clone()))
                        .child(Self::field_row("Language", self.language.clone()))
                        .child(Self::field_row("Region", self.region.clone()))
                        .child(Self::field_row("Time zone", self.timezone.clone())),
                ))
                .child(Self::card(
                    VStack::new()
                        .alignment(StackAlignment::Stretch)
                        .child(Self::row("mochiOS version", MOCHIOS_VERSION))
                        .child(Self::row("mnu version", "workspace"))
                        .child(Self::row("mBoot version", "0.2.0"))
                        .child(Self::row(
                            "Build ID",
                            option_env!("BUILD_ID").unwrap_or("development"),
                        ))
                        .child(Self::row(
                            "Kernel / architecture",
                            format!("mnu / {}", std::env::consts::ARCH),
                        )),
                )),
        )
    }

    fn appearance_page(&self) -> Box<dyn View + 'static> {
        Box::new(
            VStack::new()
                .alignment(StackAlignment::Stretch)
                .gap(StackGap::Large)
                .child(Self::page_header(
                    "Appearance",
                    "Desktop and interface presentation",
                ))
                .child(Self::card(
                    VStack::new()
                        .alignment(StackAlignment::Stretch)
                        .gap(StackGap::Medium)
                        .child(
                            SegmentedControl::new(self.appearance.binding())
                                .item(0, "Light")
                                .item(1, "Dark")
                                .item(2, "System"),
                        )
                        .child(
                            SegmentedControl::new(self.accent.binding())
                                .item(0, "Blue")
                                .item(1, "Purple")
                                .item(2, "Pink")
                                .item(3, "Red")
                                .item(4, "Green")
                                .item(5, "Graphite"),
                        )
                        .child(Self::field_row("Wallpaper", self.wallpaper.clone()))
                        .child(
                            Slider::new(self.ui_scale.binding())
                                .range(0.75..=2.0)
                                .step(0.05)
                                .label("UI scale"),
                        )
                        .child(
                            Slider::new(self.font_size.binding())
                                .range(10.0..=24.0)
                                .step(1.0)
                                .label("Font size"),
                        ),
                )),
        )
    }

    fn input_page(&self) -> Box<dyn View + 'static> {
        Box::new(
            VStack::new()
                .alignment(StackAlignment::Stretch)
                .gap(StackGap::Large)
                .child(Self::page_header(
                    "Input",
                    "Keyboard, mouse, touchpad, and shortcuts",
                ))
                .child(Self::card(
                    VStack::new()
                        .alignment(StackAlignment::Stretch)
                        .gap(StackGap::Medium)
                        .child(
                            SegmentedControl::new(self.keyboard_layout.binding())
                                .item(0, "US")
                                .item(1, "Japanese")
                                .item(2, "British"),
                        )
                        .child(
                            Slider::new(self.repeat_delay.binding())
                                .range(0.2..=1.5)
                                .step(0.1)
                                .label("Repeat delay"),
                        )
                        .child(
                            Slider::new(self.repeat_rate.binding())
                                .range(5.0..=60.0)
                                .step(1.0)
                                .label("Repeat rate"),
                        )
                        .child(
                            Slider::new(self.mouse_speed.binding())
                                .range(0.25..=3.0)
                                .step(0.05)
                                .label("Mouse speed"),
                        )
                        .child(
                            Switch::new(self.natural_scrolling.binding())
                                .label("Natural scrolling"),
                        )
                        .child(Switch::new(self.touchpad_tap.binding()).label("Tap to click"))
                        .child(Self::row("Shortcuts", "Default")),
                )),
        )
    }

    fn network_page(&self) -> Box<dyn View + 'static> {
        let service_available = network_service_available();
        Box::new(
            VStack::new()
                .alignment(StackAlignment::Stretch)
                .gap(StackGap::Large)
                .child(Self::page_header(
                    "Network",
                    "Ethernet, addressing, DNS, and proxy",
                ))
                .child(Self::card(
                    VStack::new()
                        .alignment(StackAlignment::Stretch)
                        .child(Self::row(
                            "Connection",
                            if service_available {
                                "Network service active"
                            } else {
                                "Unavailable"
                            },
                        ))
                        .child(Self::row(
                            "Ethernet",
                            if service_available {
                                "Connected or configuring"
                            } else {
                                "Not connected"
                            },
                        ))
                        .child(Self::row("Wi-Fi", "No Wi-Fi device"))
                        .child(Self::row(
                            "MAC / IP details",
                            "Interface information unavailable",
                        )),
                ))
                .child(Self::card(
                    VStack::new()
                        .alignment(StackAlignment::Stretch)
                        .gap(StackGap::Small)
                        .child(
                            SegmentedControl::new(self.network_mode.binding())
                                .item(0, "DHCP")
                                .item(1, "Static"),
                        )
                        .child(Self::field_row("IP address", self.ip_address.clone()))
                        .child(Self::field_row("DNS server", self.dns_server.clone()))
                        .child(Self::field_row("Proxy", self.proxy.clone())),
                )),
        )
    }

    fn security_page(&self) -> Box<dyn View + 'static> {
        let trust = Path::new("/libraries/certificate/state.bin").exists();
        let grants = std::fs::read_to_string("/system/policy/capability-grants.db")
            .map(|text| text.lines().filter(|line| !line.is_empty()).count())
            .unwrap_or(0);
        let events = std::fs::read_to_string("/system/logs/audit.log")
            .map(|text| text.lines().count())
            .unwrap_or(0);
        Box::new(
            VStack::new()
                .alignment(StackAlignment::Stretch)
                .gap(StackGap::Large)
                .child(Self::page_header(
                    "Security",
                    "Certificates, capabilities, and execution policy",
                ))
                .child(Self::card(
                    VStack::new()
                        .alignment(StackAlignment::Stretch)
                        .child(Self::row("Developer Certificate", "Managed by Kome"))
                        .child(Self::row(
                            "Installed certificates",
                            if trust {
                                "Trust database installed"
                            } else {
                                "No trust snapshot"
                            },
                        ))
                        .child(Self::row(
                            "Revocation status",
                            if trust {
                                "Database available"
                            } else {
                                "Unavailable"
                            },
                        ))
                        .child(Self::row(
                            "Trusted Root",
                            if trust {
                                "mochiOS Root"
                            } else {
                                "Not installed"
                            },
                        )),
                ))
                .child(Self::card(
                    VStack::new()
                        .alignment(StackAlignment::Stretch)
                        .gap(StackGap::Medium)
                        .child(Self::row(
                            "Persistent app capabilities",
                            format!("{grants} grants"),
                        ))
                        .child(Self::row(
                            "Capability revocation",
                            "Remove grants from policy database",
                        ))
                        .child(
                            SegmentedControl::new(self.unsigned_policy.binding())
                                .item(0, "Deny unsigned apps")
                                .disabled_item(1, "Ask before opening"),
                        )
                        .child(Self::row(
                            "Security event history",
                            format!("{events} events"),
                        )),
                )),
        )
    }

    fn current_preferences(&self) -> Preferences {
        Preferences {
            device_name: self.device_name.get(),
            language: self.language.get(),
            region: self.region.get(),
            timezone: self.timezone.get(),
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
            network_mode: self.network_mode.get(),
            ip_address: self.ip_address.get(),
            dns_server: self.dns_server.get(),
            proxy: self.proxy.get(),
            auto_login: self.auto_login.get(),
            unsigned_policy: self.unsigned_policy.get(),
        }
    }
}

impl App for SettingsApp {
    type Body = Box<dyn View + 'static>;

    fn new() -> Self {
        let preferences = Preferences::load();
        let users = accounts::load()
            .map(|database| database.users().to_vec())
            .unwrap_or_default();
        Self {
            section: State::new(0),
            status: State::new(String::new()),
            users: State::new(users),
            selected_user: State::new(0),
            new_name: State::new(String::new()),
            new_display_name: State::new(String::new()),
            password: State::new(String::new()),
            device_name: State::new(preferences.device_name),
            language: State::new(preferences.language),
            region: State::new(preferences.region),
            timezone: State::new(preferences.timezone),
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
            network_mode: State::new(preferences.network_mode),
            ip_address: State::new(preferences.ip_address),
            dns_server: State::new(preferences.dns_server),
            proxy: State::new(preferences.proxy),
            auto_login: State::new(preferences.auto_login),
            unsigned_policy: State::new(preferences.unsigned_policy),
        }
    }

    fn window(&self) -> WindowOptions {
        WindowOptions::new("Settings")
            .size(940.0, 680.0)
            .resizable(true)
    }

    fn body(&self, _context: &ViewContext) -> Self::Body {
        let page = match self.section.get() {
            0 => self.account_page(),
            1 => self.general_page(),
            2 => self.appearance_page(),
            3 => self.input_page(),
            4 => self.network_page(),
            _ => self.security_page(),
        };
        let preferences = self.current_preferences();
        let status = self.status.clone();
        let save = Button::new("Save")
            .style(ButtonStyle::Accent)
            .on_click(move || {
                status.set(match preferences.save() {
                    Ok(()) => String::from("Settings saved."),
                    Err(error) => format!("Unable to save settings: {error}"),
                });
            });
        Box::new(
            HStack::new()
                .alignment(StackAlignment::Stretch)
                .gap(StackGap::None)
                .child(self.sidebar().width(SIDEBAR_WIDTH).flex_shrink(0.0))
                .child(
                    Background::new()
                        .background(
                            Rectangle::new()
                                .color(RectangleColor::Custom(Color::from_rgb_hex(0xf6f6f8))),
                        )
                        .content(
                            VStack::new()
                                .alignment(StackAlignment::Stretch)
                                .gap(StackGap::Small)
                                .child(
                                    Scroll::vertical(Padding::all(28.0).content(page))
                                        .layout()
                                        .flex_grow(1.0),
                                )
                                .child(
                                    Padding::symmetric(28.0, 10.0).content(
                                        HStack::new()
                                            .alignment(StackAlignment::Center)
                                            .distribution(StackDistribution::SpaceBetween)
                                            .child(
                                                Text::new(self.status.get())
                                                    .font_size(11.0)
                                                    .color(Color::from_rgb_hex(0x6e6e73)),
                                            )
                                            .child(save),
                                    ),
                                ),
                        )
                        .layout()
                        .flex_grow(1.0),
                ),
        )
    }
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
