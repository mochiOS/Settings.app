use std::fs;
use std::io;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(1);

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
    pub diagnostics_enabled: bool,
    pub diagnostics_consent: bool,
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
            wallpaper: String::from("/system/libraries/wallpapers/default.png"),
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
            diagnostics_enabled: true,
            diagnostics_consent: false,
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
            "diagnostics",
        ] {
            let _ = recover_config(Path::new(CONFIG_ROOT), category);
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
                "diagnostics_enabled" => self.diagnostics_enabled = parse_bool(value, true),
                "diagnostics_consent" => self.diagnostics_consent = parse_bool(value, false),
                _ => {}
            }
        }
    }

    pub(crate) fn save_changed(&self, previous: &Self) -> io::Result<()> {
        if self.device_name != previous.device_name
            || self.language != previous.language
            || self.region != previous.region
            || self.timezone != previous.timezone
            || self.automatic_time != previous.automatic_time
        {
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
        }
        if self.appearance != previous.appearance
            || self.accent != previous.accent
            || self.wallpaper != previous.wallpaper
            || self.ui_scale != previous.ui_scale
            || self.font_size != previous.font_size
        {
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
        }
        if self.keyboard_layout != previous.keyboard_layout
            || self.repeat_delay != previous.repeat_delay
            || self.repeat_rate != previous.repeat_rate
            || self.mouse_speed != previous.mouse_speed
            || self.natural_scrolling != previous.natural_scrolling
            || self.touchpad_tap != previous.touchpad_tap
        {
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
        }
        if self.ethernet_enabled != previous.ethernet_enabled
            || self.wifi_enabled != previous.wifi_enabled
            || self.network_mode != previous.network_mode
            || self.ip_address != previous.ip_address
            || self.dns_server != previous.dns_server
            || self.proxy != previous.proxy
            || self.proxy_enabled != previous.proxy_enabled
        {
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
        }
        if self.auto_login != previous.auto_login
            || self.auto_login_user != previous.auto_login_user
        {
            write_config(
                "account",
                format!(
                    "auto_login={}\nauto_login_user={}\n",
                    self.auto_login,
                    single_line(&self.auto_login_user),
                ),
            )?;
        }
        if self.unsigned_policy != previous.unsigned_policy {
            write_config(
                "security",
                format!("unsigned_policy={}\n", self.unsigned_policy,),
            )?;
        }
        if self.diagnostics_enabled != previous.diagnostics_enabled
            || self.diagnostics_consent != previous.diagnostics_consent
        {
            write_config(
                "diagnostics",
                format!(
                    "diagnostics_enabled={}\ndiagnostics_consent={}\n",
                    self.diagnostics_enabled,
                    self.diagnostics_enabled && self.diagnostics_consent,
                ),
            )?;
        }
        Ok(())
    }
}

fn config_path(category: &str) -> String {
    format!("{CONFIG_ROOT}/{category}/settings.conf")
}

fn write_config(category: &str, contents: String) -> io::Result<()> {
    write_config_to(Path::new(CONFIG_ROOT), category, contents.as_bytes())
}

fn write_config_to(root: &Path, category: &str, contents: &[u8]) -> io::Result<()> {
    let parent = root.join(category);
    if !parent.is_dir() {
        fs::create_dir_all(&parent)?;
    }
    recover_config(root, category)?;
    let path = parent.join("settings.conf");
    let backup = parent.join(".settings.conf.backup");
    let temp = parent.join(format!(
        ".settings.conf-{}-{}.tmp",
        std::process::id(),
        NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed),
    ));
    let result = (|| {
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
        file.write_all(contents)?;
        drop(file);
        let mut mode = fs::metadata(&path)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .unwrap_or(0o644);
        if category == "account" {
            mode |= 0o444;
        }
        fs::set_permissions(&temp, fs::Permissions::from_mode(mode))?;
        let had_original = path.exists();
        if had_original {
            fs::rename(&path, &backup)?;
        }
        if let Err(error) = fs::rename(&temp, &path) {
            if had_original {
                let _ = fs::rename(&backup, &path);
            }
            return Err(error);
        }
        if had_original {
            let _ = fs::remove_file(&backup);
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn recover_config(root: &Path, category: &str) -> io::Result<()> {
    let parent = root.join(category);
    let path = parent.join("settings.conf");
    let backup = parent.join(".settings.conf.backup");
    if backup.exists() {
        if path.exists() {
            fs::remove_file(backup)?;
        } else {
            fs::rename(backup, path)?;
        }
    }
    Ok(())
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

    #[test]
    fn diagnostics_is_offline_until_explicit_consent() {
        let defaults = Preferences::default();
        assert!(defaults.diagnostics_enabled);
        assert!(!defaults.diagnostics_consent);
    }

    #[test]
    fn replaces_read_only_config_file_without_truncating_it() {
        let root = std::env::temp_dir().join(format!(
            "mochios-settings-test-{}-{}",
            std::process::id(),
            NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed),
        ));
        let category = root.join("diagnostics");
        fs::create_dir_all(&category).unwrap();
        let path = category.join("settings.conf");
        fs::write(&path, b"diagnostics_consent=false\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
        write_config_to(&root, "diagnostics", b"diagnostics_consent=true\n").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"diagnostics_consent=true\n");
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o444);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovers_config_interrupted_between_backup_and_replace() {
        let root = std::env::temp_dir().join(format!(
            "mochios-settings-test-{}-{}",
            std::process::id(),
            NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed),
        ));
        let category = root.join("general");
        fs::create_dir_all(&category).unwrap();
        fs::write(category.join(".settings.conf.backup"), b"device_name=mochiOS\n").unwrap();
        recover_config(&root, "general").unwrap();
        assert_eq!(fs::read(category.join("settings.conf")).unwrap(), b"device_name=mochiOS\n");
        assert!(!category.join(".settings.conf.backup").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn writes_to_existing_category_without_creating_its_parent() {
        let root = std::env::temp_dir().join(format!(
            "mochios-settings-test-{}-{}",
            std::process::id(),
            NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed),
        ));
        let category = root.join("general");
        fs::create_dir_all(&category).unwrap();
        fs::set_permissions(&category, fs::Permissions::from_mode(0o777)).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o555)).unwrap();
        let result = write_config_to(&root, "general", b"device_name=mochiOS\n");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        result.unwrap();
        assert_eq!(fs::read(category.join("settings.conf")).unwrap(), b"device_name=mochiOS\n");
        fs::remove_dir_all(root).unwrap();
    }
}
