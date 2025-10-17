use crate::actions::*;
use crate::keys::{Hotkey, KeyHandler};

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use x11rb::connection::Connection;
use x11rb::errors::ReplyOrIdError;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::*;
use x11rb::CURRENT_TIME;

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
    fn new(
        window: Window,
        frame_window: Window,
        window_geometry: &GetGeometryReply,
    ) -> WindowState {
        WindowState {
            window,
            frame_window,
            x: window_geometry.x,
            y: window_geometry.y,
            width: window_geometry.width,
            height: window_geometry.height,
            group: WindowGroup::Master,
            tag: 1,
        }
    }
    fn print(&self) {
        println!(
            "window: id {} frame_id {} x {} y {} w {} h {} g {:?}",
            self.window, self.frame_window, self.x, self.y, self.width, self.height, self.group
        );
    }
}

pub struct WindowManagerState<'a, C: Connection> {
    pub connection: &'a C,
    pub screen: &'a Screen,
    pub screen_num: usize,
    pub graphics_context: Gcontext,
    pub windows: HashMap<u16, Vec<WindowState>>,
    pub bar: WindowState,
    pending_exposed_events: HashSet<Window>,
    pub sequences_to_ignore: BinaryHeap<Reverse<u16>>,
    pub mode: ModeStack,
    active_tag: u16,
    pub key_state: KeyHandler<'a, C>,
}

type StateResult<'a, C> = Result<WindowManagerState<'a, C>, ReplyOrIdError>;

impl<'a, C: Connection> WindowManagerState<'a, C> {
    pub fn new(connection: &'a C, screen_num: usize) -> StateResult<'a, C> {
        let screen = &connection.setup().roots[screen_num];
        let id_graphics_context = connection.generate_id()?;
        let id_font = connection.generate_id()?;
        let graphics_context = CreateGCAux::new()
            .graphics_exposures(0)
            .background(screen.white_pixel)
            .foreground(screen.black_pixel)
            .font(id_font);

        //side effect
        set_font(
            connection,
            id_font,
            id_graphics_context,
            screen,
            &graphics_context,
        )?;

        let handler = KeyHandler::new(connection, screen.root)?;

        Ok(WindowManagerState {
            connection,
            screen,
            screen_num,
            graphics_context: id_graphics_context,
            windows: (1..=9).map(|x| (x as u16, Vec::new())).collect(),
            bar: WindowState {
                window: connection.generate_id()?,
                frame_window: connection.generate_id()?,
                x: 0,
                y: 0,
                width: screen.width_in_pixels,
                height: 20,
                group: WindowGroup::None,
                tag: 0,
            },
            pending_exposed_events: HashSet::default(),
            sequences_to_ignore: Default::default(),
            mode: ModeStack {
                ratio_between_master_stack: 0.5,
                spacing: 10,
                border_size: 1,
            },
            active_tag: 1,
            key_state: handler,
        })
    }

    pub fn add_hotkeys(self, hotkeys: Vec<Hotkey>) -> Result<Self, ReplyOrIdError> {
        Ok(hotkeys.into_iter().fold(self, |acc, h| Self {
            key_state: acc.key_state.add_hotkey(h).unwrap(),
            ..acc
        }))
    }

    pub fn scan_for_new_windows(self) -> StateResult<'a, C> {
        println!("scanning windows");

        Ok(self
            .connection
            .query_tree(self.screen.root)?
            .reply()?
            .children
            .iter()
            .filter(|window| {
                let window_attributes = self
                    .connection
                    .get_window_attributes(**window)
                    .unwrap()
                    .reply()
                    .unwrap();
                window_attributes.override_redirect
                    && window_attributes.map_state != MapState::UNMAPPED
            })
            .fold(self, |s, window| s.manage_new_window(*window).unwrap()))
    }

    pub fn clear_exposed_events(self) -> Result<Self, ReplyOrIdError> {
        Ok(Self {
            pending_exposed_events: HashSet::new(),
            ..self
        })
    }

    pub fn get_active_window_group(&self) -> &Vec<WindowState> {
        self.windows
            .iter()
            .find(|x| *x.0 == self.active_tag)
            .unwrap()
            .1
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

    pub fn change_active_tag(self, tag: u16) -> Result<Self, ReplyOrIdError> {
        let old_tag = self.active_tag;
        let new_self = Self {
            active_tag: tag,
            ..self
        };
        new_self.unmap_all(old_tag);
        new_self.connection.set_input_focus(InputFocus::NONE, 1 as u32, CURRENT_TIME)?;
        new_self.map_active_windows()
    }

    fn unmap_all(&self, tag: u16) {
        let v = &self.windows[&tag];
        println!("got vector {v:?}");
        self.windows[&tag]
            .iter()
            .for_each(|w| 
                // crate::actions::unmap_window(&self, w.window).unwrap()
                {
                    println!("sending unmap");
                    self.connection.unmap_window(w.window).unwrap();
                    self.connection.unmap_window(w.frame_window).unwrap();
                }
            );
    }

    fn map_active_windows(self) -> Result<Self, ReplyOrIdError> {
        // Ok(self
            // .get_active_window_group()
            // .clone()
            // .into_iter()
            // .fold(self, |acc, w| {
                // crate::actions::create_and_map_window(acc, &w).unwrap().set_last_master_others_stack().tile_windows().unwrap()
            // }))
            self.get_active_window_group().iter().for_each(|w|{self.connection.map_window(w.frame_window).unwrap(); self.connection.map_window(w.window).unwrap();});
        self.tile_windows()
    }

    pub fn handle_event(self, event: Event) -> Result<Self, ReplyOrIdError> {
        if self.sequences_to_ignore.iter().fold(false, |b, num| {
            b || num.0 == event.wire_sequence_number().unwrap()
        }) {
            return Ok(self);
        }

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

    fn print_state(&self) {
        println!("Manager state: active tag {}",self.active_tag);
        self.windows.iter().for_each(|(i,v)|{println!("tag {i} windows:");v.iter().for_each(|w|w.print());});
    }

    fn add_window(self, window: WindowState) -> Self {
        let active_group = self.get_active_window_group().clone().new_with(window);
        self.replace_vec_in_map(active_group).unwrap()
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

    fn manage_new_window(self, window: Window) -> Result<Self, ReplyOrIdError> {
        println!("managing window {window}");

        let window = WindowState::new(
            window,
            self.connection.generate_id()?,
            &self
                .connection
                .get_geometry(window)
                .unwrap()
                .reply()
                .unwrap(),
        );

        //side effect
        crate::actions::create_and_map_window(self, &window)?
            .add_window(window)
            .set_last_master_others_stack()
            .tile_windows()
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
                            self.screen.width_in_pixels - (self.mode.spacing * 2) as u16
                        } else {
                            ((self.screen.width_in_pixels as f32 * (1.0 - ratio))
                                - ((self.mode.spacing * 2) as f32))
                                as u16
                        },
                        height: self.screen.height_in_pixels
                            - (self.mode.spacing * 2) as u16
                            - self.bar.height,
                        group: WindowGroup::Master,
                        tag: self.active_tag,
                    };

                    //side effect
                    config_window(&self.connection, &new_w).unwrap();
                    new_w
                }
                WindowGroup::Stack => {
                    let new_w = WindowState {
                        window: w.window,
                        frame_window: w.frame_window,
                        x: (self.screen.width_in_pixels as f32 * (1.0 - ratio)) as i16,
                        y: if i == 0 {
                            (i * (self.screen.height_in_pixels as usize / stack_count)
                                + self.mode.spacing as usize) as i16
                                + self.bar.height as i16
                        } else {
                            (i * (self.screen.height_in_pixels as usize / stack_count)) as i16
                        },
                        width: (self.screen.width_in_pixels as f32 * ratio) as u16
                            - (self.mode.spacing) as u16,
                        height: if i == 0 {
                            (self.screen.height_in_pixels as usize / stack_count) as u16
                                - (self.mode.spacing * 2) as u16
                                - self.bar.height
                        } else {
                            (self.screen.height_in_pixels as usize / stack_count) as u16
                                - (self.mode.spacing) as u16
                        },
                        group: WindowGroup::Stack,
                        tag: self.active_tag,
                    };

                    //side effect
                    config_window(&self.connection, &new_w).unwrap();
                    new_w
                }
                _ => *w,
            })
            .collect();

        self.replace_vec_in_map(active_group)
    }
}
