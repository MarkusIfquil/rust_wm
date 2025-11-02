use std::fmt::Debug;

use crate::actions::*;
use crate::config::Config;
use crate::keys::HotkeyAction;

use x11rb::connection::Connection;
use x11rb::errors::ReplyOrIdError;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::*;

type Window = u32;

#[derive(Clone, Copy, PartialEq, Debug)]
enum WindowGroup {
    Master,
    Stack,
    None,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct WindowState {
    pub window: Window,
    pub frame_window: Window,
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
    group: WindowGroup,
}

impl WindowState {
    fn new(window: Window, frame_window: Window) -> Result<WindowState, ReplyOrIdError> {
        Ok(WindowState {
            window,
            frame_window,
            x: 0,
            y: 0,
            width: 100,
            height: 100,
            group: WindowGroup::None,
        })
    }
    pub fn print(&self) {
        println!(
            "id {} fid {} x {} y {} w {} h {} g {:?}",
            self.window, self.frame_window, self.x, self.y, self.width, self.height, self.group
        );
    }
}

pub struct Tag {
    tag: usize,
    pub focus: Option<u32>,
    pub windows: Vec<WindowState>,
}
impl Tag {
    fn new(tag: usize) -> Self {
        Tag {
            tag,
            focus: None,
            windows: Vec::new(),
        }
    }
}

pub struct ManagerState<'a, C: Connection> {
    pub tags: Vec<Tag>,
    pub active_tag: usize,
    pub bar: WindowState,
    pub config: Config,
    connection_handler: &'a ConnectionHandler<'a, C>,
}

type Res = Result<(), ReplyOrIdError>;

impl<'a, C: Connection> ManagerState<'a, C> {
    pub fn new(handler: &'a ConnectionHandler<C>) -> Result<Self, ReplyOrIdError> {
        let config = match Config::new() {
            Ok(c) => c,
            Err(_) => Config::default(),
        };

        Ok(ManagerState {
            tags: (0..=8).map(|n| Tag::new(n)).collect(),
            bar: WindowState {
                window: handler.connection.generate_id()?,
                frame_window: handler.connection.generate_id()?,
                x: 0,
                y: 0,
                width: handler.screen.width_in_pixels,
                height: handler.font_ascent as u16 * 3 / 2,
                group: WindowGroup::None,
            },
            active_tag: 0,
            connection_handler: handler,
            config,
        })
    }

    pub fn get_active_window_group(&self) -> &Vec<WindowState> {
        &self
            .tags
            .iter()
            .find(|x| x.tag == self.active_tag)
            .expect("active window group not found")
            .windows
    }

    pub fn get_mut_active_window_group(&mut self) -> &mut Vec<WindowState> {
        &mut self
            .tags
            .iter_mut()
            .find(|x| x.tag == self.active_tag)
            .expect("active window group not found")
            .windows
    }

    pub fn get_window_state(&self, window: Window) -> Option<&WindowState> {
        self.tags[self.active_tag]
            .windows
            .iter()
            .find(|w| w.window == window || w.frame_window == window)
    }

    pub fn handle_event(&mut self, event: Event) -> Res {
        match event {
            Event::UnmapNotify(e) => self.handle_unmap_notify(e)?,
            Event::MapRequest(e) => self.handle_map_request(e)?,
            Event::KeyPress(e) => self.handle_keypress(e)?,
            Event::EnterNotify(e) => self.handle_enter(e),
            _ => {}
        };
        Ok(())
    }

    fn handle_unmap_notify(&mut self, event: UnmapNotifyEvent) -> Res {
        println!(
            "EVENT UNMAP window {} event {} from config {} response {}",
            event.window, event.event, event.from_configure, event.response_type
        );

        self.get_mut_active_window_group()
            .retain(|w| w.window != event.window);
        self.set_tag_focus_to_master();
        self.refresh()
    }

    fn handle_map_request(&mut self, event: MapRequestEvent) -> Res {
        println!(
            "EVENT MAP window {} parent {} response {}",
            event.window, event.parent, event.response_type
        );

        match self.get_window_state(event.window) {
            None => {
                println!("state map: {}", event.window);
                self.manage_new_window(event.window)?;
                self.refresh()
            }
            Some(_) => Ok(()),
        }
    }

    fn handle_keypress(&mut self, event: KeyPressEvent) -> Res {
        println!("EVENT KEYPRESS code {} sym {:?}", event.detail, event.state);

        let action = match self.connection_handler.key_handler.get_action(event) {
            Some(a) => a,
            None => return Ok(()),
        };

        match action {
            HotkeyAction::SwitchTag(n) => {
                self.change_active_tag(n as usize - 1)?;
                self.refresh()?;
            }
            HotkeyAction::MoveWindow(n) => {
                self.move_window(n as usize - 1)?;
                self.refresh()?;
            }
            _ => {}
        };
        Ok(())
    }

    fn handle_enter(&mut self, event: EnterNotifyEvent) {
        println!(
            "EVENT ENTER child {} detail {:?} event {}",
            event.child, event.detail, event.event
        );
        self.tags[self.active_tag].focus = match self.get_window_state(event.child) {
            Some(w) => Some(w.window),
            None if self.tags[self.active_tag].windows.is_empty() => None,
            None => self.tags[self.active_tag].focus,
        };
    }

    fn manage_new_window(&mut self, window: Window) -> Res {
        println!("managing new window {window}");
        let window = WindowState::new(window, self.connection_handler.connection.generate_id()?)?;
        self.connection_handler.create_frame_of_window(&window)?;

        self.add_window(window);
        Ok(())
    }

    fn change_active_tag(&mut self, tag: usize) -> Res {
        if self.active_tag == tag {
            println!("tried switching to already active tag");
            return Ok(());
        }
        println!("changing tag to {tag}");
        self.unmap_all()?;
        self.active_tag = tag;
        self.map_all()?;
        Ok(())
    }

    fn map_all(&mut self) -> Res {
        self.get_active_window_group()
            .iter()
            .try_for_each(|w| self.connection_handler.map(w))
    }

    fn unmap_all(&mut self) -> Res {
        self.get_active_window_group()
            .iter()
            .try_for_each(|w| self.connection_handler.unmap(w))
    }

    fn config_all(&mut self) -> Res {
        self.get_active_window_group()
            .iter()
            .try_for_each(|w| self.connection_handler.config_window(w))
    }

    fn move_window(&mut self, tag: usize) -> Res {
        if self.active_tag == tag {
            println!("tried moving window to already active tag");
            return Ok(());
        }
        println!("moving window to tag {tag}");

        let focus_window = self.connection_handler.get_focus()?;

        let state = if let Some(s) = self.get_window_state(focus_window) {
            *s
        } else {
            return Ok(());
        };
        self.connection_handler.unmap(&state)?;

        self.tags[tag].windows.push(state);
        self.tags[self.active_tag]
            .windows
            .retain(|w| w.window != focus_window);
        self.set_tag_focus_to_master();
        Ok(())
    }

    fn add_window(&mut self, window: WindowState) {
        println!("adding window to tag {}", self.active_tag);
        self.tags[self.active_tag].windows.push(window);
        self.tags[self.active_tag].focus = Some(window.window);
    }

    fn set_tag_focus_to_master(&mut self) {
        println!("setting tag focus to master");
        self.tags[self.active_tag].focus = match self.tags[self.active_tag].windows.last() {
            Some(w) => Some(w.window),
            None => None,
        };
    }

    fn refresh(&mut self) -> Res {
        self.set_last_master_others_stack()?;
        self.tile_windows()?;
        self.config_all()?;
        self.refresh_focus()?;
        self.connection_handler.refresh(self)?;
        self.print_state();
        Ok(())
    }

    fn set_last_master_others_stack(&mut self) -> Res {
        self.get_mut_active_window_group()
            .iter_mut()
            .for_each(|w| w.group = WindowGroup::Stack);

        if let Some(w) = self.get_mut_active_window_group().last_mut() {
            w.group = WindowGroup::Master;
        };
        Ok(())
    }

    fn tile_windows(&mut self) -> Res {
        println!("tiling tag {}", self.active_tag);
        let conf = self.config.clone();
        let (maxw, maxh) = (
            self.connection_handler.screen.width_in_pixels,
            self.connection_handler.screen.height_in_pixels,
        );
        let stack_count = self.get_active_window_group().len().clamp(1, 100) - 1;

        self.get_mut_active_window_group()
            .iter_mut()
            .enumerate()
            .try_for_each(|(i, w)| -> Res {
                match w.group {
                    WindowGroup::Master => {
                        w.x = 0 + conf.spacing as i16;
                        w.y = 0 + conf.spacing as i16 + conf.bar_height as i16;
                        w.width = if stack_count == 0 {
                            maxw - (conf.spacing * 2) as u16
                        } else {
                            ((maxw as f32 * (1.0 - conf.ratio)) - ((conf.spacing * 2) as f32))
                                as u16
                        };
                        w.height = maxh - (conf.spacing * 2) as u16 - conf.bar_height;
                        Ok(())
                    }
                    WindowGroup::Stack => {
                        w.x = (maxw as f32 * (1.0 - conf.ratio)) as i16;
                        w.y = if i == 0 {
                            (i * (maxh as usize / stack_count) + conf.spacing as usize) as i16
                                + conf.bar_height as i16
                        } else {
                            (i * (maxh as usize / stack_count)) as i16
                        };
                        w.width = (maxw as f32 * conf.ratio) as u16 - (conf.spacing) as u16;

                        w.height = if i == 0 {
                            (maxh as usize / stack_count) as u16
                                - (conf.spacing * 2) as u16
                                - conf.bar_height
                        } else {
                            (maxh as usize / stack_count) as u16 - (conf.spacing) as u16
                        };
                        Ok(())
                    }
                    _ => Ok(()),
                }
            })?;
        Ok(())
    }

    fn refresh_focus(&self) -> Res {
        match self.tags[self.active_tag].focus {
            Some(w) => {
                self.connection_handler
                    .set_focus_window(self, self.get_window_state(w).unwrap())?;
            }
            None => {
                self.connection_handler.set_focus_to_root()?;
            }
        };
        Ok(())
    }

    fn print_state(&self) {
        println!(
            "Manager state: active tag {} focus {:?}",
            self.active_tag, self.tags[self.active_tag].focus
        );
        self.tags
            .iter()
            .filter(|t| !t.windows.is_empty())
            .for_each(|t| {
                println!("tag {} windows:", t.tag);
                t.windows.iter().for_each(|w| w.print());
            });
    }
}
