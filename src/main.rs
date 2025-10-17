mod actions;
mod keys;
mod state;

use std::thread;
use std::time::Duration;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::KeyButMask;
use xkeysym::Keysym;

use crate::actions::*;
use crate::keys::{Hotkey, HotkeyAction};
use crate::state::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (connection, screen_num) = x11rb::connect(None)?;
    let mut wm_state = WindowManagerState::new(&connection, screen_num)?;
    become_window_manager(&connection, wm_state.screen)?;

    let hotkeys: Vec<Hotkey> = vec![
        Hotkey::new(
            Keysym::Return,
            KeyButMask::CONTROL | KeyButMask::MOD4,
            &wm_state.key_state,
            HotkeyAction::SpawnAlacritty,
        )?,
        Hotkey::new(
            Keysym::q,
            KeyButMask::MOD4,
            &wm_state.key_state,
            HotkeyAction::ExitFocusedWindow,
        )?,
    ]
    .into_iter()
    .chain((1..=9).map(|n| {
        Hotkey::new(
            Keysym::from_char(char::from_digit(n, 10).unwrap()),
            KeyButMask::MOD4,
            &wm_state.key_state,
            HotkeyAction::SwitchTag(n as u16),
        )
        .unwrap()
    }))
    .collect();

    let bar = wm_state.bar;
    wm_state = create_and_map_window(wm_state, &bar)?
    .scan_for_new_windows()?
    .add_hotkeys(hotkeys.into())?;

    println!("screen num: {}", wm_state.screen.root);
    println!(
        "screen: w{} h{}",
        wm_state.screen.width_in_pixels, wm_state.screen.height_in_pixels
    );

    loop {
        wm_state = wm_state.clear_exposed_events()?;
        connection.flush()?;

        let event = connection.wait_for_event()?;
        let mut event_as_option = Some(event);

        while let Some(event) = event_as_option {
            println!("got event {:?}",event);
            wm_state = crate::actions::handle_event(wm_state, event.clone())?
            .handle_event(event)?;
            // thread::sleep(Duration::from_millis(200));
            event_as_option = connection.poll_for_event()?;
        }
    }
}
