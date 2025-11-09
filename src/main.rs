// Xephyr -br -ac -noreset -screen 800x600 :1

mod actions;
mod config;
mod keys;
mod state;
use crate::{actions::ConnectionHandler, state::*};
use std::{thread, time::Duration};
use x11rb::{connection::Connection, errors::ReplyOrIdError};

trait ErrorPrinter {
    fn print(self);
}

impl ErrorPrinter for Result<(), ReplyOrIdError> {
    fn print(self) {
        let error = match self {
            Ok(_) => return,
            Err(e) => e,
        };

        log::error!("got error: {:?}", error);
        match error {
            ReplyOrIdError::X11Error(e) => log::error!("x11 error {:?}", e),
            ReplyOrIdError::IdsExhausted => log::error!("ids exhausted"),
            ReplyOrIdError::ConnectionError(e) => log::error!("connection error {:?}", e),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_default_env()
        .target(env_logger::Target::Stdout)
        .init();
    let (connection, screen_num) = x11rb::connect(None)?;
    let handler = ConnectionHandler::new(&connection, screen_num)?;
    handler.become_window_manager().print();
    handler.grab_keys()?;
    handler.set_cursor()?;

    let mut wm_state = ManagerState::new(&handler)?;

    handler.create_bar_window(wm_state.bar.window).print();
    handler.create_frame_of_window(&wm_state.bar).print();
    handler.draw_bar(&wm_state, None)?;

    let bar_window = wm_state.bar.clone();

    thread::spawn(move || -> Result<(), ReplyOrIdError> {
        let (conn, s) = match x11rb::connect(None) {
            Ok((c, s)) => (c, s),
            Err(_) => {
                return Err(ReplyOrIdError::ConnectionError(
                    x11rb::errors::ConnectionError::UnknownError,
                ));
            }
        };

        let other_handler = match ConnectionHandler::new(&conn, s) {
            Ok(h) => h,
            Err(e) => {
                return Err(e);
            }
        };

        loop {
            other_handler.draw_status_bar(&bar_window)?;
            thread::sleep(Duration::from_secs(1));
        }
    });

    loop {
        connection.flush()?;
        let event = connection.wait_for_event()?;
        let mut event_as_option = Some(event);

        while let Some(event) = event_as_option {
            wm_state.handle_event(&handler, event.clone()).print();
            event_as_option = connection.poll_for_event()?;
        }
    }
}
