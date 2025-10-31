use std::num::ParseIntError;
use std::process::Command;
use std::process::exit;

use x11rb::COPY_DEPTH_FROM_PARENT;
use x11rb::CURRENT_TIME;
use x11rb::connection::Connection;
use x11rb::errors::{ReplyError, ReplyOrIdError};
use x11rb::protocol::{ErrorKind, Event, xproto::*};

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
    pub key_handler: KeyHandler<'a, C>,
    pub id_graphics_context: Gcontext,
    id_inverted_graphics_context: Gcontext,
    pub graphics: (u32, u32, u32),
    pub font_ascent: i16,
    font_width: i16,
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
        let chars = Char2b {
            byte1: ('a' as u16 >> 8) as u8,
            byte2: ('a' as u16 & 0xFF) as u8,
        };
        let f = connection.query_text_extents(id_font, &[chars])?.reply()?;
        println!(
            "got parameters a{} d{} l{} w{}",
            f.font_ascent, f.font_descent, f.length, f.overall_width
        );
        connection.close_font(id_font)?;

        Ok(ConnectionHandler {
            connection,
            screen,
            id_graphics_context,
            id_inverted_graphics_context,
            graphics: (main_color, secondary_color, id_font),
            font_ascent: f.font_ascent,
            font_width: f.overall_width as i16,
            key_handler: KeyHandler::new(connection, screen.root)?.get_hotkeys(&config)?,
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
        match event {
            Event::MapRequest(event) => match wm_state.find_window_by_id(event.window) {
                Some(w) => self.create_frame_of_window(w),
                None => Ok(()),
            },
            Event::UnmapNotify(event) => match wm_state.find_window_by_id(event.window) {
                Some(w) => self.unmap_window(wm_state, w),
                None => Ok(()),
            },
            Event::ConfigureRequest(event) => self.config_from_event(event),
            Event::EnterNotify(event) => self.set_focus_window(&wm_state, event.child),
            Event::KeyPress(e) => self.handle_keypress(e),
            _ => Ok(()),
        }
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
                        | EventMask::POINTER_MOTION
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
        let w = match wm_state.find_window_by_id(window) {
            Some(w) => w,
            None => return Ok(()),
        };

        if !wm_state.get_active_window_group().contains(w) {
            println!("tried setting focus of unmapped window");
            return Ok(());
        }
        println!("setting focus to: {:?}", window);
        self.connection
            .set_input_focus(InputFocus::PARENT, window, CURRENT_TIME)?;

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
            w.frame_window,
            &ChangeWindowAttributesAux::new().border_pixel(self.graphics.1),
        )?;

        self.draw_bar(wm_state, Some(window))?;
        Ok(())
    }

    fn handle_keypress(&self, event: KeyPressEvent) -> Res {
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
                self.kill_focus()?;
            }
            _ => {}
        };
        Ok(())
    }

    pub fn config_window(&self, window: &WindowState) -> Res {
        println!("configuring window {} from state", window.window);
        window.print();
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
            self.font_ascent as u16 * 3 / 2,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new().background_pixel(self.graphics.0),
        )?;
        Ok(())
    }

    pub fn kill_focus(&self) -> Res {
        let focus = self.connection.get_input_focus()?.reply()?.focus;
        println!("killing focus window {focus}");
        match focus == 1 {
            true => println!("tried killing root"),
            false => {
                self.connection.kill_client(focus)?.check()?
            }
        };
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

        //draw active tag rect
        self.connection.poly_fill_rectangle(
            wm_state.bar.window,
            self.id_graphics_context,
            &[self.create_tag_rectangle(h, wm_state.active_tag + 1)],
        )?;

        let text_y = (h as i16 / 2) + self.font_ascent / 5 * 2;
        //draw regular text
        (1..=9).try_for_each(|x| {
            let text = match wm_state.tags[x - 1].windows.is_empty() {
                true => format!("{}", x),
                false => format!("{}", x),
            };
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

        self.draw_status_bar(&wm_state.bar, self.id_graphics_context)?;
        Ok(())
    }

    pub fn draw_status_bar(&self, w: &WindowState, id: u32) -> Res {
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
                id,
                w.width as i16 - status_text.len() as i16 * self.font_width,
                (w.height as i16 / 2) + self.font_ascent / 3,
                status_text.as_bytes(),
            )?
            .check()?;
        Ok(())
    }
}
