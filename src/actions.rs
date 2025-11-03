use std::collections::HashMap;
use std::num::ParseIntError;
use std::process::Command;
use std::process::exit;

use x11rb::COPY_DEPTH_FROM_PARENT;
use x11rb::CURRENT_TIME;
use x11rb::connection::Connection;
use x11rb::cursor;
use x11rb::errors::{ReplyError, ReplyOrIdError};
use x11rb::protocol::{ErrorKind, Event, xproto::*};
use x11rb::resource_manager;

use crate::config::Config;
use crate::keys::HotkeyAction;
use crate::keys::KeyHandler;
use crate::state::*;

type Res = Result<(), ReplyOrIdError>;

fn hex_color_to_rgb(hex: &str) -> Result<(u16, u16, u16), ParseIntError> {
    Ok((
        u16::from_str_radix(&hex[1..3], 16)? * 257,
        u16::from_str_radix(&hex[3..5], 16)? * 257,
        u16::from_str_radix(&hex[5..7], 16)? * 257,
    ))
}

pub struct ConnectionHandler<'a, C: Connection> {
    pub connection: &'a C,
    pub screen: &'a Screen,
    pub key_handler: KeyHandler,
    pub id_graphics_context: Gcontext,
    id_inverted_graphics_context: Gcontext,
    pub graphics: (u32, u32, u32),
    pub font_ascent: i16,
    font_width: i16,
    atoms: HashMap<String, u32>,
}

impl<'a, C: Connection> ConnectionHandler<'a, C> {
    pub fn new(connection: &'a C, screen_num: usize) -> Result<Self, ReplyOrIdError> {
        let config = match Config::new() {
            Ok(c) => c,
            Err(_) => Config::default(),
        };
        let screen = &connection.setup().roots[screen_num];
        let id_graphics_context = connection.generate_id()?;
        let id_inverted_graphics_context = connection.generate_id()?;
        let id_font = connection.generate_id()?;

        let wm_protocols = connection
            .intern_atom(false, b"WM_PROTOCOLS")?
            .reply()?
            .atom;
        let wm_delete_window = connection
            .intern_atom(false, b"WM_DELETE_WINDOW")?
            .reply()?
            .atom;

        //set main color
        let (r, g, b) = hex_color_to_rgb(&config.main_color).unwrap_or_default();
        let main_color = connection
            .alloc_color(screen.default_colormap, r, g, b)?
            .reply()?
            .pixel;
        //set secondary color
        let (r, g, b) = hex_color_to_rgb(&config.secondary_color).unwrap_or_default();
        let secondary_color = connection
            .alloc_color(screen.default_colormap, r, g, b)?
            .reply()?
            .pixel;

        let graphics_context = CreateGCAux::new()
            .graphics_exposures(0)
            .background(main_color)
            .foreground(secondary_color)
            .font(id_font);

        let inverted_graphics_context = CreateGCAux::new()
            .graphics_exposures(0)
            .background(secondary_color)
            .foreground(main_color)
            .font(id_font);

        //set default cursor
        let cursor = cursor::Handle::new(
            connection,
            screen_num,
            &resource_manager::new_from_default(connection)?,
        )?
        .reply()?
        .load_cursor(connection, "left_ptr")?;
        connection.change_window_attributes(
            screen.root,
            &ChangeWindowAttributesAux::new().cursor(cursor),
        )?;

        //try setting font to first available font
        config
            .fonts
            .iter()
            .try_for_each(|f| {
                if let Ok(c) = connection.open_font(id_font, f.as_bytes()) {
                    match c.check() {
                        Ok(_) => {
                            println!("setting font to {f}");
                            Err(())
                        }
                        Err(_) => Ok(()),
                    }
                } else {
                    Ok(())
                }
            })
            .unwrap_or(());

        connection.create_gc(id_graphics_context, screen.root, &graphics_context)?;
        connection.create_gc(
            id_inverted_graphics_context,
            screen.root,
            &inverted_graphics_context,
        )?;

        //get font parameters
        let f = connection.query_font(id_font)?.reply()?.max_bounds;
        println!(
            "got parameters a{} d{} w{}",
            f.ascent, f.descent, f.character_width
        );
        connection.close_font(id_font)?;

        let key_handler = KeyHandler::new(connection, &config)?;

        Ok(ConnectionHandler {
            connection,
            screen,
            id_graphics_context,
            id_inverted_graphics_context,
            graphics: (main_color, secondary_color, id_font),
            font_ascent: f.ascent,
            font_width: f.character_width as i16,
            key_handler,
            atoms: HashMap::from([
                ("WM_PROTOCOLS".to_string(), wm_protocols),
                ("WM_DELETE_WINDOW".to_string(), wm_delete_window),
            ]),
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

    pub fn refresh(&self, wm_state: &ManagerState<C>) -> Res {
        self.draw_bar(wm_state, wm_state.tags[wm_state.active_tag].focus)?;
        Ok(())
    }

    pub fn handle_event(&self, wm_state: &ManagerState<C>, event: Event) -> Res {
        match event {
            Event::UnmapNotify(e) => self.handle_unmap(wm_state, e),
            Event::ConfigureRequest(e) => self.handle_config(wm_state, e),
            Event::EnterNotify(e) => self.handle_enter(wm_state, e),
            Event::KeyPress(e) => self.handle_keypress(wm_state, e),
            _ => Ok(()),
        }
    }

    fn handle_unmap(&self, wm_state: &ManagerState<C>, event: UnmapNotifyEvent) -> Res {
        match wm_state.get_window_state(event.window) {
            Some(w) => self.destroy_window(w),
            None => Ok(()),
        }
    }

    fn handle_config(&self, wm_state: &ManagerState<C>, event: ConfigureRequestEvent) -> Res {
        println!(
            "EVENT CONFIG w {} x {} y {} w {} h {}",
            event.window, event.x, event.y, event.width, event.height
        );
        match wm_state.get_window_state(event.window) {
            Some(_) => self.config_from_event(event),
            None => Ok(()),
        }
    }

    fn handle_enter(&self, wm_state: &ManagerState<C>, event: EnterNotifyEvent) -> Res {
        println!("got enter wid {} fid {}", event.child, event.event);
        if let Some(w) = wm_state.get_window_state(event.child) {
            return self.set_focus_window(wm_state, w);
        };
        if let Some(w) = wm_state.get_window_state(event.event) {
            return self.set_focus_window(wm_state, w);
        };
        Ok(())
    }

    fn handle_keypress(&self, wm_state: &ManagerState<C>, event: KeyPressEvent) -> Res {
        println!(
            "handling keypress with code {} and modifier {:?}",
            event.detail, event.state
        );

        let action = match self.key_handler.get_action(event) {
            Some(a) => a,
            None => return Ok(()),
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
                self.kill_focus(wm_state)?;
            }
            _ => {}
        };
        Ok(())
    }

    pub fn create_frame_of_window(&self, window: &WindowState) -> Res {
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
                        | EventMask::ENTER_WINDOW,
                )
                .background_pixel(self.graphics.0)
                .border_pixel(self.graphics.1),
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

    pub fn destroy_window(&self, window: &WindowState) -> Res {
        println!("destroying window: {}", window.window);
        self.connection
            .change_save_set(SetMode::DELETE, window.window)?;
        self.connection
            .reparent_window(window.window, self.screen.root, window.x, window.y)?;
        self.connection.destroy_window(window.frame_window)?;
        Ok(())
    }

    pub fn set_focus_window(&self, wm_state: &ManagerState<C>, window: &WindowState) -> Res {
        println!("setting focus to: {:?}", window.window);
        self.connection
            .set_input_focus(InputFocus::PARENT, window.window, CURRENT_TIME)?;

        wm_state
            .get_active_window_group()
            .iter()
            .try_for_each(|w| {
                self.connection.configure_window(
                    w.frame_window,
                    &ConfigureWindowAux::new().border_width(wm_state.config.border_size as u32),
                )?;
                self.connection.change_window_attributes(
                    w.frame_window,
                    &ChangeWindowAttributesAux::new().border_pixel(self.graphics.0),
                )?;
                Ok::<(), ReplyOrIdError>(())
            })?;

        self.connection.change_window_attributes(
            window.frame_window,
            &ChangeWindowAttributesAux::new().border_pixel(self.graphics.1),
        )?;
        Ok(())
    }

    pub fn get_focus(&self) -> Result<u32, ReplyOrIdError> {
        Ok(self.connection.get_input_focus()?.reply()?.focus)
    }

    pub fn config_window(&self, window: &WindowState) -> Res {
        println!("configuring window {} from state", window.window);
        self.connection
            .configure_window(
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
            )?
            .check()?;
        self.connection
            .configure_window(
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
            )?
            .check()?;
        Ok(())
    }

    pub fn set_focus_to_root(&self) -> Result<(), ReplyOrIdError> {
        self.connection
            .set_input_focus(InputFocus::NONE, 1 as u32, CURRENT_TIME)?;
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
            self.font_ascent as u16 * 3 / 2,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new().background_pixel(self.graphics.0),
        )?;
        Ok(())
    }

    pub fn kill_focus(&self, wm_state: &ManagerState<C>) -> Res {
        let focus = if let Some(f) = wm_state.tags[wm_state.active_tag].focus {
            f
        } else {
            return Ok(());
        };
        println!("killing focus window {focus}");
        self.connection.send_event(
            false,
            focus,
            EventMask::NO_EVENT,
            ClientMessageEvent::new(
                32,
                focus,
                self.atoms["WM_PROTOCOLS"],
                [self.atoms["WM_DELETE_WINDOW"], 0, 0, 0, 0],
            ),
        )?;
        Ok(())
    }

    fn config_from_event(&self, event: ConfigureRequestEvent) -> Res {
        println!("configuring window: {}", event.window);
        let aux = ConfigureWindowAux::from_configure_request(&event);
        self.connection.configure_window(event.window, &aux)?;
        Ok(())
    }

    fn get_window_name(&self, window: Window) -> Result<String, ReplyOrIdError> {
        match String::from_utf8(
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
        ) {
            Ok(s) => Ok(s),
            Err(_) => Ok("window".to_owned()),
        }
    }

    fn clear_window(&self, w: &WindowState) -> Res {
        Ok(self
            .connection
            .clear_area(false, w.window, w.x, w.y, w.width, w.height)?
            .check()?)
    }

    fn create_tag_rectangle(&self, h: u16, x: usize) -> Rectangle {
        Rectangle {
            x: h as i16 * (x as i16 - 1),
            y: 0,
            width: h,
            height: h,
        }
    }

    pub fn draw_bar(&self, wm_state: &ManagerState<C>, active_window: Option<Window>) -> Res {
        let bar_text = match active_window {
            Some(w) => self.get_window_name(w)?,
            None => "".to_owned(),
        };

        self.clear_window(&wm_state.bar)?;

        let h = wm_state.bar.height;

        //draw regular tag rect
        self.connection.poly_fill_rectangle(
            wm_state.bar.window,
            self.id_inverted_graphics_context,
            &(1..=9)
                .filter(|x| *x != wm_state.active_tag + 1)
                .map(|x| self.create_tag_rectangle(h, x))
                .collect::<Vec<_>>(),
        )?;

        self.connection.poly_fill_rectangle(
            wm_state.bar.window,
            self.id_graphics_context,
            &(1..=9)
                .filter(|x| {
                    *x != wm_state.active_tag + 1 && !wm_state.tags[x - 1].windows.is_empty()
                })
                .map(|x| Rectangle {
                    x: h as i16 * (x as i16 - 1) + h as i16 / 9,
                    y: h as i16 / 9,
                    width: h / 7,
                    height: h / 7,
                })
                .collect::<Vec<Rectangle>>(),
        )?;

        //draw active tag rect
        self.connection.poly_fill_rectangle(
            wm_state.bar.window,
            self.id_graphics_context,
            &[self.create_tag_rectangle(h, wm_state.active_tag + 1)],
        )?;

        if !wm_state.tags[wm_state.active_tag].windows.is_empty() {
            self.connection.poly_fill_rectangle(
                wm_state.bar.window,
                self.id_inverted_graphics_context,
                &[Rectangle {
                    x: h as i16 * (wm_state.active_tag as i16) + h as i16 / 9,
                    y: h as i16 / 9,
                    width: h / 7,
                    height: h / 7,
                }],
            )?;
        }

        let text_y = (h as i16 / 2) + self.font_ascent / 5 * 2;
        //draw regular text
        (1..=9).try_for_each(|x| {
            let text = x.to_string();
            if x == wm_state.active_tag + 1 {
                self.connection.image_text8(
                    wm_state.bar.window,
                    self.id_inverted_graphics_context,
                    (h * (x as u16 - 1) + (h / 2 - (self.font_width as u16 / 2))) as i16,
                    text_y,
                    text.as_bytes(),
                )?;
            } else {
                self.connection.image_text8(
                    wm_state.bar.window,
                    self.id_graphics_context,
                    (h * (x as u16 - 1) + (h / 2 - (self.font_width as u16 / 2))) as i16,
                    text_y,
                    text.as_bytes(),
                )?;
            }
            Ok::<(), ReplyOrIdError>(())
        })?;

        //draw window name text
        self.connection.image_text8(
            wm_state.bar.window,
            self.id_graphics_context,
            h as i16 * 10,
            text_y,
            bar_text.as_bytes(),
        )?;

        self.draw_status_bar(&wm_state.bar)?;
        Ok(())
    }

    pub fn draw_status_bar(&self, w: &WindowState) -> Res {
        let status_text = self.get_window_name(self.screen.root)?;
        self.connection
            .clear_area(
                false,
                w.window,
                w.width as i16 - status_text.len() as i16 * self.font_width,
                w.y,
                w.width,
                w.height,
            )?
            .check()?;
        self.connection
            .image_text8(
                w.window,
                self.id_graphics_context,
                w.width as i16 - status_text.len() as i16 * self.font_width,
                (w.height as i16 / 2) + self.font_ascent / 3,
                status_text.as_bytes(),
            )?
            .check()?;
        println!("DRAWING STATUS");
        Ok(())
    }
    pub fn become_window_manager(&self) -> Res {
        let change = ChangeWindowAttributesAux::default().event_mask(
            EventMask::SUBSTRUCTURE_REDIRECT
                | EventMask::SUBSTRUCTURE_NOTIFY
                | EventMask::KEY_PRESS,
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
}
