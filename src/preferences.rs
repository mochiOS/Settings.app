use std::fs;
use std::io;
use std::path::Path;

#[cfg(target_os = "mochios")]
const CONFIG_ROOT: &str = "/var/config";

#[cfg(not(target_os = "mochios"))]
const CONFIG_ROOT: &str = "/tmp/mochios-settings/config";

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Preferences {
    pub device_name: String,
    pub language: String,
    pub region: String,
    pub timezone: String,
    pub automatic_time: bool,
    pub appearance: usize,
    pub accent: usize,
    pub wallpaper: String,
    pub ui_scale: f32,
    pub font_size: f32,
    pub keyboard_layout: usize,
    pub repeat_delay: f32,
    pub repeat_rate: f32,
    pub mouse_speed: f32,
    pub natural_scrolling: bool,
    pub touchpad_tap: bool,
    pub ethernet_enabled: bool,
    pub wifi_enabled: bool,
    pub network_mode: usize,
    pub ip_address: String,
    pub dns_server: String,
    pub proxy: String,
    pub proxy_enabled: bool,
    pub auto_login: bool,
    pub auto_login_user: String,
    pub unsigned_policy: usize,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            device_name: String::from("mochiOS"),
            language: String::from("Japanese"),
            region: String::from("Japan"),
            timezone: String::from("Asia/Tokyo"),
            automatic_time: true,
            appearance: 2,
            accent: 0,
            wallpaper: String::from("/libraries/wallpapers/default.png"),
            ui_scale: 1.0,
            font_size: 13.0,
            keyboard_layout: 1,
            repeat_delay: 0.5,
            repeat_rate: 30.0,
            mouse_speed: 1.0,
            natural_scrolling: true,
            touchpad_tap: true,
            ethernet_enabled: true,
            wifi_enabled: false,
            network_mode: 0,
            ip_address: String::new(),
            dns_server: String::new(),
            proxy: String::new(),
            proxy_enabled: false,
            auto_login: false,
            auto_login_user: String::new(),
            unsigned_policy: 0,
        }
    }
}

impl Preferences {
    pub(crate) fn load() -> Self {
        let mut settings = Self::default();
        for category in [
            "general",
            "appearance",
            "input",
            "network",
            "account",
            "security",
        ] {
            let path = config_path(category);
            let Ok(text) = fs::read_to_string(path) else {
                continue;
            };
            settings.apply(&text);
        }
        settings
    }

    fn apply(&mut self, text: &str) {
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key.trim() {
                "device_name" => self.device_name = clean(value),
                "language" => self.language = clean(value),
                "region" => self.region = clean(value),
                "timezone" => self.timezone = clean(value),
                "automatic_time" => self.automatic_time = parse_bool(value, true),
                "appearance" => self.appearance = parse_usize(value, 2, 2),
                "accent" => self.accent = parse_usize(value, 0, 5),
                "wallpaper" => self.wallpaper = clean(value),
                "ui_scale" => self.ui_scale = parse_f32(value, 1.0, 0.75, 2.0),
                "font_size" => self.font_size = parse_f32(value, 13.0, 10.0, 24.0),
                "keyboard_layout" => self.keyboard_layout = parse_usize(value, 1, 2),
                "repeat_delay" => self.repeat_delay = parse_f32(value, 0.5, 0.2, 1.5),
                "repeat_rate" => self.repeat_rate = parse_f32(value, 30.0, 5.0, 60.0),
                "mouse_speed" => self.mouse_speed = parse_f32(value, 1.0, 0.25, 3.0),
                "natural_scrolling" => self.natural_scrolling = parse_bool(value, true),
                "touchpad_tap" => self.touchpad_tap = parse_bool(value, true),
                "ethernet_enabled" => self.ethernet_enabled = parse_bool(value, true),
                "wifi_enabled" => self.wifi_enabled = parse_bool(value, false),
                "network_mode" => self.network_mode = parse_usize(value, 0, 1),
                "ip_address" => self.ip_address = clean(value),
                "dns_server" => self.dns_server = clean(value),
                "proxy" => self.proxy = clean(value),
                "proxy_enabled" => self.proxy_enabled = parse_bool(value, false),
                "auto_login" => self.auto_login = parse_bool(value, false),
                "auto_login_user" => self.auto_login_user = clean(value),
                "unsigned_policy" => self.unsigned_policy = parse_usize(value, 0, 1),
                _ => {}
            }
        }
    }

    pub(crate) fn save(&self) -> io::Result<()> {
        write_config(
            "general",
            format!(
                "device_name={}\nlanguage={}\nregion={}\ntimezone={}\nautomatic_time={}\n",
                single_line(&self.device_name),
                single_line(&self.language),
                single_line(&self.region),
                single_line(&self.timezone),
                self.automatic_time,
            ),
        )?;
        write_config(
            "appearance",
            format!(
                "appearance={}\naccent={}\nwallpaper={}\nui_scale={}\nfont_size={}\n",
                self.appearance,
                self.accent,
                single_line(&self.wallpaper),
                self.ui_scale,
                self.font_size,
            ),
        )?;
        write_config(
            "input",
            format!(
                "keyboard_layout={}\nrepeat_delay={}\nrepeat_rate={}\nmouse_speed={}\nnatural_scrolling={}\ntouchpad_tap={}\n",
                self.keyboard_layout,
                self.repeat_delay,
                self.repeat_rate,
                self.mouse_speed,
                self.natural_scrolling,
                self.touchpad_tap,
            ),
        )?;
        write_config(
            "network",
            format!(
                "ethernet_enabled={}\nwifi_enabled={}\nnetwork_mode={}\nip_address={}\ndns_server={}\nproxy={}\nproxy_enabled={}\n",
                self.ethernet_enabled,
                self.wifi_enabled,
                self.network_mode,
                single_line(&self.ip_address),
                single_line(&self.dns_server),
                single_line(&self.proxy),
                self.proxy_enabled,
            ),
        )?;
        write_config(
            "account",
            format!(
                "auto_login={}\nauto_login_user={}\n",
                self.auto_login,
                single_line(&self.auto_login_user),
            ),
        )?;
        write_config(
            "security",
            format!("unsigned_policy={}\n", self.unsigned_policy,),
        )?;
        Ok(())
    }
}

fn config_path(category: &str) -> String {
    format!("{CONFIG_ROOT}/{category}/settings.conf")
}

fn write_config(category: &str, contents: String) -> io::Result<()> {
    let path = config_path(category);
    let parent = Path::new(&path)
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid settings path"))?;
    fs::create_dir_all(parent)?;
    fs::write(path, contents)
}

fn clean(value: &str) -> String {
    value.trim().chars().take(256).collect()
}

fn single_line(value: &str) -> String {
    value
        .chars()
        .filter(|character| !matches!(character, '\n' | '\r' | '='))
        .take(256)
        .collect()
}

fn parse_bool(value: &str, fallback: bool) -> bool {
    match value.trim() {
        "true" => true,
        "false" => false,
        _ => fallback,
    }
}

fn parse_usize(value: &str, fallback: usize, maximum: usize) -> usize {
    value
        .trim()
        .parse::<usize>()
        .ok()
        .filter(|value| *value <= maximum)
        .unwrap_or(fallback)
}

fn parse_f32(value: &str, fallback: f32, minimum: f32, maximum: f32) -> f32 {
    value
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(minimum, maximum))
        .unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_removes_config_delimiters() {
        assert_eq!(single_line("mochi=OS\nnext"), "mochiOSnext");
    }

    #[test]
    fn numeric_values_are_bounded() {
        assert_eq!(parse_usize("9", 1, 2), 1);
        assert_eq!(parse_f32("99", 1.0, 0.5, 2.0), 2.0);
    }
}
