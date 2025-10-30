use crate::actions::*;
use crate::config::Config;
use crate::keys::{HotkeyAction, KeyHandler};

use std::collections::HashSet;
use std::process::Command;
use x11rb::connection::Connection;
use x11rb::errors::ReplyOrIdError;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::*;

type Window = u32;

#[derive(PartialEq, Clone, Copy, Debug)]
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
            "window: id {} x {} y {} w {} h {} g {:?}",
            self.window, self.x, self.y, self.width, self.height, self.group
        );
    }
}

pub struct Tag {
    tag: usize,
    windows: Vec<WindowState>,
}
impl Tag {
    fn new(tag: usize) -> Self {
        Tag {
            tag,
            windows: Vec::new(),
        }
    }
}

pub struct ManagerState<'a, C: Connection> {
    pub pending_exposed_events: HashSet<Window>,
    pub tags: Vec<Tag>,
    pub active_tag: usize,
    pub bar: WindowState,
    pub config: Config,
    pub key_handler: KeyHandler<'a, C>,
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
            pending_exposed_events: HashSet::default(),
            active_tag: 0,
            key_handler: KeyHandler::new(handler.connection, handler.screen.root)?
                .get_hotkeys(&config)?,
            connection_handler: handler,
            config,
        })
    }

    pub fn change_active_tag(&mut self, tag: usize) -> Res {
        if self.active_tag == tag {
            println!("tried switching to already active tag");
            return Ok(());
        }
        println!("changing tag to {tag}");
        //unmap old tag
        self.tags[self.active_tag]
            .windows
            .iter()
            .try_for_each(|w| self.connection_handler.unmap(w))?;

        self.active_tag = tag;
        //map new tag
        self.get_active_window_group()
            .iter()
            .try_for_each(|w| self.connection_handler.map(w))?;

        self.connection_handler.draw_bar(&self, None)?;
        if let Some(w) = self.get_active_window_group().last() {
            self.connection_handler.set_focus_window(&self, w.window)?;
        } else {
            self.connection_handler.set_focus_to_root()?;
        }
        self.tile_windows()
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

    pub fn find_window_by_id(&self, window: Window) -> Option<&WindowState> {
        for tag in self.tags.iter() {
            if let Some(f) = tag
                .windows
                .iter()
                .find(|w| w.window == window || w.frame_window == window)
            {
                return Some(f);
            }
        }
        None
    }

    pub fn handle_event(&mut self, event: Event) -> Res {
        match event {
            Event::UnmapNotify(e) => {
                self.handle_unmap_notify(e)?;
                self.print_state();
            }
            Event::MapRequest(e) => {
                self.handle_map_request(e)?;
                self.print_state();
            }
            Event::Expose(e) => self.handle_expose(e),
            Event::KeyPress(e) => self.handle_keypress(e)?,
            Event::Error(e) => {
                println!("GOT ERROR: {e:?}");
            }
            _ => {}
        };
        Ok(())
    }

    fn handle_unmap_notify(&mut self, event: UnmapNotifyEvent) -> Res {
        println!("state unmap: {}", event.window);
        self.get_mut_active_window_group()
            .retain(|w| w.window != event.window);
        self.set_last_master_others_stack()?;
        self.tile_windows()?;
        Ok(())
    }

    fn handle_map_request(&mut self, event: MapRequestEvent) -> Res {
        println!("state map: {}", event.window);
        self.manage_new_window(event.window)
    }

    fn handle_expose(&mut self, event: ExposeEvent) {
        println!("state expose: {}", event.window);
        self.pending_exposed_events.insert(event.window);
    }

    fn handle_keypress(&mut self, event: KeyPressEvent) -> Res {
        println!(
            "handling keypress with code {} and modifier {:?}",
            event.detail, event.state
        );
        let action = if let Some(h) = self
            .key_handler
            .get_registered_hotkey(event.state, event.detail as u32)
        {
            h.action.clone()
        } else {
            return Ok(());
        };

        match action {
            HotkeyAction::Spawn(command) => {
                let parts = command.split(" ").map(|s| s.to_owned()).collect::<Vec<_>>();
                Command::new(parts[0].clone())
                    .args(parts[1..].iter())
                    .spawn()
                    .expect("cant spawn process");
            }
            HotkeyAction::ExitFocusedWindow => {
                self.connection_handler.connection.kill_client(
                    self.connection_handler
                        .connection
                        .get_input_focus()?
                        .reply()?
                        .focus,
                )?;
            }
            HotkeyAction::SwitchTag(n) => {
                println!("switching to tag {}",n-1);
                self.change_active_tag(n as usize - 1)?;
            }
            HotkeyAction::MoveWindow(n) => {
                println!("moving window to {n}");
                self.move_window(n as usize - 1)?;
            }
        };
        Ok(())
    }

    fn move_window(&mut self, tag: usize) -> Res {
        if self.active_tag == tag {
            println!("tried moving window to already active tag");
            return Ok(());
        }
        println!("moving window to tag {tag}");

        let focus_window = self
            .connection_handler
            .connection
            .get_input_focus()?
            .reply()?
            .focus;

        let state = if let Some(s) = self.find_window_by_id(focus_window) {
            *s
        } else {
            return Ok(());
        };
        self.connection_handler.unmap(&state)?;

        if self.get_active_window_group().len() == 1 {
            self.connection_handler.set_focus_to_root()?;
        }
        self.tags[tag].windows.push(state);
        self.tags[self.active_tag]
            .windows
            .retain(|w| w.window != focus_window);

        self.tile_windows()
    }

    fn add_window(&mut self, window: WindowState) {
        println!("adding window to tag {}",self.active_tag);
        self.tags[self.active_tag].windows.push(window);
    }

    fn manage_new_window(&mut self, window: Window) -> Res {
        println!("managing new window {window}");

        let window = WindowState::new(window, self.connection_handler.connection.generate_id()?)?;

        //side effect
        self.connection_handler.create_frame_of_window(&window)?;

        self.add_window(window);
        self.set_last_master_others_stack()?;
        self.tile_windows()?;
        self.connection_handler
            .set_focus_window(&self, window.window)?;
        Ok(())
    }

    fn tile_windows(&mut self) -> Res {
        println!("tiling tag {}",self.active_tag);
        let conf = self.config.clone();
        let (maxw, maxh) = (
            self.connection_handler.screen.width_in_pixels,
            self.connection_handler.screen.height_in_pixels,
        );
        let stack_count = self.get_active_window_group().len().clamp(1, 100) - 1;
        println!("{stack_count}");
        println!("{}",self.get_active_window_group().len());

        self.get_mut_active_window_group()
            .iter_mut()
            .enumerate()
            .try_for_each(|(i, w)| -> Res {
                println!("FUCK");
                w.print();
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
        self.get_active_window_group()
            .iter()
            .try_for_each(|w| self.connection_handler.config_window(w))?;
        println!("AAAAAAAAAA");
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

    fn print_state(&self) {
        println!("Manager state: active tag {}", self.active_tag);
        self.tags.iter().for_each(|t| {
            println!("tag {} windows:", t.tag);
            t.windows.iter().for_each(|w| w.print());
        });
    }
}
