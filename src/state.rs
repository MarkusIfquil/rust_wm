use crate::actions::*;
use crate::config::Config;
use crate::keys::{Hotkey, HotkeyAction, KeyHandler};

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::process::Command;
use x11rb::connection::Connection;
use x11rb::errors::ReplyOrIdError;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::*;

type Window = u32;
trait VecExt<T>
where
    T: Clone + PartialEq,
{
    fn new_with(self, value: T) -> Vec<T>;
}

impl<T> VecExt<T> for Vec<T>
where
    T: Clone + PartialEq,
{
    fn new_with(self, value: T) -> Vec<T> {
        self.iter().cloned().chain(std::iter::once(value)).collect()
    }
}

#[derive(PartialEq, Clone, Copy, Debug)]
enum WindowGroup {
    Master,
    Stack,
    None,
}

pub struct ModeStack {
    ratio_between_master_stack: f32,
    spacing: i16,
    pub border_size: i16,
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
    tag: u16,
}

impl WindowState {
    fn new<C: Connection>(
        window: Window,
        frame_window: Window,
        handler: &ConnectionHandler<C>,
    ) -> WindowState {
        let geo = handler
            .connection
            .get_geometry(window)
            .unwrap()
            .reply()
            .unwrap();
        WindowState {
            window,
            frame_window,
            x: geo.x,
            y: geo.y,
            width: geo.width,
            height: geo.height,
            group: WindowGroup::None,
            tag: 1,
        }
    }
    pub fn print(&self) {
        println!(
            "window: id {} x {} y {} w {} h {} g {:?}",
            self.window, self.x, self.y, self.width, self.height, self.group
        );
    }
}

pub struct ManagerState<'a, C: Connection> {
    pub windows: HashMap<u16, Vec<WindowState>>,
    pub active_tag: u16,
    pub mode: ModeStack,
    pub key_handler: KeyHandler<'a, C>,
    pub bar: WindowState,
    pub sequences_to_ignore: BinaryHeap<Reverse<u16>>,
    pending_exposed_events: HashSet<Window>,
    connection_handler: &'a ConnectionHandler<'a, C>,
}

type Res = Result<(), ReplyOrIdError>;

impl<'a, C: Connection> ManagerState<'a, C> {
    pub fn new(handler: &'a ConnectionHandler<C>) -> Result<Self, ReplyOrIdError> {
        let config = Config::new();

        Ok(ManagerState {
            windows: (1..=9).map(|x| (x as u16, Vec::new())).collect(),
            bar: WindowState {
                window: handler.connection.generate_id()?,
                frame_window: handler.connection.generate_id()?,
                x: 0,
                y: 0,
                width: handler.screen.width_in_pixels,
                height: config.bar_height,
                group: WindowGroup::None,
                tag: 0,
            },
            pending_exposed_events: HashSet::default(),
            sequences_to_ignore: Default::default(),
            mode: ModeStack {
                ratio_between_master_stack: config.ratio,
                spacing: config.spacing as i16,
                border_size: config.border_size as i16,
            },
            active_tag: 1,
            key_handler: KeyHandler::new(handler.connection, handler.screen.root)?,
            connection_handler: handler,
        })
    }

    pub fn add_hotkeys(self, hotkeys: Vec<Hotkey>) -> Result<Self, ReplyOrIdError> {
        Ok(hotkeys.into_iter().fold(self, |acc, h| Self {
            key_handler: acc.key_handler.add_hotkey(h).unwrap(),
            ..acc
        }))
    }

    pub fn clear_exposed_events(self) -> Result<Self, ReplyOrIdError> {
        Ok(Self {
            pending_exposed_events: HashSet::new(),
            ..self
        })
    }

    pub fn scan_for_new_windows(self) -> Result<Self, ReplyOrIdError> {
        Ok(self
            .connection_handler
            .get_unmanaged_windows()?
            .iter()
            .fold(self, |s, window| s.manage_new_window(*window).unwrap()))
    }

    pub fn change_active_tag(self, tag: u16) -> Result<Self, ReplyOrIdError> {
        if self.active_tag == tag {
            println!("tried switching to already active tag");
            return Ok(self);
        }
        let old_tag = self.active_tag;
        let new_self = Self {
            active_tag: tag,
            ..self
        };
        new_self.unmap_tag(old_tag)?;
        new_self.connection_handler.set_focus_to_root()?;
        new_self.redraw_tag()
    }

    pub fn get_active_window_group(&self) -> &Vec<WindowState> {
        self.windows
            .iter()
            .find(|x| *x.0 == self.active_tag)
            .unwrap()
            .1
    }

    pub fn find_window_by_id(&self, window: Window) -> Option<&WindowState> {
        for (_, v) in self.windows.iter() {
            if let Some(f) = v
                .iter()
                .find(|w| w.window == window || w.frame_window == window)
            {
                return Some(f);
            }
        }
        None
    }

    pub fn is_valid_window(&self, window: Window) -> bool {
        self.find_window_by_id(window).is_some()
    }

    pub fn handle_event(self, event: Event) -> Result<Self, ReplyOrIdError> {
        let state = match event {
            Event::UnmapNotify(e) => {
                let s = self.handle_unmap_notify(e)?;
                s.print_state();
                s
            }
            Event::MapRequest(e) => {
                let s = self.handle_map_request(e)?;
                s.print_state();
                s
            }
            Event::Expose(e) => self.handle_expose(e)?,
            Event::KeyPress(e) => self.handle_keypress(e)?,
            Event::Error(e) => {
                println!("GOT ERROR: {e:?}");
                self
            }
            _ => self,
        };

        Ok(state.clear_ignored_sequences())
    }

    fn handle_unmap_notify(self, event: UnmapNotifyEvent) -> Result<Self, ReplyOrIdError> {
        println!("state unmap: {}", event.window);
        let active_window_group = self
            .get_active_window_group()
            .iter()
            .filter(|w| w.window != event.window)
            .map(|x| *x)
            .collect();
        self.replace_vec_in_map(active_window_group)?
            .set_last_master_others_stack()
            .tile_windows()
    }

    fn handle_map_request(self, event: MapRequestEvent) -> Result<Self, ReplyOrIdError> {
        println!("state map: {}", event.window);
        self.manage_new_window(event.window)
    }

    fn handle_expose(self, event: ExposeEvent) -> Result<Self, ReplyOrIdError> {
        println!("state expose: {}", event.window);
        Ok(Self {
            pending_exposed_events: {
                let mut p = self.pending_exposed_events.clone();
                p.insert(event.window);
                p
            },
            ..self
        })
    }

    fn handle_keypress(mut self, event: KeyPressEvent) -> Result<Self, ReplyOrIdError> {
        println!(
            "handling keypress with code {} and modifier {:?}",
            event.detail, event.state
        );

        if let Some(hotkey) = self
            .key_handler
            .hotkeys
            .iter()
            .find(|h| event.state == h.mask && event.detail as u32 == h.code.raw())
        {
            match hotkey.action {
                HotkeyAction::SpawnAlacritty => {
                    Command::new("alacritty").spawn().expect("woah");
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
                    println!("switching to tag {n}");
                    self = self.change_active_tag(n).unwrap();
                }
            }
        }
        Ok(self)
    }

    fn replace_vec_in_map(self, v: Vec<WindowState>) -> Result<Self, ReplyOrIdError> {
        let mut hash = self.windows;
        hash.remove(&self.active_tag);
        hash.insert(self.active_tag, v);
        Ok(Self {
            windows: hash,
            ..self
        })
    }

    fn add_window(self, window: WindowState) -> Self {
        let active_group = self.get_active_window_group().clone().new_with(window);
        self.replace_vec_in_map(active_group).unwrap()
    }

    fn manage_new_window(self, window: Window) -> Result<Self, ReplyOrIdError> {
        println!("managing new window {window}");

        let window = WindowState::new(
            window,
            self.connection_handler.connection.generate_id()?,
            self.connection_handler,
        );

        //side effect
        self.connection_handler.create_frame_of_window(&window)?;

        let new_self = self
            .add_window(window)
            .set_last_master_others_stack()
            .tile_windows()?;
        new_self
            .connection_handler
            .set_focus_window(&new_self, window.window)?;

        Ok(new_self)
    }

    fn tile_windows(self) -> Result<Self, ReplyOrIdError> {
        let ratio = self.mode.ratio_between_master_stack;
        let stack_count = self
            .get_active_window_group()
            .iter()
            .filter(|w| w.group == WindowGroup::Stack)
            .count();

        let active_group = self
            .get_active_window_group()
            .iter()
            .enumerate()
            .map(|(i, w)| match w.group {
                WindowGroup::Master => {
                    let new_w = WindowState {
                        window: w.window,
                        frame_window: w.frame_window,
                        x: 0 + self.mode.spacing,
                        y: 0 + self.mode.spacing + self.bar.height as i16,
                        width: if stack_count == 0 {
                            self.connection_handler.screen.width_in_pixels
                                - (self.mode.spacing * 2) as u16
                        } else {
                            ((self.connection_handler.screen.width_in_pixels as f32
                                * (1.0 - ratio))
                                - ((self.mode.spacing * 2) as f32))
                                as u16
                        },
                        height: self.connection_handler.screen.height_in_pixels
                            - (self.mode.spacing * 2) as u16
                            - self.bar.height,
                        group: WindowGroup::Master,
                        tag: self.active_tag,
                    };

                    //side effect
                    self.connection_handler.config_window(&new_w).unwrap();
                    new_w
                }
                WindowGroup::Stack => {
                    let new_w = WindowState {
                        window: w.window,
                        frame_window: w.frame_window,
                        x: (self.connection_handler.screen.width_in_pixels as f32 * (1.0 - ratio))
                            as i16,
                        y: if i == 0 {
                            (i * (self.connection_handler.screen.height_in_pixels as usize
                                / stack_count)
                                + self.mode.spacing as usize) as i16
                                + self.bar.height as i16
                        } else {
                            (i * (self.connection_handler.screen.height_in_pixels as usize
                                / stack_count)) as i16
                        },
                        width: (self.connection_handler.screen.width_in_pixels as f32 * ratio)
                            as u16
                            - (self.mode.spacing) as u16,
                        height: if i == 0 {
                            (self.connection_handler.screen.height_in_pixels as usize / stack_count)
                                as u16
                                - (self.mode.spacing * 2) as u16
                                - self.bar.height
                        } else {
                            (self.connection_handler.screen.height_in_pixels as usize / stack_count)
                                as u16
                                - (self.mode.spacing) as u16
                        },
                        group: WindowGroup::Stack,
                        tag: self.active_tag,
                    };

                    //side effect
                    self.connection_handler.config_window(&new_w).unwrap();
                    new_w
                }
                _ => *w,
            })
            .collect();

        self.replace_vec_in_map(active_group)
    }

    fn set_last_master_others_stack(self) -> Self {
        let active_group = self
            .get_active_window_group()
            .iter()
            .enumerate()
            .map(|(i, w)| {
                if i == self.get_active_window_group().len() - 1 {
                    WindowState {
                        group: WindowGroup::Master,
                        ..*w
                    }
                } else {
                    WindowState {
                        group: WindowGroup::Stack,
                        ..*w
                    }
                }
            })
            .collect();
        self.replace_vec_in_map(active_group).unwrap()
    }

    fn clear_ignored_sequences(self) -> Self {
        Self {
            sequences_to_ignore: {
                let mut s = self.sequences_to_ignore;
                s.clear();
                s
            },
            ..self
        }
    }

    fn redraw_tag(self) -> Result<Self, ReplyOrIdError> {
        self.get_active_window_group()
            .iter()
            .try_for_each(|w| self.connection_handler.map(w))?;
        self.tile_windows()
    }

    fn unmap_tag(&self, tag: u16) -> Res {
        self.windows[&tag]
            .iter()
            .try_for_each(|w| self.connection_handler.unmap(w))?;
        Ok(())
    }

    fn print_state(&self) {
        println!("Manager state: active tag {}", self.active_tag);
        self.windows.iter().for_each(|(i, v)| {
            println!("tag {i} windows:");
            v.iter().for_each(|w| w.print());
        });
    }
}
