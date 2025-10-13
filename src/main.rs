mod actions;
mod state;

use std::process::exit;

use x11rb::connection::Connection;
use x11rb::errors::ReplyError;
use x11rb::protocol::ErrorKind;
use x11rb::protocol::xproto::*;

use crate::actions::create_and_map_window;
use crate::state::*;

fn become_window_manager<C: Connection>(connection: &C, screen: &Screen) -> Result<(), ReplyError> {
    let change = ChangeWindowAttributesAux::default()
        .event_mask(EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY);
    let result = connection
        .change_window_attributes(screen.root, &change)?
        .check();
    if let Err(ReplyError::X11Error(ref error)) = result {
        if error.error_kind == ErrorKind::Access {
            println!("another wm is running");
            exit(1);
        } else {
            println!("became wm");
            result
        }
    } else {
        result
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (connection, screen_num) = x11rb::connect(None)?;

    let mut wm_state = WindowManagerState::new(&connection, screen_num)?;
    become_window_manager(&connection, wm_state.screen)?;

    let bar = wm_state.bar;
    create_and_map_window(&mut wm_state, &bar)?;
    
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
        wm_state.draw_bar()?;

        while let Some(event) = event_as_option {
            wm_state = wm_state.handle_event(event)?;
            // thread::sleep(time::Duration::from_millis(1000));
            event_as_option = connection.poll_for_event()?;
        }
    }
}
