use crate::actions::*;

use std::collections::{BinaryHeap, HashSet};
use x11rb::connection::Connection;
use x11rb::errors::ReplyOrIdError;
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use std::cmp::Reverse;

type Window = u32;
trait VecExt<T>
where
    T: Clone + PartialEq,
{
    fn new_with(self, value: T) -> Vec<T>;
    fn new_remove(self, value: T) -> Vec<T>;
}

impl<T> VecExt<T> for Vec<T>
where
    T: Clone + PartialEq,
{
    fn new_with(self, value: T) -> Vec<T> {
        self.iter().cloned().chain(std::iter::once(value)).collect()
    }
    fn new_remove(self, value: T) -> Vec<T> {
        self.iter().cloned().filter(|x| *x != value).collect()
    }
}

#[derive(PartialEq, Clone, Copy, Debug)]
enum WindowGroup {
    Master,
    Stack,
}

enum TilingMode {
    Stack(ModeStack),
}

struct ModeStack {
    ratio_between_master_stack: f32,
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
        }
    }
}

pub struct WindowManagerState<'a, C: Connection> {
    pub connection: &'a C,
    pub screen: &'a Screen,
    pub screen_num: usize,
    graphics_context: Gcontext,
    windows: Vec<WindowState>,
    pending_exposed_events: HashSet<Window>,
    protocols: Atom,
    delete_window: Atom,
    pub sequences_to_ignore: BinaryHeap<Reverse<u16>>,
    mode: TilingMode,
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

        //TODO: Separate side effect into function
        connection.open_font(id_font, b"9x15")?;
        connection.create_gc(id_graphics_context, screen.root, &graphics_context)?;
        connection.close_font(id_font)?;

        Ok(WindowManagerState {
            connection,
            screen,
            screen_num,
            graphics_context: id_graphics_context,
            windows: Vec::default(),
            pending_exposed_events: HashSet::default(),
            protocols: connection
                .intern_atom(false, b"WM_PROTOCOLS")?
                .reply()?
                .atom,
            delete_window: connection
                .intern_atom(false, b"WM_DELETE_WINDOW")?
                .reply()?
                .atom,
            sequences_to_ignore: Default::default(),
            mode: TilingMode::Stack(ModeStack {
                ratio_between_master_stack: 0.5,
            }),
        })
    }

    pub fn scan_windows(self) -> Result<Self, ReplyOrIdError> {
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
            .fold(self, |s, window| s.manage_window(*window).unwrap()))
    }
    
    pub fn refresh(self) -> Result<Self, ReplyOrIdError> {
        Ok(Self {
            pending_exposed_events: {
                let mut p = self.pending_exposed_events;
                p.clear();
                p
            },
            ..self
        })
    }

    pub fn find_window_by_id(&self, window: Window) -> Option<&WindowState> {
        self.windows
            .iter()
            .find(|x| x.window == window || x.frame_window == window)
    }

    pub fn handle_event(self, event: Event) -> Result<Self, ReplyOrIdError> {
        if self.sequences_to_ignore.iter().fold(false, |b, num| {
            b || num.0 == event.wire_sequence_number().unwrap()
        }) {
            println!("ignoring event {:?}", event);
            return Ok(self);
        }

        println!("got event {:?}", event);

        let state = match event {
            Event::UnmapNotify(event) => self.handle_unmap_notify(event),
            Event::ConfigureRequest(event) => self.handle_configure_request(event),
            Event::MapRequest(event) => self.handle_map_request(event),
            Event::Expose(event) => self.handle_expose(event),
            Event::EnterNotify(event) => self.handle_enter(event),
            _ => Ok(self),
        }?;
        state.print_state();
        Ok(state.clear_ignored_sequences())
    }
    
    fn print_state(&self) {
        println!("Manager state:");
        println!(
            "windows: \n{:?}\nevents: \n{:?}\nseq: \n{:?}",
            self.windows, self.pending_exposed_events, self.sequences_to_ignore
        );
    }
    
    fn add_window(self, window: WindowState) -> Self {
        Self {
            windows: self.windows.new_with(window),
            ..self
        }
    }

    fn set_last_master_others_stack(self) -> Self {
        Self {
            windows: self
                .windows
                .iter()
                .enumerate()
                .map(|(i, w)| {
                    if i == self.windows.len() - 1 {
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
                .collect(),
            ..self
        }
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

    fn manage_window(mut self, window: Window) -> Result<Self, ReplyOrIdError> {
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
        create_and_map_window(&mut self, &window)?;

        self.add_window(window)
            .set_last_master_others_stack()
            .set_new_window_geometry()
    }

    fn set_new_window_geometry(self) -> Result<Self, ReplyOrIdError> {
        let ratio = match &self.mode {
            TilingMode::Stack(mode) => mode.ratio_between_master_stack,
        };
        let stack_count = self
            .windows
            .iter()
            .filter(|w| w.group == WindowGroup::Stack)
            .count();

        Ok(Self {
            windows: self
                .windows
                .iter()
                .enumerate()
                .map(|(i, w)| match w.group {
                    WindowGroup::Master => {
                        let new_w = WindowState {
                            window: w.window,
                            frame_window: w.frame_window,
                            x: 0,
                            y: 0,
                            width: if stack_count == 0 {
                                self.screen.width_in_pixels as u16
                            } else {
                                (self.screen.width_in_pixels as f32 * (1.0 - ratio)) as u16
                            },
                            height: self.screen.height_in_pixels,
                            group: WindowGroup::Master,
                        };
                        println!(
                            "master window: w{} h{} x{} y{}",
                            new_w.width, new_w.height, 0, 0
                        );
                        //side effect
                        config_window(&self.connection, &new_w).unwrap();
                        new_w
                    }
                    WindowGroup::Stack => {
                        let new_w = WindowState {
                            window: w.window,
                            frame_window: w.frame_window,
                            x: (self.screen.width_in_pixels as f32 * (1.0 - ratio)) as i16,
                            y: (i * (self.screen.height_in_pixels as usize / stack_count))
                                .try_into()
                                .expect("damn"),
                            width: (self.screen.width_in_pixels as f32 * ratio) as u16,
                            height: (self.screen.height_in_pixels as usize / stack_count) as u16,
                            group: WindowGroup::Stack,
                        };
                        println!(
                            "stack window: w{} h{} x{} y{}",
                            new_w.width, new_w.height, new_w.x, new_w.y
                        );
                        //side effect
                        config_window(&self.connection, &new_w).unwrap();
                        new_w
                    }
                })
                .collect(),
            ..self
        })
    }


    fn handle_unmap_notify(self, event: UnmapNotifyEvent) -> Result<Self, ReplyOrIdError> {
        println!("unmapping window {:?}", event.window);
        let state = Self {
            windows: self
                .windows
                .iter()
                .filter(|w| {
                    if w.window == event.window {
                        //side effect
                        crate::actions::unmap_window(&self, &w).unwrap();

                        false
                    } else {
                        true
                    }
                })
                .map(|x| *x)
                .collect(),
            ..self
        };
        state.set_last_master_others_stack().set_new_window_geometry()
    }

    fn handle_configure_request(
        self,
        event: ConfigureRequestEvent,
    ) -> Result<Self, ReplyOrIdError> {
        //side effect
        config_event_window(&self, event).unwrap();

        Ok(Self { ..self })
    }

    fn handle_map_request(self, event: MapRequestEvent) -> Result<Self, ReplyOrIdError> {
        self.manage_window(event.window)
    }

    fn handle_expose(self, event: ExposeEvent) -> Result<Self, ReplyOrIdError> {
        Ok(Self {
            pending_exposed_events: {
                let mut p = self.pending_exposed_events.clone();
                p.insert(event.window);
                p
            },
            ..self
        })
    }

    fn handle_enter(self, event: EnterNotifyEvent) -> Result<Self, ReplyOrIdError> {
        //side effect
        set_focus_window(&self, event).unwrap();

        Ok(Self { ..self })
    }
}