use serde::Deserialize;

use crate::keys::HotkeyAction;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub bar_height: u16,
    pub spacing: u16,
    pub ratio: f32,
    pub border_size: u16,
    pub main_color: String,
    pub secondary_color: String,
    pub fonts: Vec<String>,
    pub hotkeys: Vec<HotkeyConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HotkeyConfig {
    pub modifier: String,
    pub key: String,
    pub action: HotkeyAction,
}

impl Config {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(toml::from_str(&std::fs::read_to_string("config.toml")?)?)
    }
    pub fn default() -> Self {
        Config {
            bar_height: 20,
            spacing: 10,
            ratio: 0.5,
            border_size: 1,
            main_color: String::from("#11111b"),
            secondary_color: String::from("#74c7ec"),
            fonts: vec![
                String::from("-misc-jetbrainsmononl nfp-medium-r-normal--20-0-0-0-p-0-koi8-e"),
                String::from(
                    "-misc-jetbrainsmononl nfp medium-medium-r-normal--0-0-0-0-p-0-iso8859-16",
                ),
                String::from("6x13"),
                String::from("fixed"),
            ],
            hotkeys: vec![
                HotkeyConfig {
                    modifier: "CONTROL|MOD".to_string(),
                    key: "Return".to_string(),
                    action: HotkeyAction::Spawn("alacritty".to_string()),
                },
                HotkeyConfig {
                    modifier: "MOD".to_string(),
                    key: "q".to_string(),
                    action: HotkeyAction::ExitFocusedWindow,
                },
                HotkeyConfig {
                    modifier: "CONTROL|MOD".to_string(),
                    key: "q".to_string(),
                    action: HotkeyAction::Spawn("killall rust_wm".to_string()),
                },
                HotkeyConfig {
                    modifier: "MOD".to_string(),
                    key: "c".to_string(),
                    action: HotkeyAction::Spawn("rofi -show drun".to_string()),
                },
                // tag switch
                HotkeyConfig {
                    modifier: "MOD".to_string(),
                    key: "1".to_string(),
                    action: HotkeyAction::SwitchTag(1),
                },
                HotkeyConfig {
                    modifier: "MOD".to_string(),
                    key: "2".to_string(),
                    action: HotkeyAction::SwitchTag(2),
                },
                HotkeyConfig {
                    modifier: "MOD".to_string(),
                    key: "3".to_string(),
                    action: HotkeyAction::SwitchTag(3),
                },
                HotkeyConfig {
                    modifier: "MOD".to_string(),
                    key: "4".to_string(),
                    action: HotkeyAction::SwitchTag(4),
                },
                HotkeyConfig {
                    modifier: "MOD".to_string(),
                    key: "5".to_string(),
                    action: HotkeyAction::SwitchTag(5),
                },
                HotkeyConfig {
                    modifier: "MOD".to_string(),
                    key: "6".to_string(),
                    action: HotkeyAction::SwitchTag(6),
                },
                HotkeyConfig {
                    modifier: "MOD".to_string(),
                    key: "7".to_string(),
                    action: HotkeyAction::SwitchTag(7),
                },
                HotkeyConfig {
                    modifier: "MOD".to_string(),
                    key: "8".to_string(),
                    action: HotkeyAction::SwitchTag(8),
                },
                HotkeyConfig {
                    modifier: "MOD".to_string(),
                    key: "9".to_string(),
                    action: HotkeyAction::SwitchTag(9),
                },
                // move window to tag
                HotkeyConfig {
                    modifier: "MOD|SHIFT".to_string(),
                    key: "1".to_string(),
                    action: HotkeyAction::MoveWindow(1),
                },
                HotkeyConfig {
                    modifier: "MOD|SHIFT".to_string(),
                    key: "2".to_string(),
                    action: HotkeyAction::MoveWindow(2),
                },
                HotkeyConfig {
                    modifier: "MOD|SHIFT".to_string(),
                    key: "3".to_string(),
                    action: HotkeyAction::MoveWindow(3),
                },
                HotkeyConfig {
                    modifier: "MOD|SHIFT".to_string(),
                    key: "4".to_string(),
                    action: HotkeyAction::MoveWindow(4),
                },
                HotkeyConfig {
                    modifier: "MOD|SHIFT".to_string(),
                    key: "5".to_string(),
                    action: HotkeyAction::MoveWindow(5),
                },
                HotkeyConfig {
                    modifier: "MOD|SHIFT".to_string(),
                    key: "6".to_string(),
                    action: HotkeyAction::MoveWindow(6),
                },
                HotkeyConfig {
                    modifier: "MOD|SHIFT".to_string(),
                    key: "7".to_string(),
                    action: HotkeyAction::MoveWindow(7),
                },
                HotkeyConfig {
                    modifier: "MOD|SHIFT".to_string(),
                    key: "8".to_string(),
                    action: HotkeyAction::MoveWindow(8),
                },
                HotkeyConfig {
                    modifier: "MOD|SHIFT".to_string(),
                    key: "9".to_string(),
                    action: HotkeyAction::MoveWindow(9),
                },
                // media controls
                HotkeyConfig {
                    modifier: "".to_string(),
                    key: "XF86_AudioRaiseVolume".to_string(),
                    action: HotkeyAction::Spawn("/usr/bin/pactl set-sink-volume 0 +5%".to_string()),
                },
                HotkeyConfig {
                    modifier: "".to_string(),
                    key: "XF86_AudioLowerVolume".to_string(),
                    action: HotkeyAction::Spawn("/usr/bin/pactl set-sink-volume 0 -5%".to_string()),
                },
                HotkeyConfig {
                    modifier: "".to_string(),
                    key: "XF86_AudioMute".to_string(),
                    action: HotkeyAction::Spawn(
                        "/usr/bin/pactl set-sink-mute 0 toggle".to_string(),
                    ),
                },
                HotkeyConfig {
                    modifier: "".to_string(),
                    key: "XF86_MonBrightnessUp".to_string(),
                    action: HotkeyAction::Spawn("sudo light -A 5".to_string()),
                },
                HotkeyConfig {
                    modifier: "".to_string(),
                    key: "XF86_MonBrightnessDown".to_string(),
                    action: HotkeyAction::Spawn("sudo light -U 5".to_string()),
                },
            ],
        }
    }
}
