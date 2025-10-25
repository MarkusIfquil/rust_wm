mod actions;
mod state;
mod keys;
mod config;

use std::thread;
use std::time::Duration;

use x11rb::connection::Connection;

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

    let bar_window = wm_state.bar.clone();
    
    thread::spawn(move || {
        let (conn, s) = x11rb::connect(None).unwrap();
        let other_handler = ConnectionHandler::new(&conn, s).unwrap();


        loop {
            other_handler.draw_time_on_bar(&bar_window, other_handler.id_graphics_context);
            thread::sleep(Duration::from_secs(1));
        }
    });

    loop {
        wm_state = wm_state.clear_exposed_events()?;
        connection.flush()?;
        let event = connection.wait_for_event()?;
        let mut event_as_option = Some(event);

        while let Some(event) = event_as_option {
            handler.handle_event(&wm_state, event.clone())?;
            wm_state = wm_state.handle_event(event.clone())?;
            event_as_option = connection.poll_for_event()?;
        }
    }
}
