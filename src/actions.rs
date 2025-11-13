use std::collections::HashMap;
use std::process::Command;
use std::process::exit;

use x11rb::protocol::xproto::ConnectionExt;
use x11rb::{
    COPY_DEPTH_FROM_PARENT, CURRENT_TIME,
    connection::Connection,
    cursor,
    errors::{ReplyError, ReplyOrIdError},
    protocol::{ErrorKind, xproto::*},
    resource_manager,
};

use crate::{
    config::{self, Config},
    keys::KeyHandler,
    state::*,
};

pub type Res = Result<(), ReplyOrIdError>;

pub struct ConnectionHandler<'a, C: Connection> {
    pub conn: &'a C,
    pub screen: &'a Screen,
    screen_num: usize,
    pub id_graphics_context: Gcontext,
    id_inverted_graphics_context: Gcontext,
    pub graphics: (u32, u32, u32),
    pub font_ascent: i16,
    font_width: i16,
    pub atoms: HashMap<String, u32>,
    pub config: Config,
    pub bar: WindowState,
}

impl<'a, C: Connection> ConnectionHandler<'a, C> {
    pub fn new(conn: &'a C, screen_num: usize, config: &Config) -> Result<Self, ReplyOrIdError> {
        let screen = &conn.setup().roots[screen_num];
        become_window_manager(conn, screen.root)?;
        log::debug!("screen num {screen_num} root {}", screen.root);

        let id_graphics_context = conn.generate_id()?;
        let id_inverted_graphics_context = conn.generate_id()?;
        let id_font = conn.generate_id()?;

        let atom_strings = vec![
            "UTF8_STRING",
            "WM_PROTOCOLS",
            "WM_DELETE_WINDOW",
            "WM_NAME",
            "_NET_WM_NAME",
            "_NET_SUPPORTED",
            "_NET_CLIENT_LIST",
            "_NET_NUMBER_OF_DESKTOPS",
            "_NET_DESKTOP_GEOMETRY",
            "_NET_DESKTOP_VIEWPORT",
            "_NET_CURRENT_DESKTOP",
            "_NET_DESKTOP_NAMES",
            "_NET_ACTIVE_WINDOW",
            "_NET_WORKAREA",
            "_NET_SUPPORTING_WM_CHECK",
            "_NET_VIRTUAL_ROOTS",
            "_NET_DESKTOP_LAYOUT",
            "_NET_SHOWING_DESKTOP",
            "_NET_WM_NAME",
            "_NET_WM_ALLOWED_ACTIONS",
            "_NET_WM_STATE_MODAL",
            "_NET_WM_STATE",
            "_NET_WM_STATE_STICKY",
            "_NET_WM_STATE_MAXIMIZED_VERT",
            "_NET_WM_STATE_MAXIMIZED_HORZ",
            "_NET_WM_STATE_SHADED",
            "_NET_WM_STATE_SKIP_TASKBAR",
            "_NET_WM_STATE_SKIP_PAGER",
            "_NET_WM_STATE_HIDDEN",
            "_NET_WM_STATE_FULLSCREEN",
            "_NET_WM_STATE_ABOVE",
            "_NET_WM_STATE_BELOW",
            "_NET_WM_STATE_DEMANDS_ATTENTION",
            "_NET_WM_STATE_FOCUSED",
            "_NET_WM_ACTION_MOVE",
            "_NET_WM_ACTION_RESIZE",
            "_NET_WM_ACTION_MINIMIZE",
            "_NET_WM_ACTION_SHADE",
            "_NET_WM_ACTION_STICK",
            "_NET_WM_ACTION_MAXIMIZE_HORZ",
            "_NET_WM_ACTION_MAXIMIZE_VERT",
            "_NET_WM_ACTION_FULLSCREEN",
            "_NET_WM_ACTION_CHANGE_DESKTOP",
            "_NET_WM_ACTION_CLOSE",
            "_NET_WM_ACTION_ABOVE",
            "_NET_WM_ACTION_BELOW",
        ];

        let atom_nums = get_atom_nums(conn, &atom_strings)?;
        let atoms = get_atom_mapping(&atom_strings, &atom_nums);

        let main_color = get_color_id(conn, screen, config.main_color)?;
        let secondary_color = get_color_id(conn, screen, config.secondary_color)?;

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

        set_font(conn, id_font, config)?;

        conn.create_gc(id_graphics_context, screen.root, &graphics_context)?;
        conn.create_gc(
            id_inverted_graphics_context,
            screen.root,
            &inverted_graphics_context,
        )?;

        //get font parameters
        let f = conn.query_font(id_font)?.reply()?.max_bounds;
        log::debug!(
            "got font parameters ascent {} descent {} width {}",
            f.ascent,
            f.descent,
            f.character_width
        );
        conn.close_font(id_font)?;

        let handler = ConnectionHandler {
            conn,
            screen,
            screen_num,
            id_graphics_context,
            id_inverted_graphics_context,
            graphics: (main_color, secondary_color, id_font),
            font_ascent: f.ascent,
            font_width: f.character_width as i16,
            atoms,
            config: config.clone(),
            bar: WindowState {
                window: conn.generate_id()?,
                frame_window: conn.generate_id()?,
                x: 0,
                y: 0,
                width: screen.width_in_pixels,
                height: f.ascent as u16 * 3 / 2,
                group: WindowGroup::Floating,
            },
        };

        handler.change_atom_prop(screen.root, "_NET_SUPPORTED", unsafe {
            atom_nums.as_slice().align_to::<u8>().1
        })?;
        handler.add_heartbeat_window()?;
        handler.grab_keys(&KeyHandler::new(conn, &config)?)?;
        handler.set_cursor()?;
        handler.create_bar_window()?;

        Ok(handler)
    }

    pub fn map(&self, window: &WindowState) -> Res {
        log::debug!("handling map of {}", window.window);
        self.conn.map_window(window.frame_window)?;
        self.conn.map_window(window.window)?;
        Ok(())
    }

    pub fn unmap(&self, window: &WindowState) -> Res {
        log::debug!("handling unmap of {}", window.window);
        self.conn.unmap_window(window.window)?;
        self.conn.unmap_window(window.frame_window)?;
        Ok(())
    }

    pub fn refresh(&self, wm_state: &StateHandler) -> Res {
        log::debug!("refreshing");
        self.draw_bar(wm_state, wm_state.tags[wm_state.active_tag].focus)?;
        Ok(())
    }

    pub fn handle_config(&self, event: ConfigureRequestEvent) -> Res {
        log::debug!(
            "EVENT CONFIG w {} x {} y {} w {} h {}",
            event.window,
            event.x,
            event.y,
            event.width,
            event.height
        );
        let aux = ConfigureWindowAux::from_configure_request(&event);
        self.conn.configure_window(event.window, &aux)?;
        Ok(())
    }

    pub fn create_frame_of_window(&self, window: &WindowState) -> Res {
        log::debug!("creating frame of {}", window.window);
        self.conn.create_window(
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
                        | EventMask::SUBSTRUCTURE_NOTIFY
                        | EventMask::ENTER_WINDOW
                        | EventMask::PROPERTY_CHANGE
                        | EventMask::RESIZE_REDIRECT,
                )
                .background_pixel(self.graphics.0)
                .border_pixel(self.graphics.1),
        )?;

        self.conn.change_window_attributes(
            window.window,
            &ChangeWindowAttributesAux::new().event_mask(
                EventMask::KEY_PRESS
                    | EventMask::SUBSTRUCTURE_NOTIFY
                    | EventMask::ENTER_WINDOW
                    | EventMask::PROPERTY_CHANGE
                    | EventMask::RESIZE_REDIRECT,
            ),
        )?;

        let allowed_actions = [
            "_NET_WM_ACTION_MOVE",
            "_NET_WM_ACTION_RESIZE",
            "_NET_WM_ACTION_MINIMIZE",
            "_NET_WM_ACTION_SHADE",
            "_NET_WM_ACTION_STICK",
            "_NET_WM_ACTION_MAXIMIZE_HORZ",
            "_NET_WM_ACTION_MAXIMIZE_VERT",
            "_NET_WM_ACTION_FULLSCREEN",
            "_NET_WM_ACTION_CHANGE_DESKTOP",
            "_NET_WM_ACTION_CLOSE",
            "_NET_WM_ACTION_ABOVE",
            "_NET_WM_ACTION_BELOW",
        ]
        .map(|a| self.atoms[a]);

        self.change_atom_prop(window.window, "_NET_WM_ALLOWED_ACTIONS", &unsafe {
            allowed_actions.align_to::<u8>().1
        })?;

        self.conn.grab_server()?;
        self.conn.change_save_set(SetMode::INSERT, window.window)?;
        self.conn
            .reparent_window(window.window, window.frame_window, 0, 0)?;
        self.map(window)?;
        self.conn.ungrab_server()?;
        Ok(())
    }

    pub fn destroy_window(&self, window: &WindowState) -> Res {
        log::debug!("destroying window: {}", window.window);
        self.conn.change_save_set(SetMode::DELETE, window.window)?;
        self.conn
            .reparent_window(window.window, self.screen.root, window.x, window.y)?;
        self.conn.destroy_window(window.frame_window)?;

        Ok(())
    }

    pub fn set_focus_window(&self, windows: &Vec<WindowState>, window: &WindowState) -> Res {
        log::debug!("setting focus to: {:?}", window.window);
        self.conn
            .set_input_focus(InputFocus::PARENT, window.window, CURRENT_TIME)?;

        //set borders
        windows.iter().try_for_each(|w| {
            if w.group == WindowGroup::Floating {
                return Ok(());
            }
            self.conn.configure_window(
                w.frame_window,
                &ConfigureWindowAux::new().border_width(self.config.border_size as u32),
            )?;
            self.conn.change_window_attributes(
                w.frame_window,
                &ChangeWindowAttributesAux::new().border_pixel(self.graphics.0),
            )?;
            Ok::<(), ReplyOrIdError>(())
        })?;

        self.conn.change_window_attributes(
            window.frame_window,
            &ChangeWindowAttributesAux::new().border_pixel(self.graphics.1),
        )?;
        Ok(())
    }

    pub fn get_focus(&self) -> Result<u32, ReplyOrIdError> {
        Ok(self.conn.get_input_focus()?.reply()?.focus)
    }

    pub fn config_window_from_state(&self, window: &WindowState) -> Res {
        log::debug!("configuring window {} from state", window.window);
        self.conn
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
        self.conn
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
        log::debug!("setting focus to root");
        self.conn
            .set_input_focus(InputFocus::NONE, 1 as u32, CURRENT_TIME)?;
        Ok(())
    }

    pub fn create_bar_window(&self) -> Res {
        log::debug!("creating bar: {}", self.bar.window);
        self.conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            self.bar.window,
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
        self.create_frame_of_window(&self.bar)?;
        Ok(())
    }

    pub fn kill_focus(&self, focus: u32) -> Res {
        log::debug!("killing focus window {focus}");
        self.conn.send_event(
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

    pub fn draw_bar(&self, wm_state: &StateHandler, active_window: Option<Window>) -> Res {
        let bar_text = match active_window {
            Some(w) => self.get_window_name(w)?,
            None => "".to_owned(),
        };

        log::debug!("drawing bar with text: {bar_text}");

        self.conn.clear_area(
            false,
            self.bar.window,
            self.bar.x,
            self.bar.y,
            self.bar.width / 2,
            self.bar.height,
        )?;

        let h = self.font_ascent as u16 * 3 / 2;

        //draw regular tag rect
        self.conn.poly_fill_rectangle(
            self.bar.window,
            self.id_inverted_graphics_context,
            &(1..=9)
                .filter(|x| *x != wm_state.active_tag + 1)
                .map(|x| self.create_tag_rectangle(h, x))
                .collect::<Vec<_>>(),
        )?;

        //draw indicator that windows are active in tag
        self.conn.poly_fill_rectangle(
            self.bar.window,
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
        self.conn.poly_fill_rectangle(
            self.bar.window,
            self.id_graphics_context,
            &[self.create_tag_rectangle(h, wm_state.active_tag + 1)],
        )?;

        if !wm_state.tags[wm_state.active_tag].windows.is_empty() {
            self.conn.poly_fill_rectangle(
                self.bar.window,
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
                self.conn.image_text8(
                    self.bar.window,
                    self.id_inverted_graphics_context,
                    (h * (x as u16 - 1) + (h / 2 - (self.font_width as u16 / 2))) as i16,
                    text_y,
                    text.as_bytes(),
                )?;
            } else {
                self.conn.image_text8(
                    self.bar.window,
                    self.id_graphics_context,
                    (h * (x as u16 - 1) + (h / 2 - (self.font_width as u16 / 2))) as i16,
                    text_y,
                    text.as_bytes(),
                )?;
            }
            Ok::<(), ReplyOrIdError>(())
        })?;

        //draw window name text
        self.conn.image_text8(
            self.bar.window,
            self.id_graphics_context,
            h as i16 * 9 + h as i16 / 2,
            text_y,
            bar_text.as_bytes(),
        )?;

        Ok(())
    }

    pub fn draw_status_bar(&self) -> Res {
        let status_text = self.get_window_name(self.screen.root)?;
        log::debug!("drawing root windows name on bar with text: {status_text}");
        self.conn
            .clear_area(
                false,
                self.bar.window,
                self.bar.width as i16 - (status_text.len() + 5) as i16 * self.font_width,
                self.bar.y,
                self.bar.width,
                self.bar.height,
            )?
            .check()?;
        self.conn
            .image_text8(
                self.bar.window,
                self.id_graphics_context,
                self.bar.width as i16 - status_text.len() as i16 * self.font_width,
                (self.bar.height as i16 / 2) + self.font_ascent / 3,
                status_text.as_bytes(),
            )?
            .check()?;
        Ok(())
    }

    pub fn set_fullscreen(&self, window: &WindowState) -> Res {
        log::debug!("setting window to fullscreen {}", window.window);
        self.config_window_from_state(window)?;
        self.change_atom_prop(
            window.window,
            "_NET_WM_STATE",
            &self.atoms["_NET_WM_STATE_FULLSCREEN"].to_ne_bytes(),
        )?;
        self.conn.configure_window(
            window.frame_window,
            &ConfigureWindowAux::new().border_width(0),
        )?;
        Ok(())
    }

    pub fn get_atom_name(&self, atom: u32) -> Result<String, ReplyOrIdError> {
        match String::from_utf8(self.conn.get_atom_name(atom)?.reply()?.name) {
            Ok(s) => Ok(s),
            Err(_) => Ok("".to_string()),
        }
    }

    fn get_window_name(&self, window: Window) -> Result<String, ReplyOrIdError> {
        log::debug!("getting window name of {window}");

        let result = String::from_utf8(
            self.conn
                .get_property(
                    false,
                    window,
                    self.atoms["_NET_WM_NAME"],
                    self.atoms["UTF8_STRING"],
                    0,
                    100,
                )?
                .reply()?
                .value,
        )
        .unwrap_or_default();

        return if result.is_empty() {
            let result = String::from_utf8(
                self.conn
                    .get_property(false, window, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 100)?
                    .reply()?
                    .value,
            )
            .unwrap_or_default();
            Ok(result)
        } else {
            Ok(result)
        };
    }

    fn create_tag_rectangle(&self, h: u16, x: usize) -> Rectangle {
        Rectangle {
            x: h as i16 * (x as i16 - 1),
            y: 0,
            width: h,
            height: h,
        }
    }

    fn set_cursor(&self) -> Res {
        let cursor = cursor::Handle::new(
            self.conn,
            self.screen_num,
            &resource_manager::new_from_default(self.conn)?,
        )?
        .reply()?
        .load_cursor(self.conn, "left_ptr")?;
        self.conn.change_window_attributes(
            self.screen.root,
            &ChangeWindowAttributesAux::new().cursor(cursor),
        )?;
        Ok(())
    }

    fn change_atom_prop(&self, window: Window, property: &str, data: &[u8]) -> Res {
        self.conn
            .change_property(
                PropMode::REPLACE,
                window,
                self.atoms[property],
                AtomEnum::ATOM,
                32,
                data.len() as u32 / 4,
                data,
            )?
            .check()?;
        Ok(())
    }

    pub fn remove_atom_prop(&self, window: Window, property: &str) -> Res {
        self.conn
            .change_property(
                PropMode::REPLACE,
                window,
                self.atoms[property],
                AtomEnum::ATOM,
                32,
                1,
                &[0, 0, 0, 0],
            )?
            .check()?;
        Ok(())
    }

    fn add_heartbeat_window(&self) -> Res {
        let support_atom = "_NET_SUPPORTING_WM_CHECK";
        let name_atom = "_NET_WM_NAME";
        let proof_window_id = self.conn.generate_id()?;

        self.conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            proof_window_id,
            self.screen.root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            0,
            &CreateWindowAux::new(),
        )?;

        self.conn.change_property(
            PropMode::REPLACE,
            self.screen.root,
            self.atoms[support_atom],
            AtomEnum::WINDOW,
            32,
            1,
            &proof_window_id.to_ne_bytes(),
        )?;
        self.conn.change_property(
            PropMode::REPLACE,
            proof_window_id,
            self.atoms[support_atom],
            AtomEnum::WINDOW,
            32,
            1,
            &proof_window_id.to_ne_bytes(),
        )?;
        self.conn.change_property(
            PropMode::REPLACE,
            proof_window_id,
            self.atoms[name_atom],
            AtomEnum::STRING,
            8,
            "hematite".len() as u32,
            "hematite".as_bytes(),
        )?;
        Ok(())
    }

    fn grab_keys(&self, handler: &KeyHandler) -> Res {
        handler.hotkeys.iter().try_for_each(|h| {
            self.conn
                .grab_key(
                    false,
                    self.screen.root,
                    h.modifier,
                    h.code,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                )?
                .check()
        })?;
        Ok(())
    }
}

pub fn spawn_command(command: &str) {
    match Command::new("sh").arg("-c").arg(command).spawn() {
        Ok(_) => (),
        Err(e) => log::error!("error when spawning command {e:?}"),
    };
}

fn get_atom_mapping(atom_strings: &[&str], atom_nums: &[u32]) -> HashMap<String, u32> {
    let mut atoms: HashMap<String, u32> = HashMap::new();
    atom_strings
        .iter()
        .map(|s| s.to_string())
        .zip(atom_nums)
        .for_each(|(k, v)| {
            atoms.insert(k, *v);
        });
    atoms
}

fn get_atom_nums<C: Connection>(
    conn: &C,
    atom_strings: &[&str],
) -> Result<Vec<u32>, ReplyOrIdError> {
    Ok(atom_strings
        .iter()
        .flat_map(|s| -> Result<u32, ReplyOrIdError> {
            Ok(conn.intern_atom(false, s.as_bytes())?.reply()?.atom)
        })
        .collect())
}

fn become_window_manager<C: Connection>(conn: &C, root: u32) -> Res {
    let change = ChangeWindowAttributesAux::default().event_mask(
        EventMask::SUBSTRUCTURE_REDIRECT
            | EventMask::SUBSTRUCTURE_NOTIFY
            | EventMask::KEY_PRESS
            | EventMask::PROPERTY_CHANGE,
    );
    let result = conn.change_window_attributes(root, &change)?.check();

    if let Err(ReplyError::X11Error(ref error)) = result {
        if error.error_kind == ErrorKind::Access {
            log::error!("another wm is running");
            exit(1);
        } else {
        }
    } else {
        log::info!("became window manager successfully");
    }
    Ok(())
}

fn get_color_id<C: Connection>(
    conn: &C,
    screen: &Screen,
    color: (u16, u16, u16),
) -> Result<u32, ReplyOrIdError> {
    Ok(conn
        .alloc_color(screen.default_colormap, color.0, color.1, color.2)?
        .reply()?
        .pixel)
}

fn set_font<C: Connection>(conn: &C, id_font: u32, config: &Config) -> Res {
    match conn.open_font(id_font, config.font.as_bytes())?.check() {
        Ok(_) => {
            log::info!("setting font to {}", config.font);
        }
        Err(_) => {
            log::error!("BAD FONT, USING DEFAULT");
            conn.open_font(id_font, config::FONT.as_bytes())?.check()?
        }
    };
    Ok(())
}
