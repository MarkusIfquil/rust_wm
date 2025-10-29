mod actions;
mod config;
mod keys;
mod state;

use std::thread;
use std::time::Duration;

use x11rb::connection::Connection;
use x11rb::errors::ReplyOrIdError;

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

    thread::spawn(move || -> Result<(), ReplyOrIdError> {
        let (conn, s) = match x11rb::connect(None) {
            Ok((c, s)) => (c, s),
            Err(_) => return Err(ReplyOrIdError::IdsExhausted),
        };

        let other_handler = match ConnectionHandler::new(&conn, s) {
            Ok(h) => h,
            Err(e) => return Err(e),
        };

        loop {
            other_handler.draw_status_bar(&bar_window, other_handler.id_graphics_context)?;
            thread::sleep(Duration::from_secs(1));
        }
    });

    loop {
        wm_state.pending_exposed_events.clear();
        connection.flush()?;
        let event = connection.wait_for_event()?;
        let mut event_as_option = Some(event);

        while let Some(event) = event_as_option {
            handler.handle_event(&wm_state, event.clone())?;
            wm_state.handle_event(event.clone())?;
            event_as_option = connection.poll_for_event()?;
        }
    }
}
