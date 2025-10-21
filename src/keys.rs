use x11rb::{
    connection::Connection,
    errors::ReplyOrIdError,
    protocol::xproto::{
        ConnectionExt, GetKeyboardMappingReply, GrabMode, KeyButMask, ModMask, Window,
    },
};
use xkeysym::{KeyCode, Keysym, keysym};
#[derive(Debug)]
pub enum HotkeyAction {
    SpawnAlacritty,
    ExitFocusedWindow,
    SwitchTag(u16),
}
#[derive(Debug)]
pub struct Hotkey {
    pub main_key: Keysym,
    pub code: KeyCode,
    pub mask: KeyButMask,
    modifier: ModMask,
    pub action: HotkeyAction,
}

impl Hotkey {
    pub fn new<C: Connection>(
        sym: Keysym,
        mask: KeyButMask,
        handler: &KeyHandler<C>,
        action: HotkeyAction,
    ) -> Result<Self, ReplyOrIdError>
    {
        Ok(Hotkey {
            main_key: sym,
            code: sym_to_code(handler, &sym).unwrap(),
            mask: mask,
            modifier: ModMask::from(mask.bits()),
            action: action,
        })
    }
}

pub struct KeyHandler<'a, C: Connection> {
    connection: &'a C,
    root: Window,
    mapping: GetKeyboardMappingReply,
    min_code: u8,
    max_code: u8,
    pub hotkeys: Vec<Hotkey>,
}

impl<'a, C: Connection> KeyHandler<'a, C> {
    pub fn new(connection: &'a C, root: Window) -> Result<Self, ReplyOrIdError> {
        Ok(KeyHandler {
            connection: connection,
            hotkeys: Vec::default(),
            mapping: connection
                .get_keyboard_mapping(
                    connection.setup().min_keycode,
                    connection.setup().max_keycode - connection.setup().min_keycode + 1,
                )?
                .reply()?,
            min_code: connection.setup().min_keycode,
            max_code: connection.setup().max_keycode - connection.setup().min_keycode + 1,
            root: root,
        })
    }
    pub fn add_hotkey(mut self, hotkey: Hotkey) -> Result<Self, ReplyOrIdError> {
        listen_to_hotkey(self.connection, self.root, &hotkey)?;
        self.hotkeys.push(hotkey);
        Ok(self)
    }
}

pub fn code_to_sym<C: Connection>(
    handler: &KeyHandler<C>,
    code: u8,
) -> Option<Keysym> {
    xkeysym::keysym(
        code.into(),
        0,
        handler.min_code.into(),
        handler.mapping.keysyms_per_keycode,
        handler.mapping.keysyms.as_slice(),
    )
}

fn sym_to_code<C: Connection>(
    handler: &KeyHandler<C>,
    sym: &Keysym,
) -> Option<KeyCode> {
    for i in handler.min_code..=handler.max_code {
        if let Some(s) = code_to_sym(handler, i) {
            if s == *sym {
                return Some(KeyCode::new(i.into()));
            }
        }
    }
    None
}

fn listen_to_hotkey<C: Connection>(
    connection: C,
    root: Window,
    hotkey: &Hotkey,
) -> Result<(), ReplyOrIdError> {
    connection.grab_key(
        true,
        root,
        hotkey.modifier,
        hotkey.code,
        GrabMode::ASYNC,
        GrabMode::ASYNC,
    )?;
    Ok(())
}
