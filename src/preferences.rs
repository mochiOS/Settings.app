use std::fs;
use std::io;
use std::path::Path;

#[cfg(target_os = "mochios")]
const SETTINGS_PATH: &str = "/libraries/system/settings.conf";

#[cfg(not(target_os = "mochios"))]
const SETTINGS_PATH: &str = "/tmp/mochios-settings/settings.conf";

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Preferences {
    pub device_name: String,
    pub language: String,
    pub region: String,
    pub timezone: String,
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
    pub network_mode: usize,
    pub ip_address: String,
    pub dns_server: String,
    pub proxy: String,
    pub auto_login: bool,
    pub unsigned_policy: usize,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            device_name: String::from("mochiOS"),
            language: String::from("Japanese"),
            region: String::from("Japan"),
            timezone: String::from("Asia/Tokyo"),
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
            network_mode: 0,
            ip_address: String::new(),
            dns_server: String::new(),
            proxy: String::new(),
            auto_login: false,
            unsigned_policy: 0,
        }
    }
}

impl Preferences {
    pub(crate) fn load() -> Self {
        let Ok(text) = fs::read_to_string(SETTINGS_PATH) else {
            return Self::default();
        };
        let mut settings = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key.trim() {
                "device_name" => settings.device_name = clean(value),
                "language" => settings.language = clean(value),
                "region" => settings.region = clean(value),
                "timezone" => settings.timezone = clean(value),
                "appearance" => settings.appearance = parse_usize(value, 2, 2),
                "accent" => settings.accent = parse_usize(value, 0, 5),
                "wallpaper" => settings.wallpaper = clean(value),
                "ui_scale" => settings.ui_scale = parse_f32(value, 1.0, 0.75, 2.0),
                "font_size" => settings.font_size = parse_f32(value, 13.0, 10.0, 24.0),
                "keyboard_layout" => settings.keyboard_layout = parse_usize(value, 1, 2),
                "repeat_delay" => settings.repeat_delay = parse_f32(value, 0.5, 0.2, 1.5),
                "repeat_rate" => settings.repeat_rate = parse_f32(value, 30.0, 5.0, 60.0),
                "mouse_speed" => settings.mouse_speed = parse_f32(value, 1.0, 0.25, 3.0),
                "natural_scrolling" => settings.natural_scrolling = parse_bool(value, true),
                "touchpad_tap" => settings.touchpad_tap = parse_bool(value, true),
                "network_mode" => settings.network_mode = parse_usize(value, 0, 1),
                "ip_address" => settings.ip_address = clean(value),
                "dns_server" => settings.dns_server = clean(value),
                "proxy" => settings.proxy = clean(value),
                "auto_login" => settings.auto_login = parse_bool(value, false),
                "unsigned_policy" => settings.unsigned_policy = parse_usize(value, 0, 1),
                _ => {}
            }
        }
        settings
    }

    pub(crate) fn save(&self) -> io::Result<()> {
        let path = Path::new(SETTINGS_PATH);
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid settings path"))?;
        fs::create_dir_all(parent)?;
        fs::write(
            path,
            format!(
                concat!(
                    "device_name={}\n",
                    "language={}\n",
                    "region={}\n",
                    "timezone={}\n",
                    "appearance={}\n",
                    "accent={}\n",
                    "wallpaper={}\n",
                    "ui_scale={}\n",
                    "font_size={}\n",
                    "keyboard_layout={}\n",
                    "repeat_delay={}\n",
                    "repeat_rate={}\n",
                    "mouse_speed={}\n",
                    "natural_scrolling={}\n",
                    "touchpad_tap={}\n",
                    "network_mode={}\n",
                    "ip_address={}\n",
                    "dns_server={}\n",
                    "proxy={}\n",
                    "auto_login={}\n",
                    "unsigned_policy={}\n"
                ),
                single_line(&self.device_name),
                single_line(&self.language),
                single_line(&self.region),
                single_line(&self.timezone),
                self.appearance,
                self.accent,
                single_line(&self.wallpaper),
                self.ui_scale,
                self.font_size,
                self.keyboard_layout,
                self.repeat_delay,
                self.repeat_rate,
                self.mouse_speed,
                self.natural_scrolling,
                self.touchpad_tap,
                self.network_mode,
                single_line(&self.ip_address),
                single_line(&self.dns_server),
                single_line(&self.proxy),
                self.auto_login,
                self.unsigned_policy,
            ),
        )
    }
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
