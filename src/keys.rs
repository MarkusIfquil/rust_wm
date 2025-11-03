use std::collections::HashMap;

use serde::Deserialize;
use x11rb::{
    connection::Connection,
    errors::ReplyOrIdError,
    protocol::xproto::{ConnectionExt, KeyButMask, KeyPressEvent, ModMask},
};
use xkeysym::{KeyCode, Keysym};

use crate::config::Config;
#[derive(Debug, Clone, Deserialize)]
pub enum HotkeyAction {
    Spawn(String),
    ExitFocusedWindow,
    SwitchTag(u16),
    MoveWindow(u16),
}
#[derive(Debug)]
pub struct Hotkey {
    _sym: Keysym,
    pub code: KeyCode,
    mask: KeyButMask,
    pub modifier: ModMask,
    action: HotkeyAction,
}

pub struct KeyHandler {
    pub hotkeys: Vec<Hotkey>,
}

impl KeyHandler {
    pub fn new<C: Connection>(connection: &C, config: &Config) -> Result<Self, ReplyOrIdError> {
        let min = connection.setup().min_keycode;
        let max = connection.setup().max_keycode;

        let mapping = connection
            .get_keyboard_mapping(min, max - min + 1)?
            .reply()?;

        let sym_code: HashMap<Keysym, KeyCode> = (min..=max)
            .filter_map(|x| {
                if let Some(s) = xkeysym::keysym(
                    x.into(),
                    0,
                    min.into(),
                    mapping.keysyms_per_keycode,
                    mapping.keysyms.as_slice(),
                ) {
                    Some((s, KeyCode::new(x.into())))
                } else {
                    None
                }
            })
            .collect();

        let hotkeys: Vec<Hotkey> = config
            .hotkeys
            .iter()
            .cloned()
            .map(|c| {
                let modi = c
                    .modifier
                    .split("|")
                    .map(|m| match m {
                        "CONTROL" => KeyButMask::CONTROL,
                        "SHIFT" => KeyButMask::SHIFT,
                        "MOD" => KeyButMask::MOD4,
                        _ => KeyButMask::default(),
                    })
                    .fold(KeyButMask::default(), |acc, m| acc | m);

                let sym = match c.key.as_str() {
                    "Return" => Keysym::Return,
                    "XF86_MonBrightnessUp" => Keysym::XF86_MonBrightnessUp,
                    "XF86_MonBrightnessDown" => Keysym::XF86_MonBrightnessDown,
                    "XF86_AudioRaiseVolume" => Keysym::XF86_AudioRaiseVolume,
                    "XF86_AudioLowerVolume" => Keysym::XF86_AudioLowerVolume,
                    "XF86_AudioMute" => Keysym::XF86_AudioMute,
                    c => Keysym::from_char(c.chars().next().unwrap_or_default()),
                };

                Hotkey {
                    _sym: sym,
                    code: *sym_code.get(&sym).expect("expected sym to have code"),
                    mask: modi,
                    modifier: ModMask::from(modi.bits()),
                    action: c.action,
                }
            })
            .collect();

        Ok(KeyHandler { hotkeys })
    }

    fn get_registered_hotkey(&self, mask: KeyButMask, code_raw: u32) -> Option<&Hotkey> {
        self.hotkeys
            .iter()
            .find(|h| mask == h.mask && code_raw == h.code.raw())
    }

    pub fn get_action(&self, event: KeyPressEvent) -> Option<HotkeyAction> {
        if let Some(h) = self.get_registered_hotkey(event.state, event.detail as u32) {
            Some(h.action.clone())
        } else {
            None
        }
    }
}
