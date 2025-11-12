// Xephyr -br -ac -noreset -screen 800x600 :1

mod actions;
mod config;
mod events;
mod keys;
mod state;
use crate::{
    actions::ConnectionHandler,
    config::{Config, ConfigDeserialized},
    events::EventHandler,
    keys::KeyHandler,
    state::*,
};
use std::{sync::mpsc, thread, time::Duration};
use x11rb::{connection::Connection, errors::ReplyOrIdError};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_default_env()
        .target(env_logger::Target::Stdout)
        .init();

    let (conn, screen_num) = x11rb::connect(None)?;
    let config = Config::from(ConfigDeserialized::new());
    let conn_handler = ConnectionHandler::new(&conn, screen_num, &config)?;
    let key_handler = KeyHandler::new(&conn, &config)?;
    let manager = StateHandler::new(TilingInfo {
        gap: config.spacing as u16,
        ratio: config.ratio,
        width: conn_handler.screen.width_in_pixels,
        height: conn_handler.screen.height_in_pixels,
        bar_height: conn_handler.bar.height,
    });

    conn_handler.draw_bar(&manager, None)?;

    let mut event_handler = EventHandler {
        conn: &conn_handler,
        man: manager,
        key: key_handler,
    };

    let (tx, rx) = mpsc::channel();

    thread::spawn(move || -> Result<(), ReplyOrIdError> {
        loop {
            let _ = tx.send(1);
            thread::sleep(Duration::from_secs(1));
        }
    });

    loop {
        if let Ok(_) = rx.try_recv() {
            conn_handler.draw_status_bar()?;
        }
        conn.flush()?;
        let event = conn.wait_for_event()?;
        let mut event_as_option = Some(event);

        while let Some(event) = event_as_option {
            match event_handler.handle_event(event) {
                Ok(_) => (),
                Err(e) => log::error!("{}", e),
            };
            event_as_option = conn.poll_for_event()?;
        }
    }
}
