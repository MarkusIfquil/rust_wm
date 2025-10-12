use core::time;
use std::thread;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};
use std::process::{exit, id};

use x11rb::connection::{self, Connection};
use x11rb::errors::{ConnectionError, ReplyError, ReplyOrIdError};
use x11rb::protocol::xproto::*;
use x11rb::protocol::{ErrorKind, Event};
use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME};

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
    Monocle,
}

struct ModeStack {
    ratio_between_master_stack: f32,
    spacing: u16,
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct WindowState {
    window: Window,
    frame_window: Window,
    x: i16,
    y: i16,
    width: u16,
    height: u16,
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

struct WindowManagerState<'a, C: Connection> {
    connection: &'a C,
    screen: &'a Screen,
    screen_num: usize,
    graphics_context: Gcontext,
    windows: Vec<WindowState>,
    pending_exposed_events: HashSet<Window>,
    protocols: Atom,
    delete_window: Atom,
    sequences_to_ignore: BinaryHeap<Reverse<u16>>,
    mode: TilingMode,
}

type StateResult<'a, C> = Result<WindowManagerState<'a, C>, ReplyOrIdError>;

impl<'a, C: Connection> WindowManagerState<'a, C> {
    fn new(connection: &'a C, screen_num: usize) -> StateResult<'a, C> {
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
                spacing: 0,
            }),
        })
    }

    fn print_state(&self) {
        println!("Manager state:");
        println!("windows: \n{:?}\nevents: \n{:?}\nseq: \n{:?}",self.windows,self.pending_exposed_events,self.sequences_to_ignore);
    }

    fn add_window(self, window: WindowState) -> Self {
        Self {
            windows: self.windows.new_with(window),
            ..self
        }
    }

    fn scan_windows(self) -> Result<Self, ReplyOrIdError> {
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
        create_and_map_window( &mut self, &window)?;

        self
            .set_all_windows_stack()
            .add_window(window)
            .set_new_window_geometry()
    }

    fn set_all_windows_stack(self) -> Self {
        Self {
            windows: self
                .windows
                .iter()
                .map(|w| WindowState {
                    group: WindowGroup::Stack,
                    ..*w
                })
                .collect::<Vec<_>>(),
            ..self
        }
    }

    fn set_new_window_geometry(self) -> Result<Self, ReplyOrIdError> {
        let ratio = match &self.mode {
            TilingMode::Stack(mode) => mode.ratio_between_master_stack,
            _ => 1.0,
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

    fn refresh(self) -> Result<Self, ReplyOrIdError> {
        // self.pending_exposed_events.iter().map(|window| {
        // self.pending_exposed_events.remove(&window);
        // if let Some(state) = self.find_window_by_id(window) {
        // println!("there are pending events");
        // println!("window pending {window}");
        // self.connection.clear_area(
        // false,
        // window,
        // 0,
        // 0,
        // self.screen.width_in_pixels,
        // self.screen.height_in_pixels,
        // )?;
        // }
        // });
        Ok(Self {
            pending_exposed_events: {
                let mut p = self.pending_exposed_events;
                p.clear();
                p
            },
            ..self
        })
    }

    fn find_window_by_id(&self, window: Window) -> Option<&WindowState> {
        self.windows
            .iter()
            .find(|x| x.window == window || x.frame_window == window)
    }

    fn handle_event(mut self, event: Event) -> Result<Self, ReplyOrIdError> {
        let mut should_ignore = false;
        if let Some(seqno) = event.wire_sequence_number() {
            // Check sequences_to_ignore and remove entries with old (=smaller) numbers.
            while let Some(&Reverse(to_ignore)) = self.sequences_to_ignore.peek() {
                // Sequence numbers can wrap around, so we cannot simply check for
                // "to_ignore <= seqno". This is equivalent to "to_ignore - seqno <= 0", which is what we
                // check instead. Since sequence numbers are unsigned, we need a trick: We decide
                // that values from [MAX/2, MAX] count as "<= 0" and the rest doesn't.
                if to_ignore.wrapping_sub(seqno) <= u16::MAX / 2 {
                    // If the two sequence numbers are equal, this event should be ignored.
                    should_ignore = to_ignore == seqno;
                    break;
                }
                self.sequences_to_ignore.pop();
            }
        }
        if should_ignore {
            println!("ignoring event {:?}",event);
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
        Ok(state)
    }

    fn handle_unmap_notify(self, event: UnmapNotifyEvent) -> Result<Self, ReplyOrIdError> {
        Ok(Self {
            windows: self
                .windows
                .iter()
                .filter(|w| {
                    if w.window != event.window {
                        //side effect
                        unmap_window(&self, &w).unwrap();

                        false
                    } else {
                        true
                    }
                })
                .map(|x| *x)
                .collect(),
            ..self
        })
    }

    fn handle_configure_request(self, event: ConfigureRequestEvent) -> Result<Self, ReplyOrIdError> {
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

fn set_focus_window<C: Connection>(wm_state: &WindowManagerState<C>, event: EnterNotifyEvent) -> Result<(),ReplyOrIdError> {
    if let Some(state) = wm_state.find_window_by_id(event.event) {
        println!("setting focus to: {:?}",state.window);
        // Set the input focus (ignoring ICCCM's WM_PROTOCOLS / WM_TAKE_FOCUS)
        wm_state.connection
            .set_input_focus(InputFocus::PARENT, state.window, CURRENT_TIME)?;
        // Also raise the window to the top of the stacking order
        wm_state.connection.configure_window(
            state.frame_window,
            &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
        )?;
    }
    Ok(())
}

fn config_event_window<C: Connection>(
    wm_state: &WindowManagerState<C>,
    event: ConfigureRequestEvent,
) -> Result<(), ReplyOrIdError> {
    let aux = ConfigureWindowAux::from_configure_request(&event)
        .sibling(None)
        .stack_mode(None);
    println!("Configure: {aux:?}");
    wm_state.connection.configure_window(event.window, &aux)?;
    Ok(())
}

fn unmap_window<C: Connection>(
    wm_state: &WindowManagerState<C>,
    window: &WindowState,
) -> Result<(), ReplyOrIdError> {
    wm_state
        .connection
        .change_save_set(SetMode::DELETE, window.window)
        .unwrap();
    wm_state
        .connection
        .reparent_window(
            window.window,
            wm_state.connection.setup().roots[wm_state.screen_num].root,
            window.x,
            window.y,
        )
        .unwrap();
    wm_state
        .connection
        .destroy_window(window.frame_window)
        .unwrap();
    Ok(())
}

fn create_and_map_window<C: Connection>(
    wm_state: &mut WindowManagerState<C>,
    window: &WindowState,
) -> Result<(), ReplyOrIdError> {
    wm_state.connection.create_window(
        COPY_DEPTH_FROM_PARENT,
        window.frame_window,
        wm_state.screen.root,
        window.x,
        window.y,
        window.width,
        window.height,
        1,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new()
            .event_mask(
                EventMask::EXPOSURE
                    | EventMask::SUBSTRUCTURE_NOTIFY
                    | EventMask::BUTTON_PRESS
                    | EventMask::BUTTON_RELEASE
                    | EventMask::POINTER_MOTION
                    | EventMask::ENTER_WINDOW,
            )
            .background_pixel(wm_state.screen.white_pixel),
    )?;
    wm_state.connection.grab_server()?;
    wm_state
        .connection
        .change_save_set(SetMode::INSERT, window.window)?;
    let cookie = wm_state
        .connection
        .reparent_window(window.window, window.frame_window, 0, 0)?;
    wm_state.connection.map_window(window.frame_window)?;
    wm_state.connection.map_window(window.window)?;
    wm_state.connection.ungrab_server()?;
    wm_state.sequences_to_ignore
            .push(Reverse(cookie.sequence_number() as u16));
    Ok(())
}

fn config_window<C: Connection>(
    connection: &C,
    window: &WindowState,
) -> Result<(), ReplyOrIdError> {
    connection.configure_window(
        window.window,
        &ConfigureWindowAux {
            x: Some(0),
            y: Some(0),
            width: Some(window.width as u32),
            height: Some(window.height as u32),
            border_width: None,
            sibling: None,
            stack_mode: None,
        },
    )?;
    connection.configure_window(
        window.frame_window,
        &get_config_from_window_properties(window, Some(StackMode::ABOVE)),
    )?;
    Ok(())
}

fn get_config_from_window_properties(
    window: &WindowState,
    mode: Option<StackMode>,
) -> ConfigureWindowAux {
    ConfigureWindowAux {
        x: Some(window.x.into()),
        y: Some(window.y.into()),
        width: Some(window.width.into()),
        height: Some(window.height.into()),
        border_width: None,
        sibling: None,
        stack_mode: mode,
    }
}

fn become_window_manager<C: Connection>(connection: &C, screen: &Screen) -> Result<(), ReplyError> {
    let change = ChangeWindowAttributesAux::default()
        .event_mask(EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY);
    let result = connection
        .change_window_attributes(screen.root, &change)?
        .check();
    if let Err(ReplyError::X11Error(ref error)) = result {
        if error.error_kind == ErrorKind::Access {
            println!("another wm is running");
            exit(1);
        } else {
            println!("became wm");
            result
        }
    } else {
        result
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (connection, screen_num) = x11rb::connect(None)?;

    let mut wm_state = WindowManagerState::new(&connection, screen_num)?;
    become_window_manager(&connection, wm_state.screen)?;

    println!(
        "screen: w{} h{}",
        wm_state.screen.width_in_pixels, wm_state.screen.height_in_pixels
    );

    wm_state = wm_state.scan_windows()?;
    loop {
        wm_state = wm_state.refresh()?;
        connection.flush()?;

        let event = connection.wait_for_event()?;
        let mut event_as_option = Some(event);

        while let Some(event) = event_as_option {
            wm_state = wm_state.handle_event(event)?;
            // thread::sleep(time::Duration::from_millis(1000));
            event_as_option = connection.poll_for_event()?;
        }
    }
}
