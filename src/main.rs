mod actions;
mod keys;
mod state;

// use std::thread;
// use std::time::Duration;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::KeyButMask;
use xkeysym::Keysym;

use crate::actions::ConnectionHandler;
use crate::keys::{Hotkey, HotkeyAction};
use crate::state::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (connection, screen_num) = x11rb::connect(None)?;
    let handler = ConnectionHandler::new(&connection, screen_num)?;
    handler.become_window_manager()?;
    let mut wm_state = ManagerState::new(&handler)?;

    let hotkeys: Vec<Hotkey> = vec![
        Hotkey::new(
            Keysym::Return,
            KeyButMask::CONTROL | KeyButMask::MOD4,
            &wm_state.key_handler,
            HotkeyAction::SpawnAlacritty,
        )?,
        Hotkey::new(
            Keysym::q,
            KeyButMask::MOD4,
            &wm_state.key_handler,
            HotkeyAction::ExitFocusedWindow,
        )?,
    ]
    .into_iter()
    .chain((1..=9).map(|n| {
        Hotkey::new(
            Keysym::from_char(char::from_digit(n, 10).unwrap()),
            KeyButMask::MOD4,
            &wm_state.key_handler,
            HotkeyAction::SwitchTag(n as u16),
        )
        .unwrap()
    }))
    .collect();

    handler.create_bar_window(wm_state.bar.window)?;
    handler.create_frame_of_window(&wm_state.bar)?;
    wm_state = wm_state
        .scan_for_new_windows()?
        .add_hotkeys(hotkeys.into())?;

    println!("screen num: {}", handler.screen.root);
    println!(
        "screen: w{} h{}",
        handler.screen.width_in_pixels, handler.screen.height_in_pixels
    );

    loop {
        wm_state = wm_state.clear_exposed_events()?;
        connection.flush()?;

        let event = connection.wait_for_event()?;
        let mut event_as_option = Some(event);

        while let Some(event) = event_as_option {
            // println!("got event {:?}", event);
            handler.handle_event(&wm_state, event.clone())?;
            wm_state = wm_state.handle_event(event.clone())?;
            // thread::sleep(Duration::from_millis(200));
            event_as_option = connection.poll_for_event()?;
        }
    }
}
