// Xephyr -br -ac -noreset -screen 800x600 :1

mod actions;
mod config;
mod keys;
mod state;
use crate::{
    actions::ConnectionHandler,
    config::{Config, ConfigDeserialized},
    keys::KeyHandler,
    state::*,
};
use std::{sync::mpsc, thread, time::Duration};
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

    let (conn, screen_num) = x11rb::connect(None)?;
    let config = Config::from(ConfigDeserialized::new());
    let handler = ConnectionHandler::new(&conn, screen_num, &config)?;
    let key_handler = KeyHandler::new(&conn, &config)?;
    let mut wm_state = ManagerState::new(&handler)?;
    handler.draw_bar(&wm_state, None)?;
    
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || -> Result<(), ReplyOrIdError> {
        loop {
            let _ = tx.send(1);
            thread::sleep(Duration::from_secs(1));
        }
    });

    loop {
        if let Ok(_) = rx.try_recv() {
            handler.draw_status_bar()?;
        }
        conn.flush()?;
        let event = conn.wait_for_event()?;
        let mut event_as_option = Some(event);

        while let Some(event) = event_as_option {
            wm_state.handle_event(&handler, &key_handler, event).print();
            event_as_option = conn.poll_for_event()?;
        }
    }
}
