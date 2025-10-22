use x11rb::{
    connection::Connection,
    errors::ReplyOrIdError,
    protocol::xproto::{
        ConnectionExt, GetKeyboardMappingReply, GrabMode, KeyButMask, ModMask, Window,
    },
};
use xkeysym::{KeyCode, Keysym};
#[derive(Debug, Clone)]
pub enum HotkeyAction {
    Spawn(String),
    ExitFocusedWindow,
    SwitchTag(u16),
}
#[derive(Debug)]
pub struct Hotkey {
    pub sym: Keysym,
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
            sym,
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
        KeyHandler {
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
        }
        .get_hotkeys()
    }

    pub fn get_registered_hotkey(&self, mask: KeyButMask, code_raw: u32) -> Result<&Hotkey,ReplyOrIdError> {
        self.hotkeys
            .iter()
            .find(|h| mask == h.mask && code_raw == h.code.raw())
            .ok_or(ReplyOrIdError::IdsExhausted)
    }

    pub fn get_hotkeys(self) -> Result<Self, ReplyOrIdError> {
        let hotkeys = vec![
            Hotkey::new(
                &self,
                Keysym::Return,
                KeyButMask::CONTROL | KeyButMask::MOD4,
                HotkeyAction::Spawn(String::from("alacritty")),
            ),
            Hotkey::new(
                &self,
                Keysym::q,
                KeyButMask::MOD4,
                HotkeyAction::ExitFocusedWindow,
            ),
            Hotkey::new(
                &self,
                Keysym::c,
                KeyButMask::MOD4,
                HotkeyAction::Spawn(String::from("/usr/bin/rofi -show drun")),
            ),
        ]
        .into_iter()
        .chain((1..=9).map(|n| {
            Hotkey::new(
                &self,
                Keysym::from_char(char::from_digit(n, 10).unwrap()),
                KeyButMask::MOD4,
                HotkeyAction::SwitchTag(n as u16),
            )
        }))
        .collect::<Vec<_>>();

        hotkeys.iter().try_for_each(|h| self.listen_to_hotkey(h))?;

        Ok(Self { hotkeys, ..self })
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
