use x11rb::{
    connection::Connection,
    errors::ReplyOrIdError,
    protocol::xproto::{
        ConnectionExt, GetKeyboardMappingReply, GrabMode, KeyButMask, ModMask, Window,
    },
};
use xkeysym::{KeyCode, Keysym};

use crate::config::Config;
#[derive(Debug, Clone)]
pub enum HotkeyAction {
    Spawn(String),
    ExitFocusedWindow,
    SwitchTag(u16),
    MoveWindow(u16),
}
#[derive(Debug)]
pub struct Hotkey {
    pub _sym: Keysym,
    pub code: KeyCode,
    pub mask: KeyButMask,
    modifier: ModMask,
    pub action: HotkeyAction,
}

impl Hotkey {
    fn new<C: Connection>(
        handler: &KeyHandler<C>,
        sym: Keysym,
        mask: KeyButMask,
        action: HotkeyAction,
    ) -> Self {
        Hotkey {
            _sym: sym,
            code: handler
                .sym_to_code(&sym)
                .expect("expected sym to have code"),
            mask: mask,
            modifier: ModMask::from(mask.bits()),
            action: action,
        }
    }
}

pub struct KeyHandler<'a, C: Connection> {
    mapping: GetKeyboardMappingReply,
    min_code: u8,
    max_code: u8,
    pub hotkeys: Vec<Hotkey>,
    root: Window,
    connection: &'a C,
}

impl<'a, C: Connection> KeyHandler<'a, C> {
    pub fn new(connection: &'a C, root: Window) -> Result<Self, ReplyOrIdError> {
        Ok(KeyHandler {
            hotkeys: Vec::default(),
            mapping: connection
                .get_keyboard_mapping(
                    connection.setup().min_keycode,
                    connection.setup().max_keycode - connection.setup().min_keycode + 1,
                )?
                .reply()?,
            min_code: connection.setup().min_keycode,
            max_code: connection.setup().max_keycode - connection.setup().min_keycode + 1,
            root,
            connection,
        })
    }

    pub fn get_registered_hotkey(&self, mask: KeyButMask, code_raw: u32) -> Option<&Hotkey> {
        self.hotkeys
            .iter()
            .find(|h| mask == h.mask && code_raw == h.code.raw())
    }

    pub fn get_hotkeys(self, config: &Config) -> Result<Self, ReplyOrIdError> {
        let keys = config.hotkeys.iter().map(|h| {
            // println!("got vec {h:?}");
            let modifiers = h[0]
                .split("|")
                .map(|m| match m {
                    "CONTROL" => KeyButMask::CONTROL,
                    "SHIFT" => KeyButMask::SHIFT,
                    "MOD" => KeyButMask::MOD4,
                    _ => KeyButMask::default(),
                })
                .fold(KeyButMask::default(), |acc, m| acc | m);
            let sym = match h[1].as_str() {
                "Return" => Keysym::Return,
                "XF86_MonBrightnessUp" => Keysym::XF86_MonBrightnessUp,
                "XF86_MonBrightnessDown" => Keysym::XF86_MonBrightnessDown,
                "XF86_AudioRaiseVolume" => Keysym::XF86_AudioRaiseVolume,
                "XF86_AudioLowerVolume" => Keysym::XF86_AudioLowerVolume,
                "XF86_AudioMute" => Keysym::XF86_AudioMute,
                c => Keysym::from_char(c.chars().next().unwrap()),
            };
            let action = match h[2].as_str() {
                "spawn" => HotkeyAction::Spawn(h[3].clone()),
                "exit" => HotkeyAction::ExitFocusedWindow,
                "switchtag" => HotkeyAction::SwitchTag(h[3].clone().parse().unwrap()),
                "movewindow" => HotkeyAction::MoveWindow(h[3].clone().parse().unwrap()),
                _ => unimplemented!(),
            };

            // println!("{:?} {:?} {:?}",modifiers,sym,action);

            Hotkey::new(&self, sym, modifiers, action)
        }).collect::<Vec<_>>();

        keys.iter().try_for_each(|h| self.listen_to_hotkey(&h))?;

        Ok(Self {
            hotkeys: keys,
            ..self
        })
    }

    pub fn code_to_sym(&self, code: u8) -> Option<Keysym> {
        xkeysym::keysym(
            code.into(),
            0,
            self.min_code.into(),
            self.mapping.keysyms_per_keycode,
            self.mapping.keysyms.as_slice(),
        )
    }

    fn sym_to_code(&self, sym: &Keysym) -> Option<KeyCode> {
        for i in self.min_code..=self.max_code {
            if let Some(s) = self.code_to_sym(i) {
                if s == *sym {
                    return Some(KeyCode::new(i.into()));
                }
            }
        }
        None
    }

    fn listen_to_hotkey(&self, hotkey: &Hotkey) -> Result<(), ReplyOrIdError> {
        self.connection.grab_key(
            true,
            self.root,
            hotkey.modifier,
            hotkey.code,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
        )?;
        Ok(())
    }
}
