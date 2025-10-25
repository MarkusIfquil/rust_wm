use std::error::Error;
use std::process::{Command, exit};

use crate::config::Config;
use crate::state::*;
use x11rb::COPY_DEPTH_FROM_PARENT;
use x11rb::CURRENT_TIME;
use x11rb::connection::Connection;
use x11rb::errors::ReplyError;
use x11rb::errors::ReplyOrIdError;
use x11rb::protocol::ErrorKind;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::*;
use x11rb::x11_utils::TryParse;

type Res = Result<(), ReplyOrIdError>;

fn hex_color_to_rgb(hex: String) -> (u16, u16, u16) {
    (
        u16::from_str_radix(&hex[1..3], 16).unwrap() * 257,
        u16::from_str_radix(&hex[3..5], 16).unwrap() * 257,
        u16::from_str_radix(&hex[5..7], 16).unwrap() * 257,
    )
}

pub struct ConnectionHandler<'a, C: Connection> {
    pub connection: &'a C,
    pub screen: &'a Screen,
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

        let (r, g, b) = hex_color_to_rgb(config.main_color);
        let main_color = connection
            .alloc_color(screen.default_colormap, r, g, b)?
            .reply()?
            .pixel;
        let (r, g, b) = hex_color_to_rgb(config.secondary_color);
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

        // println!("got fonts");
        // connection
        // .list_fonts(10000, b"*")?
        // .reply()?
        // .names
        // .iter()
        // .for_each(|n| println!("{:?}", String::from_utf8(n.name.clone()).unwrap()));

        config
            .fonts
            .iter()
            .try_for_each(
                |f| match connection.open_font(id_font, f.as_bytes()).unwrap().check() {
                    Ok(_) => {
                        println!("setting font to {f}");
                        Err(0)
                    }
                    Err(_) => Ok(()),
                },
            )
            .unwrap_or(());

        connection.create_gc(id_graphics_context, screen.root, &graphics_context)?;
        connection.create_gc(
            id_inverted_graphics_context,
            screen.root,
            &inverted_graphics_context,
        )?;
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
                    &ChangeWindowAttributesAux::new().border_pixel(self.graphics.0),
                )?;
                Ok::<(), ReplyOrIdError>(())
            })?;

        self.connection.change_window_attributes(
            wm_state.find_window_by_id(window).unwrap().frame_window,
            &ChangeWindowAttributesAux::new().border_pixel(self.graphics.1),
        )?;

        self.draw_bar(wm_state, Some(window))?;
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

    fn clear_window(&self, w: &WindowState) -> Res {
        Ok(self
            .connection
            .clear_area(false, w.window, w.x, w.y, w.width, w.height)?
            .check()?)
    }

    fn create_tag_rectangle(&self, h: u16, x: u16) -> Rectangle {
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
                .filter(|x| *x != wm_state.active_tag)
                .map(|x| self.create_tag_rectangle(h, x))
                .collect::<Vec<_>>(),
        )?;

        //draw active tag rect
        self.connection.poly_fill_rectangle(
            wm_state.bar.window,
            self.id_graphics_context,
            &[self.create_tag_rectangle(h, wm_state.active_tag)],
        )?;

        //draw regular text
        (1..=9).try_for_each(|x| {
            if x == wm_state.active_tag {
                self.connection.image_text8(
                    wm_state.bar.window,
                    self.id_inverted_graphics_context,
                    (h * (x - 1) + (h / 2 - (self.font_width as u16 / 2))) as i16,
                    (h as i16 / 2) + self.font_ascent / 5 * 2,
                    x.to_string().as_bytes(),
                )?;
            } else {
                self.connection.image_text8(
                    wm_state.bar.window,
                    self.id_graphics_context,
                    (h * (x - 1) + (h / 2 - (self.font_width as u16 / 2))) as i16,
                    (h as i16 / 2) + self.font_ascent / 5 * 2,
                    x.to_string().as_bytes(),
                )?;
            }
            Ok::<(), ReplyOrIdError>(())
        })?;

        //draw active tag text
        self.connection.image_text8(
            wm_state.bar.window,
            self.id_graphics_context,
            wm_state.bar.height as i16 * self.font_width,
            self.font_ascent,
            bar_text.as_bytes(),
        )?;

        self.draw_time_on_bar(&wm_state.bar, self.id_graphics_context)?;
        Ok(())
    }

    pub fn draw_time_on_bar(&self, w: &WindowState, id: u32) -> Res {
        let time = chrono::Local::now()
            .format("%Y, %b %d. %a, %H:%M:%S")
            .to_string();
        let audio =
            send_command("pactl get-sink-volume 0 | awk '{print $5}'").unwrap_or(String::from(""));
        let battery =
            send_command("cat /sys/class/power_supply/BAT0/capacity").unwrap_or(String::from(""));
        let light = send_command("light").unwrap_or(String::from(""));
        let light = (light.trim().parse::<f32>().unwrap().round() as i16).to_string();
        let final_text = format!(
            "L {}% | A {} | B {}% | T {}",
            light.trim(),
            audio.trim(),
            battery.trim(),
            time
        );
        self.connection
            .clear_area(
                false,
                w.window,
                w.width as i16 - final_text.len() as i16 * self.font_width,
                w.y,
                w.width,
                w.height,
            )?
            .check()?;
        self.connection
            .image_text8(
                w.window,
                id,
                w.width as i16 - final_text.len() as i16 * self.font_width,
                (w.height as i16 / 2) + self.font_ascent / 3,
                final_text.as_bytes(),
            )?
            .check()?;
        Ok(())
    }
}

fn send_command(arg: &str) -> Result<String, Box<dyn Error>> {
    Ok(String::from_utf8(
        Command::new("sh")
            .arg("-c")
            .arg(arg)
            .output()
            .expect("process failed")
            .stdout,
    )?)
}
