use std::process::exit;

use crate::state::*;
use x11rb::COPY_DEPTH_FROM_PARENT;
use x11rb::CURRENT_TIME;
use x11rb::connection::Connection;
use x11rb::errors::ReplyError;
use x11rb::errors::ReplyOrIdError;
use x11rb::protocol::ErrorKind;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::*;

type Res = Result<(), ReplyOrIdError>;

pub struct ConnectionHandler<'a, C: Connection> {
    pub connection: &'a C,
    pub screen: &'a Screen,
    pub screen_num: usize,
    pub id_graphics_context: Gcontext,
}

impl<'a, C: Connection> ConnectionHandler<'a, C> {
    pub fn new(connection: &'a C, screen_num: usize) -> Result<Self, ReplyOrIdError> {
        let screen = &connection.setup().roots[screen_num];
        let id_graphics_context = connection.generate_id()?;
        let id_font = connection.generate_id()?;
        let graphics_context = CreateGCAux::new()
            .graphics_exposures(0)
            .background(screen.white_pixel)
            .foreground(screen.black_pixel)
            .font(id_font);

        connection.open_font(id_font, b"fixed")?;
        connection.create_gc(id_graphics_context, screen.root, &graphics_context)?;
        connection.close_font(id_font)?;

        Ok(ConnectionHandler {
            connection,
            screen,
            screen_num,
            id_graphics_context,
        })
    }

    pub fn map(&self, window: &WindowState) -> Res {
        self.connection.map_window(window.frame_window)?;
        self.connection.map_window(window.window)?;
        Ok(())
    }

    pub fn unmap(&self, window: &WindowState) -> Res {
        self.connection.unmap_window(window.window)?;
        self.connection.unmap_window(window.frame_window)?;
        Ok(())
    }

    pub fn handle_event(&self, wm_state: &ManagerState<C>, event: Event) -> Res {
        if let Some(w) = match event {
            Event::MapRequest(e) => Some(e.window),
            Event::UnmapNotify(e) => Some(e.window),
            Event::ConfigureRequest(e) => Some(e.window),
            Event::EnterNotify(e) => Some(e.child),
            _ => None,
        } {
            if !wm_state.is_valid_window(w) {
                return Ok(());
            }
        }

        match event {
            Event::MapRequest(event) => {
                self.create_frame_of_window(wm_state.find_window_by_id(event.window).unwrap())
            }
            Event::UnmapNotify(event) => {
                self.unmap_window(wm_state, wm_state.find_window_by_id(event.window).unwrap())
            }
            Event::ConfigureRequest(event) => self.config_event_window(event),
            Event::EnterNotify(event) => self.set_focus_window(&wm_state, event.child),
            _ => Ok(()),
        }
    }

    pub fn create_frame_of_window(&self, window: &WindowState) -> Res {
        println!("CREATING FRAME");
        window.print();
        self.connection.create_window(
            COPY_DEPTH_FROM_PARENT,
            window.frame_window,
            self.screen.root,
            window.x,
            window.y,
            window.width,
            window.height,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new()
                .event_mask(
                    EventMask::KEY_PRESS
                        | EventMask::KEY_RELEASE
                        | EventMask::EXPOSURE
                        | EventMask::SUBSTRUCTURE_NOTIFY
                        | EventMask::BUTTON_PRESS
                        | EventMask::BUTTON_RELEASE
                        | EventMask::POINTER_MOTION
                        | EventMask::ENTER_WINDOW,
                )
                .background_pixel(self.screen.white_pixel)
                .border_pixel(self.screen.white_pixel),
        )?;

        self.connection.grab_server()?;
        self.connection
            .change_save_set(SetMode::INSERT, window.window)?;
        self.connection
            .reparent_window(window.window, window.frame_window, 0, 0)?;
        self.map(window)?;
        self.connection.ungrab_server()?;
        Ok(())
    }

    pub fn unmap_window(&self, wm_state: &ManagerState<C>, window: &WindowState) -> Res {
        if !wm_state.get_active_window_group().contains(window) {
            println!("tried destroying non active tagged window");
            return Ok(());
        }
        println!("destroying window: {}", window.window);
        self.connection
            .change_save_set(SetMode::DELETE, window.window)?;
        self.connection
            .reparent_window(window.window, self.screen.root, window.x, window.y)?;
        self.connection.destroy_window(window.frame_window)?;
        if wm_state.get_active_window_group().len() == 1 {
            self.set_focus_to_root()?;
        }
        Ok(())
    }

    pub fn set_focus_window(&self, wm_state: &ManagerState<C>, window: Window) -> Res {
        if !wm_state
            .get_active_window_group()
            .contains(wm_state.find_window_by_id(window).unwrap())
        {
            println!("tried setting focus of unmapped window");
            return Ok(());
        }
        println!("setting focus to: {:?}", window);
        // Set the input focus (ignoring ICCCM's WM_PROTOCOLS / WM_TAKE_FOCUS)
        self.connection
            .set_input_focus(InputFocus::PARENT, window, CURRENT_TIME)?;

        wm_state
            .get_active_window_group()
            .iter()
            .try_for_each(|w| {
                self.connection.configure_window(
                    w.frame_window,
                    &ConfigureWindowAux::new().border_width(wm_state.mode.border_size as u32),
                )?;
                self.connection.change_window_attributes(
                    w.frame_window,
                    &ChangeWindowAttributesAux::new().border_pixel(self.screen.black_pixel),
                )?;
                Ok::<(), ReplyOrIdError>(())
            })?;

        self.connection.change_window_attributes(
            wm_state.find_window_by_id(window).unwrap().frame_window,
            &ChangeWindowAttributesAux::new().border_pixel(self.screen.white_pixel),
        )?;

        let bar_text = format!("{} {}", wm_state.active_tag, self.get_window_name(window)?);
        self.draw_bar(&wm_state.bar, &bar_text)?;
        Ok(())
    }

    pub fn config_window(&self, window: &WindowState) -> Res {
        println!("CONFIG WINDOW {}", window.window);
        window.print();
        self.connection.configure_window(
            window.frame_window,
            &ConfigureWindowAux {
                x: Some(window.x as i32),
                y: Some(window.y as i32),
                width: Some(window.width as u32),
                height: Some(window.height as u32),
                border_width: None,
                sibling: None,
                stack_mode: None,
            },
        )?;
        self.connection.configure_window(
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
        Ok(())
    }

    pub fn get_unmanaged_windows(&self) -> Result<Vec<u32>, ReplyOrIdError> {
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
            .cloned()
            .collect())
    }

    pub fn set_focus_to_root(&self) -> Result<(), ReplyOrIdError> {
        self.connection
            .set_input_focus(InputFocus::NONE, 1 as u32, CURRENT_TIME)?;
        Ok(())
    }

    pub fn become_window_manager(&self) -> Res {
        let change = ChangeWindowAttributesAux::default().event_mask(
            EventMask::SUBSTRUCTURE_REDIRECT
                | EventMask::SUBSTRUCTURE_NOTIFY
                | EventMask::KEY_PRESS
                | EventMask::KEY_RELEASE,
        );
        let result = self
            .connection
            .change_window_attributes(self.screen.root, &change)?
            .check();
        self.set_focus_to_root()?;
        if let Err(ReplyError::X11Error(ref error)) = result {
            if error.error_kind == ErrorKind::Access {
                println!("another wm is running");
                exit(1);
            } else {
            }
        } else {
            println!("became wm");
        }
        Ok(())
    }

    pub fn create_bar_window(&self, window: Window) -> Res {
        println!("creating window: {}", window);
        self.connection.create_window(
            COPY_DEPTH_FROM_PARENT,
            window,
            self.screen.root,
            0,
            0,
            self.screen.width_in_pixels,
            20,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new().background_pixel(self.screen.white_pixel),
        )?;
        // self.map(window)?;
        Ok(())
    }

    fn config_event_window(&self, event: ConfigureRequestEvent) -> Res {
        let aux = ConfigureWindowAux::from_configure_request(&event);
        println!("configuring window: {}", event.window);
        self.connection.configure_window(event.window, &aux)?;
        Ok(())
    }

    fn get_window_name(&self, window: Window) -> Result<String, ReplyOrIdError> {
        Ok(String::from_utf8(
            self.connection
                .get_property(
                    false,
                    window,
                    AtomEnum::WM_NAME,
                    AtomEnum::STRING,
                    0,
                    u32::MAX,
                )?
                .reply()?
                .value,
        )
        .unwrap())
    }

    fn draw_bar(&self, bar: &WindowState, text: &str) -> Res {
        self.connection
            .clear_area(false, bar.window, bar.x, bar.y, bar.width, bar.height)?;
        self.connection.image_text8(
            bar.window,
            self.id_graphics_context,
            5,
            10,
            text.as_bytes(),
        )?;
        Ok(())
    }
}
