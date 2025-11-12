use x11rb::{
    connection::Connection,
    protocol::{Event, xproto::*},
};

use crate::{
    actions::{ConnectionHandler, Res},
    keys::{HotkeyAction, KeyHandler},
    state::{StateHandler, WindowGroup, WindowState},
};

pub struct EventHandler<'a, C: Connection> {
    pub conn: &'a ConnectionHandler<'a, C>,
    pub man: StateHandler,
    pub key: KeyHandler,
}

impl<'a, C: Connection> EventHandler<'a, C> {
    pub fn handle_event(&mut self, event: Event) -> Res {
        match event {
            Event::MapRequest(e) => {
                self.handle_map_request(e)?;
            }
            Event::UnmapNotify(e) => {
                self.handle_unmap_notify(e)?;
            }
            Event::KeyPress(e) => {
                self.handle_keypress(e)?;
            }
            Event::EnterNotify(e) => {
                self.handle_enter(e)?;
            }
            Event::ConfigureRequest(e) => {
                self.handle_config(e)?;
            }
            Event::ClientMessage(e) => {
                self.handle_client_message(e)?;
            }
            _ => (),
        };
        Ok(())
    }

    fn handle_map_request(&mut self, event: MapRequestEvent) -> Res {
        if let Some(_) = self.man.get_window_state(event.window) {
            return Ok(());
        };

        log::debug!(
            "EVENT MAP window {} parent {} response {}",
            event.window,
            event.parent,
            event.response_type
        );

        let window = WindowState::new(event.window, self.conn.conn.generate_id()?)?;

        self.conn.create_frame_of_window(&window)?;
        self.man.add_window(window);
        self.refresh()
    }

    fn handle_unmap_notify(&mut self, event: UnmapNotifyEvent) -> Res {
        let window = match self.man.get_window_state(event.window) {
            Some(w) => w,
            None => return Ok(()),
        };
        log::debug!(
            "EVENT UNMAP window {} event {} from config {} response {}",
            event.window,
            event.event,
            event.from_configure,
            event.response_type
        );

        //side effect
        self.conn.destroy_window(window)?;

        self.man
            .get_mut_active_tag_windows()
            .retain(|w| w.window != event.window);
        self.man.set_tag_focus_to_master();
        self.refresh()
    }

    fn handle_keypress(&mut self, event: KeyPressEvent) -> Res {
        let action = match self.key.get_action(event) {
            Some(a) => a,
            None => return Ok(()),
        };

        log::debug!(
            "EVENT KEYPRESS code {} sym {:?} action {:?}",
            event.detail,
            event.state,
            action
        );

        match action {
            HotkeyAction::SwitchTag(n) => {
                self.change_active_tag(n - 1)?;
            }
            HotkeyAction::MoveWindow(n) => {
                self.move_window(n - 1)?;
            }
            HotkeyAction::Spawn(command) => {
                crate::actions::spawn_command(&command);
            }
            HotkeyAction::ExitFocusedWindow => {
                let focus = match self.man.get_focus() {
                    Some(f) => f,
                    None => return Ok(()),
                };
                self.conn.kill_focus(focus)?;
            }
            HotkeyAction::ChangeRatio(change) => {
                self.man.tiling.ratio = (self.man.tiling.ratio + change).clamp(0.15, 0.85);
            }
            HotkeyAction::NextFocus(change) => {
                self.man.switch_focus_next(change);
            }
            HotkeyAction::NextTag(change) => {
                self.change_active_tag(
                    (self.man.active_tag as i16 + change).rem_euclid(9) as usize
                )?;
            }
            HotkeyAction::SwapMaster => {
                self.man.swap_master();
            }
        };
        self.refresh()?;
        Ok(())
    }

    fn handle_enter(&mut self, event: EnterNotifyEvent) -> Res {
        log::debug!(
            "EVENT ENTER child {} detail {:?} event {}",
            event.child,
            event.detail,
            event.event
        );

        if let Some(w) = self.man.get_window_state(event.child) {
            self.man.tags[self.man.active_tag].focus = Some(w.window);
        };
        if let Some(w) = self.man.get_window_state(event.event) {
            self.man.tags[self.man.active_tag].focus = Some(w.window);
        };
        self.refresh()?;
        Ok(())
    }

    fn handle_config(&self, event: ConfigureRequestEvent) -> Res {
        match self.man.get_window_state(event.window) {
            Some(_) => self.conn.handle_config(event)?,
            None => (),
        };
        Ok(())
    }

    fn handle_client_message(&mut self, event: ClientMessageEvent) -> Res {
        let data = event.data.as_data32();

        if data[1] == 0 {
            return Ok(());
        }

        let event_type = self.conn.get_atom_name(event.type_)?;

        let first_property = self.conn.get_atom_name(data[1])?;

        log::debug!("got client data {data:?}");
        log::debug!(
            "GOT CLIENT EVENT window {} atom {:?} first prop {:?}",
            event.window,
            event_type,
            first_property
        );

        match event_type.as_str() {
            "_NET_WM_STATE" => match first_property.as_str() {
                "_NET_WM_STATE_FULLSCREEN" => {
                    let state = match self.man.get_mut_window_state(event.window) {
                        Some(s) => s,
                        None => return Ok(()),
                    };
                    let window = state.window;
                    match data[0] {
                        0 => {
                            state.group = WindowGroup::Stack;
                            self.conn.remove_atom_prop(window, "_NET_WM_STATE")?;
                            self.refresh()?;
                        }
                        1 => {
                            state.group = WindowGroup::Floating;
                            state.x = 0;
                            state.y = 0;
                            state.width = self.conn.screen.width_in_pixels;
                            state.height = self.conn.screen.height_in_pixels;
                            self.conn.set_fullscreen(state)?;
                            self.refresh()?;
                        }
                        2 => {}
                        _ => {}
                    };
                }
                _ => {}
            },
            _ => {}
        };

        Ok(())
    }

    fn refresh(&mut self) -> Res {
        self.refresh_focus()?;
        self.man.refresh();
        self.config_all()?;
        self.conn.refresh(&self.man)?;
        self.man.print_state();
        Ok(())
    }

    fn refresh_focus(&self) -> Res {
        match self.man.tags[self.man.active_tag].focus {
            Some(w) => {
                let window = match self.man.get_window_state(w) {
                    Some(w) => w,
                    None => return Ok(()),
                };
                self.conn
                    .set_focus_window(self.man.get_active_tag_windows(), window)?;
            }
            None => {
                self.conn.set_focus_to_root()?;
            }
        };
        Ok(())
    }

    fn change_active_tag(&mut self, tag: usize) -> Res {
        if self.man.active_tag == tag {
            log::error!("tried switching to already active tag");
            return Ok(());
        }
        log::debug!("changing tag to {tag}");
        self.unmap_all()?;
        self.man.active_tag = tag;
        self.map_all()?;
        Ok(())
    }

    fn map_all(&mut self) -> Res {
        self.man
            .get_active_tag_windows()
            .iter()
            .try_for_each(|w| self.conn.map(w))
    }

    fn unmap_all(&mut self) -> Res {
        self.man
            .get_active_tag_windows()
            .iter()
            .try_for_each(|w| self.conn.unmap(w))
    }

    fn config_all(&mut self) -> Res {
        self.man
            .get_active_tag_windows()
            .iter()
            .try_for_each(|w| self.conn.config_window_from_state(w))
    }

    fn move_window(&mut self, tag: usize) -> Res {
        if self.man.active_tag == tag {
            log::error!("tried moving window to already active tag");
            return Ok(());
        }
        log::debug!("moving window to tag {tag}");

        let focus_window = self.conn.get_focus()?;

        let state = if let Some(s) = self.man.get_window_state(focus_window) {
            *s
        } else {
            return Ok(());
        };
        self.conn.unmap(&state)?;

        self.man.tags[tag].windows.push(state);
        self.man.tags[self.man.active_tag]
            .windows
            .retain(|w| w.window != focus_window);
        self.man.set_tag_focus_to_master();
        Ok(())
    }
}
