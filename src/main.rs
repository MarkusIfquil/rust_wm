mod actions;
mod config;
mod keys;
mod state;

use std::thread;
use std::time::Duration;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, CreateGCAux};

use crate::actions::ConnectionHandler;
use crate::state::*;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (connection, screen_num) = x11rb::connect(None)?;
    let handler = ConnectionHandler::new(&connection, screen_num)?;
    handler.become_window_manager()?;
    let mut wm_state = ManagerState::new(&handler)?;

    handler.create_bar_window(wm_state.bar.window)?;
    handler.create_frame_of_window(&wm_state.bar)?;
    handler.draw_bar(&wm_state, None)?;

    // wm_state = wm_state.scan_for_new_windows()?;

    let bar_window = wm_state.bar.clone();
    let gc = CreateGCAux::new()
        .graphics_exposures(0)
        .background(handler.graphics.0)
        .foreground(handler.graphics.1);
    thread::spawn(move || {
        let (conn, _) = x11rb::connect(None).unwrap();
        let id = conn.generate_id().unwrap();

        conn.create_gc(id, bar_window.window, &gc).unwrap();
        loop {
            actions::draw_time_on_bar(&conn, &bar_window, id);
            thread::sleep(Duration::from_secs(1));
        }
    });

    loop {
        wm_state = wm_state.clear_exposed_events()?;
        connection.flush()?;
        let event = connection.wait_for_event()?;
        let mut event_as_option = Some(event);

        while let Some(event) = event_as_option {
            // println!("got event {:?}", event);
            handler.handle_event(&wm_state, event.clone())?;
            wm_state = wm_state.handle_event(event.clone())?;
            event_as_option = connection.poll_for_event()?;
        }
    }
}
