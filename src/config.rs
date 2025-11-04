use std::num::ParseIntError;
use serde::Deserialize;
use crate::keys::HotkeyAction;

pub const SPACING: u32 = 10;
pub const RATIO: f32 = 0.5;
pub const BORDER_SIZE: u32 = 1;
pub const MAIN_COLOR: (u16, u16, u16) = (4369, 4369, 6939); // #11111b
pub const SECONDARY_COLOR: (u16, u16, u16) = (29812, 51143, 60652); // #74c7ec
pub const FONT: &'static str = "fixed";

fn hex_color_to_rgb(hex: &str) -> Result<(u16, u16, u16), ParseIntError> {
    Ok((
        u16::from_str_radix(&hex[1..3], 16)? * 257,
        u16::from_str_radix(&hex[3..5], 16)? * 257,
        u16::from_str_radix(&hex[5..7], 16)? * 257,
    ))
}

#[derive(Clone)]
pub struct Config {
    pub spacing: u32,
    pub ratio: f32,
    pub border_size: u32,
    pub main_color: (u16, u16, u16),
    pub secondary_color: (u16, u16, u16),
    pub font: String,
    pub hotkeys: Vec<HotkeyConfig>,
}

impl From<ConfigDeserialized> for Config {
    fn from(config: ConfigDeserialized) -> Self {
        let main_color = match hex_color_to_rgb(&config.colors.main_color) {
            Ok(c) => c,
            Err(_) => {
                println!("BAD COLOR VALUE");
                MAIN_COLOR
            }
        };
        let secondary_color = match hex_color_to_rgb(&config.colors.secondary_color) {
            Ok(c) => c,
            Err(_) => {
                println!("BAD COLOR VALUE");
                SECONDARY_COLOR
            }
        };

        Self {
            main_color,
            secondary_color,
            spacing: config.sizing.spacing.clamp(0, 1000),
            ratio: config.sizing.ratio.clamp(0.0, 1.0),
            border_size: config.sizing.border_size.clamp(0, 1000),
            font: config.font.font,
            hotkeys: config.hotkeys,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ConfigDeserialized {
    sizing: Sizing,
    colors: Colors,
    font: Font,
    hotkeys: Vec<HotkeyConfig>,
}

#[derive(Debug, Deserialize)]
struct Sizing {
    spacing: u32,
    ratio: f32,
    border_size: u32,
}

#[derive(Debug, Deserialize)]
struct Colors {
    main_color: String,
    secondary_color: String,
}

#[derive(Debug, Deserialize)]
struct Font {
    font: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HotkeyConfig {
    pub modifiers: String,
    pub key: String,
    pub action: HotkeyAction,
}

impl ConfigDeserialized {
    pub fn new() -> Self {
        let config_str = match std::fs::read_to_string("/home/Markus/.config/rwm/config.toml") {
            Ok(s) => s,
            Err(e) => {
                println!("CONFIG FILE ERROR {e:?}");
                return Self::default();
            }
        };
        match toml::from_str(&config_str) {
            Ok(d) => d,
            Err(e) => {
                println!("ERROR PARSING CONFIG {e:?}");
                Self::default()
            }
        }
    }
    fn default() -> Self {
        println!("USING DEFAULT CONFIG");
        let mut hotkeys = vec![
            HotkeyConfig {
                modifiers: "CONTROL|MOD".to_string(),
                key: "Return".to_string(),
                action: HotkeyAction::Spawn("alacritty".to_string()),
            },
            HotkeyConfig {
                modifiers: "MOD".to_string(),
                key: "q".to_string(),
                action: HotkeyAction::ExitFocusedWindow,
            },
            HotkeyConfig {
                modifiers: "CONTROL|MOD".to_string(),
                key: "q".to_string(),
                action: HotkeyAction::Spawn("killall rust_wm".to_string()),
            },
            HotkeyConfig {
                modifiers: "MOD".to_string(),
                key: "c".to_string(),
                action: HotkeyAction::Spawn("rofi -show drun".to_string()),
            },
            HotkeyConfig {
                modifiers: "MOD".to_string(),
                key: "u".to_string(),
                action: HotkeyAction::Spawn(
                    "maim --select | xclip -selection clipboard -t image/png".to_string(),
                ),
            }, // tag switch
        ];
        hotkeys.extend(
            (1..=9)
                .map(|x| HotkeyConfig {
                    modifiers: "MOD".to_string(),
                    key: x.to_string(),
                    action: HotkeyAction::SwitchTag(x),
                })
                .chain((1..=9).map(|x| HotkeyConfig {
                    modifiers: "MOD|SHIFT".to_string(),
                    key: x.to_string(),
                    action: HotkeyAction::MoveWindow(x),
                }))
                .collect::<Vec<_>>(),
        );

        ConfigDeserialized {
            sizing: Sizing {
                spacing: SPACING,
                ratio: RATIO,
                border_size: BORDER_SIZE,
            },
            colors: Colors {
                main_color: String::from("#11111b"),
                secondary_color: String::from("#74c7ec"),
            },
            font: Font {
                font: FONT.to_owned(),
            },
            hotkeys,
        }
    }
}
