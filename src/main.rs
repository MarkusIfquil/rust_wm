mod actions;
mod state;
mod keys;

use x11rb::connection::Connection;

use crate::actions::*;
use crate::state::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (connection, screen_num) = x11rb::connect(None)?;
    let mut wm_state = WindowManagerState::new(&connection, screen_num)?;
    become_window_manager(&connection, wm_state.screen)?;
    // connection.grab_key(
    //     true,
    //     wm_state.screen.root,
    //     ModMask::M4,
    //     46,
    //     GrabMode::ASYNC,
    //     GrabMode::ASYNC,
    // )?;
    
    let bar = wm_state.bar;
    create_and_map_window(&mut wm_state, &bar)?;
    
    println!("screen num: {}", wm_state.screen.root);
    println!(
        "screen: w{} h{}",
        wm_state.screen.width_in_pixels, wm_state.screen.height_in_pixels
    );

    wm_state = wm_state.scan_for_new_windows()?;
    loop {
        wm_state = wm_state.refresh()?;
        connection.flush()?;

        let event = connection.wait_for_event()?;
        let mut event_as_option = Some(event);

        while let Some(event) = event_as_option {
            wm_state = wm_state.handle_event(event)?;
            event_as_option = connection.poll_for_event()?;
        }
    }
}
