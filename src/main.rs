use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};
use std::process::{exit, id};

use x11rb::connection::Connection;
use x11rb::errors::{ConnectionError, ReplyError, ReplyOrIdError};
use x11rb::protocol::xproto::*;
use x11rb::protocol::{ErrorKind, Event};
use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME};
#[derive(PartialEq)]
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
    screen_num: usize,
    graphics_context: Gcontext,
    windows: Vec<WindowState>,
    pending_exposed_events: HashSet<Window>,
    protocols: Atom,
    delete_window: Atom,
    sequences_to_ignore: BinaryHeap<Reverse<u16>>,
    mode: TilingMode,
}

impl<'a, C: Connection> WindowManagerState<'a, C> {
    fn new(
        connection: &'a C,
        screen_num: usize,
    ) -> Result<WindowManagerState<'a, C>, ReplyOrIdError> {
        let screen = &connection.setup().roots[screen_num];
        let id_graphics_context = connection.generate_id()?;
        let id_font = connection.generate_id()?;
        connection.open_font(id_font, b"9x15")?;
        let graphics_context = CreateGCAux::new()
            .graphics_exposures(0)
            .background(screen.white_pixel)
            .foreground(screen.black_pixel)
            .font(id_font);
        connection.create_gc(id_graphics_context, screen.root, &graphics_context)?;
        connection.close_font(id_font)?;

        let protocols = connection.intern_atom(false, b"WM_PROTOCOLS")?;
        let delete_window = connection.intern_atom(false, b"WM_DELETE_WINDOW")?;

        Ok(WindowManagerState {
            connection,
            screen_num,
            graphics_context: id_graphics_context,
            windows: Vec::default(),
            pending_exposed_events: HashSet::default(),
            protocols: protocols.reply()?.atom,
            delete_window: delete_window.reply()?.atom,
            sequences_to_ignore: Default::default(),
            mode: TilingMode::Stack(ModeStack {
                ratio_between_master_stack: 0.5,
                spacing: 0,
            }),
        })
    }

    fn scan_windows(&mut self) -> Result<(), ReplyOrIdError> {
        println!("scanning windows");
        let screen = &self.connection.setup().roots[self.screen_num];
        let root_tree_reply = self.connection.query_tree(screen.root)?.reply()?;
        let _ = root_tree_reply.children.iter().map(|window| {
            let window_attributes = self.connection.get_window_attributes(*window)?;
            let window_geometry = self.connection.get_geometry(*window)?;

            if let (Ok(window_attributes), Ok(window_geometry)) =
                (window_attributes.reply(), window_geometry.reply())
            {
                if !window_attributes.override_redirect
                    && window_attributes.map_state != MapState::UNMAPPED
                {
                    self.manage_window(*window, &window_geometry)?;
                }
            } else {
            }
            Ok::<(), ReplyOrIdError>(())
        });
        Ok(())
    }

    fn manage_window(
        &mut self,
        window: Window,
        window_geometry: &GetGeometryReply,
    ) -> Result<(), ReplyOrIdError> {
        println!("managing window {window}");
        let screen = &self.connection.setup().roots[self.screen_num];

        let id_frame_of_window = self.connection.generate_id()?;
        let window_aux = CreateWindowAux::new()
            .event_mask(
                EventMask::EXPOSURE
                    | EventMask::SUBSTRUCTURE_NOTIFY
                    | EventMask::BUTTON_PRESS
                    | EventMask::BUTTON_RELEASE
                    | EventMask::POINTER_MOTION
                    | EventMask::ENTER_WINDOW,
            )
            .background_pixel(screen.white_pixel);

        let window_state = WindowState::new(window, id_frame_of_window, window_geometry);

        self.connection.create_window(
            COPY_DEPTH_FROM_PARENT,
            id_frame_of_window,
            screen.root,
            window_geometry.x,
            window_geometry.y,
            window_geometry.width,
            window_geometry.height,
            1,
            WindowClass::INPUT_OUTPUT,
            0,
            &window_aux,
        )?;

        self.connection.grab_server()?;
        self.connection.change_save_set(SetMode::INSERT, window)?;
        let cookie = self
            .connection
            .reparent_window(window, id_frame_of_window, 0, 0)?;
        self.connection.map_window(id_frame_of_window)?;
        self.connection.map_window(window)?;
        self.connection.ungrab_server()?;

        self.set_all_windows_stack();
        self.windows.push(window_state);
        self.set_new_window_geometry()?;
        Ok(())
    }

    fn set_all_windows_stack(&mut self) {
        self.windows
            .iter_mut()
            .for_each(|w| w.group = WindowGroup::Stack);
    }

    fn set_new_window_geometry(&mut self) -> Result<(), ReplyOrIdError> {
        let ratio = match &self.mode {
            TilingMode::Stack(mode) => mode.ratio_between_master_stack,
            _ => 1.0,
        };

        let screen = &self.connection.setup().roots[self.screen_num];

        let stack_count = self
            .windows
            .iter()
            .filter(|w| w.group == WindowGroup::Stack)
            .count();

        if let Some(master_window) = self
            .windows
            .iter_mut()
            .find(|w| w.group == WindowGroup::Master)
        {
            master_window.x = 0;
            master_window.y = 0;
            master_window.width = if stack_count == 0 {
                screen.width_in_pixels as u16
            } else {
                (screen.width_in_pixels as f32 * (1.0 - ratio)) as u16
            };
            master_window.height = screen.height_in_pixels;

            println!(
                "master window: w{} h{} x{} y{}",
                master_window.width, master_window.height, 0, 0
            );

            self.connection.configure_window(
                master_window.window,
                &ConfigureWindowAux {
                    x: Some(0),
                    y: Some(0),
                    width: Some(master_window.width as u32),
                    height: Some(master_window.height as u32),
                    border_width: None,
                    sibling: None,
                    stack_mode: None,
                },
            )?;
            self.connection.configure_window(
                master_window.frame_window,
                &get_config_from_window_properties(master_window, Some(StackMode::ABOVE)),
            )?;
        }

        self.windows
            .iter_mut()
            .filter(|w| w.group == WindowGroup::Stack)
            .enumerate()
            .for_each(|(i, w)| {
                w.x = (screen.width_in_pixels as f32 * (1.0 - ratio)) as i16;
                w.y = (i * (screen.height_in_pixels as usize / stack_count))
                    .try_into()
                    .expect("damn");
                w.width = (screen.width_in_pixels as f32 * ratio) as u16;
                w.height = (screen.height_in_pixels as usize / stack_count) as u16;

                println!("stack window: w{} h{} x{} y{}", w.width, w.height, w.x, w.y);

                self.connection
                    .configure_window(
                        w.window,
                        &ConfigureWindowAux {
                            x: Some(0),
                            y: Some(0),
                            width: Some(w.width as u32),
                            height: Some(w.height as u32),
                            border_width: None,
                            sibling: None,
                            stack_mode: None,
                        },
                    )
                    .unwrap();
                self.connection
                    .configure_window(
                        w.frame_window,
                        &get_config_from_window_properties(w, Some(StackMode::ABOVE)),
                    )
                    .unwrap();
            });
        self.connection.flush()?;
        Ok(())
    }

    fn refresh(&mut self) -> Result<(), ReplyOrIdError> {
        while let Some(&window) = self.pending_exposed_events.iter().next() {
            self.pending_exposed_events.remove(&window);
            if let Some(state) = self.find_window_by_id(window) {
                println!("there are pending events");
                println!("window pending {window}");

                let screen = &self.connection.setup().roots[self.screen_num];
                self.connection.clear_area(
                    false,
                    window,
                    0,
                    0,
                    screen.width_in_pixels,
                    screen.height_in_pixels,
                )?;
            }
        }
        Ok(())
    }

    fn find_window_by_id(&self, window: Window) -> Option<&WindowState> {
        self.windows
            .iter()
            .find(|x| x.window == window || x.frame_window == window)
    }

    fn find_window_by_id_mut(&mut self, window: Window) -> Option<&mut WindowState> {
        self.windows
            .iter_mut()
            .find(|x| x.window == window || x.frame_window == window)
    }

    fn handle_event(&mut self, event: Event) -> Result<(), ReplyOrIdError> {
        let mut should_ignore = false;
        if let Some(sequence_number) = event.wire_sequence_number() {
            while let Some(&Reverse(number_to_ignore)) = self.sequences_to_ignore.peek() {
                if number_to_ignore.wrapping_sub(sequence_number) <= u16::MAX / 2 {
                    should_ignore = number_to_ignore == sequence_number;
                    break;
                }
                self.sequences_to_ignore.pop();
            }
        }

        if should_ignore {
            return Ok(());
        }
        println!("got event {:?}", event);

        match event {
            Event::UnmapNotify(event) => self.handle_unmap_notify(event),
            Event::ConfigureRequest(event) => self.handle_configure_request(event)?,
            Event::MapRequest(event) => self.handle_map_request(event)?,
            Event::Expose(event) => self.handle_expose(event),
            Event::EnterNotify(event) => self.handle_enter(event)?,
            _ => {}
        }
        Ok(())
    }

    fn handle_unmap_notify(&mut self, event: UnmapNotifyEvent) {
        let root = self.connection.setup().roots[self.screen_num].root;
        self.windows.retain(|window_state| {
            if window_state.window != event.window {
                return true;
            }
            self.connection
                .change_save_set(SetMode::DELETE, window_state.window)
                .unwrap();
            self.connection
                .reparent_window(window_state.window, root, window_state.x, window_state.y)
                .unwrap();
            self.connection
                .destroy_window(window_state.frame_window)
                .unwrap();
            false
        });
    }

    fn handle_configure_request(&mut self, event: ConfigureRequestEvent) -> Result<(), ReplyError> {
        if let Some(state) = self.find_window_by_id_mut(event.window) {
            let _ = state;
        }
        // Allow clients to change everything, except sibling / stack mode
        let aux = ConfigureWindowAux::from_configure_request(&event)
            .sibling(None)
            .stack_mode(None);
        println!("Configure: {aux:?}");
        self.connection.configure_window(event.window, &aux)?;
        Ok(())
    }

    fn handle_map_request(&mut self, event: MapRequestEvent) -> Result<(), ReplyOrIdError> {
        self.manage_window(
            event.window,
            &self.connection.get_geometry(event.window)?.reply()?,
        )
    }

    fn handle_expose(&mut self, event: ExposeEvent) {
        self.pending_exposed_events.insert(event.window);
    }

    fn handle_enter(&mut self, event: EnterNotifyEvent) -> Result<(), ReplyError> {
        if let Some(state) = self.find_window_by_id(event.event) {
            // Set the input focus (ignoring ICCCM's WM_PROTOCOLS / WM_TAKE_FOCUS)
            self.connection
                .set_input_focus(InputFocus::PARENT, state.window, CURRENT_TIME)?;
            // Also raise the window to the top of the stacking order
            self.connection.configure_window(
                state.frame_window,
                &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
            )?;
        }
        Ok(())
    }
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
    let screen = &connection.setup().roots[screen_num];
    println!(
        "screen: w{} h{}",
        screen.width_in_pixels, screen.height_in_pixels
    );

    become_window_manager(&connection, screen)?;

    let mut wm_state = WindowManagerState::new(&connection, screen_num)?;
    wm_state.scan_windows()?;

    loop {
        wm_state.refresh();
        connection.flush()?;

        let event = connection.wait_for_event()?;
        let mut event_as_option = Some(event);
        while let Some(event) = event_as_option {
            wm_state.handle_event(event)?;
            event_as_option = connection.poll_for_event()?;
        }
    }
}
