use std::fmt::Debug;

use crate::actions::*;
use crate::keys::HotkeyAction;

use x11rb::connection::Connection;
use x11rb::errors::ReplyOrIdError;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::*;

type Window = u32;

#[derive(Clone, Copy, PartialEq, Debug)]
enum WindowGroup {
    Master,
    Stack,
    None,
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
    fn new(window: Window, frame_window: Window) -> Result<WindowState, ReplyOrIdError> {
        Ok(WindowState {
            window,
            frame_window,
            x: 0,
            y: 0,
            width: 100,
            height: 100,
            group: WindowGroup::None,
        })
    }
    pub fn print(&self) {
        println!(
            "id {} fid {} x {} y {} w {} h {} g {:?}",
            self.window, self.frame_window, self.x, self.y, self.width, self.height, self.group
        );
    }
}

pub struct Tag {
    tag: usize,
    pub focus: Option<u32>,
    pub windows: Vec<WindowState>,
}
impl Tag {
    fn new(tag: usize) -> Self {
        Tag {
            tag,
            focus: None,
            windows: Vec::new(),
        }
    }
}

struct TilingPreferences {
    gap: u32,
    ratio: f32,
}

pub struct ManagerState {
    pub tags: Vec<Tag>,
    pub active_tag: usize,
    pub bar: WindowState,
    tiling: TilingPreferences,
}

type Res = Result<(), ReplyOrIdError>;

impl ManagerState {
    pub fn new<C: Connection>(handler: &ConnectionHandler<C>) -> Result<Self, ReplyOrIdError> {
        Ok(ManagerState {
            tags: (0..=8).map(|n| Tag::new(n)).collect(),
            bar: WindowState {
                window: handler.connection.generate_id()?,
                frame_window: handler.connection.generate_id()?,
                x: 0,
                y: 0,
                width: handler.screen.width_in_pixels,
                height: handler.font_ascent as u16 * 3 / 2,
                group: WindowGroup::None,
            },
            active_tag: 0,
            tiling: TilingPreferences {
                gap: handler.config.spacing,
                ratio: handler.config.ratio,
            },
        })
    }

    pub fn get_active_tag(&self) -> &Vec<WindowState> {
        &self.tags[self.active_tag].windows
    }

    pub fn get_mut_active_tag(&mut self) -> &mut Vec<WindowState> {
        &mut self.tags[self.active_tag].windows
    }

    pub fn get_window_state(&self, window: Window) -> Option<&WindowState> {
        self.tags[self.active_tag]
            .windows
            .iter()
            .find(|w| w.window == window || w.frame_window == window)
    }

    fn get_index_of_window(&self, window: Window) -> Option<usize> {
        self.tags[self.active_tag]
            .windows
            .iter()
            .position(|w| w.window == window || w.frame_window == window)
    }

    pub fn handle_event<C: Connection>(
        &mut self,
        conn: &ConnectionHandler<C>,
        event: Event,
    ) -> Res {
        match event {
            Event::MapRequest(e) => self.handle_map_request(conn, e)?,
            Event::UnmapNotify(e) => {
                self.handle_unmap_notify(conn, e)?;
            }
            Event::KeyPress(e) => {
                self.handle_keypress(conn, e)?;
            }
            Event::EnterNotify(e) => {
                self.handle_enter(conn, e)?;
            }
            Event::ConfigureRequest(e) => {
                match self.get_window_state(e.window) {
                    Some(_) => conn.handle_config(e)?,
                    None => (),
                };
            }
            _ => (),
        };
        Ok(())
    }

    fn handle_unmap_notify<C: Connection>(
        &mut self,
        conn: &ConnectionHandler<C>,
        event: UnmapNotifyEvent,
    ) -> Res {
        let window = match self.get_window_state(event.window) {
            Some(w) => w,
            None => return Ok(()),
        };
        println!(
            "EVENT UNMAP window {} event {} from config {} response {}",
            event.window, event.event, event.from_configure, event.response_type
        );

        //side effect
        conn.destroy_window(window)?;

        self.get_mut_active_tag()
            .retain(|w| w.window != event.window);
        self.set_tag_focus_to_master();
        self.refresh(conn)
    }

    fn handle_map_request<C: Connection>(
        &mut self,
        conn: &ConnectionHandler<C>,
        event: MapRequestEvent,
    ) -> Res {
        if let Some(_) = self.get_window_state(event.window) {
            return Ok(());
        };

        println!(
            "EVENT MAP window {} parent {} response {}",
            event.window, event.parent, event.response_type
        );

        let window = WindowState::new(event.window, conn.connection.generate_id()?)?;

        conn.create_frame_of_window(&window)?;
        self.add_window(window);
        self.refresh(conn)
    }

    fn handle_keypress<C: Connection>(
        &mut self,
        conn: &ConnectionHandler<C>,
        event: KeyPressEvent,
    ) -> Res {
        let action = match conn.key_handler.get_action(event) {
            Some(a) => a,
            None => return Ok(()),
        };
        println!(
            "EVENT KEYPRESS code {} sym {:?} action {:?}",
            event.detail, event.state, action
        );

        match action {
            HotkeyAction::SwitchTag(n) => {
                self.change_active_tag(conn, n - 1)?;
            }
            HotkeyAction::MoveWindow(n) => {
                self.move_window(conn, n - 1)?;
            }
            HotkeyAction::Spawn(command) => {
                crate::actions::spawn_command(&command);
            }
            HotkeyAction::ExitFocusedWindow => {
                let focus = match self.tags[self.active_tag].focus {
                    Some(f) => f,
                    None => return Ok(()),
                };
                conn.kill_focus(focus)?;
            }
            HotkeyAction::ChangeRatio(change) => {
                self.tiling.ratio = (self.tiling.ratio + change).clamp(0.15, 0.85);
            }
            HotkeyAction::NextFocus(change) => {
                self.switch_focus_next(change);
            }
            HotkeyAction::NextTag(change) => {
                self.change_active_tag(
                    conn,
                    (self.active_tag as i16 + change).rem_euclid(9) as usize,
                )?;
            }
            HotkeyAction::SwapMaster => {
                self.swap_master();
            }
        };
        self.refresh(conn)?;
        Ok(())
    }

    fn swap_master(&mut self) {
        let focus_window = match self.tags[self.active_tag].focus {
            Some(w) => w,
            None => return,
        };
        let len = self.tags[self.active_tag].windows.len();
        let mut master = self.tags[self.active_tag].windows[len - 1].window;
        if master == focus_window && len > 1 {
            master = self.tags[self.active_tag].windows[len - 2].window;
        }
        let index_f = match self.get_index_of_window(focus_window) {
            Some(i) => i,
            None => return,
        };
        let index_m = match self.get_index_of_window(master) {
            Some(i) => i,
            None => return,
        };
        self.tags[self.active_tag].windows.swap(index_f, index_m);
    }

    fn switch_focus_next(&mut self, change: i16) {
        let focus_window = match self.tags[self.active_tag].focus {
            Some(w) => w,
            None => return,
        };
        let focus_index = (match self
            .get_active_tag()
            .iter()
            .position(|w| w.window == focus_window)
        {
            Some(i) => i,
            None => return,
        } as i16
            + change)
            .rem_euclid(self.get_active_tag().len() as i16);
        self.tags[self.active_tag].focus = Some(self.get_active_tag()[focus_index as usize].window);
    }

    fn handle_enter<C: Connection>(
        &mut self,
        conn: &ConnectionHandler<C>,
        event: EnterNotifyEvent,
    ) -> Res {
        println!(
            "EVENT ENTER child {} detail {:?} event {}",
            event.child, event.detail, event.event
        );

        if let Some(w) = self.get_window_state(event.child) {
            self.tags[self.active_tag].focus = Some(w.window);
        };
        if let Some(w) = self.get_window_state(event.event) {
            self.tags[self.active_tag].focus = Some(w.window);
        };
        self.refresh(conn)?;
        Ok(())
    }

    fn change_active_tag<C: Connection>(&mut self, conn: &ConnectionHandler<C>, tag: usize) -> Res {
        if self.active_tag == tag {
            println!("tried switching to already active tag");
            return Ok(());
        }
        println!("changing tag to {tag}");
        self.unmap_all(conn)?;
        self.active_tag = tag;
        self.map_all(conn)?;
        Ok(())
    }

    fn map_all<C: Connection>(&mut self, conn: &ConnectionHandler<C>) -> Res {
        self.get_active_tag().iter().try_for_each(|w| conn.map(w))
    }

    fn unmap_all<C: Connection>(&mut self, conn: &ConnectionHandler<C>) -> Res {
        self.get_active_tag().iter().try_for_each(|w| conn.unmap(w))
    }

    fn config_all<C: Connection>(&mut self, conn: &ConnectionHandler<C>) -> Res {
        self.get_active_tag()
            .iter()
            .try_for_each(|w| conn.config_window_from_state(w))
    }

    fn move_window<C: Connection>(&mut self, conn: &ConnectionHandler<C>, tag: usize) -> Res {
        if self.active_tag == tag {
            println!("tried moving window to already active tag");
            return Ok(());
        }
        println!("moving window to tag {tag}");

        let focus_window = conn.get_focus()?;

        let state = if let Some(s) = self.get_window_state(focus_window) {
            *s
        } else {
            return Ok(());
        };
        conn.unmap(&state)?;

        self.tags[tag].windows.push(state);
        self.tags[self.active_tag]
            .windows
            .retain(|w| w.window != focus_window);
        self.set_tag_focus_to_master();
        Ok(())
    }

    fn add_window(&mut self, window: WindowState) {
        println!("adding window to tag {}", self.active_tag);
        self.tags[self.active_tag].windows.push(window);
        self.tags[self.active_tag].focus = Some(window.window);
    }

    fn set_tag_focus_to_master(&mut self) {
        println!("setting tag focus to master");
        self.tags[self.active_tag].focus = match self.tags[self.active_tag].windows.last() {
            Some(w) => Some(w.window),
            None => None,
        };
    }

    fn refresh<C: Connection>(&mut self, conn: &ConnectionHandler<C>) -> Res {
        self.set_last_master_others_stack()?;
        self.tile_windows(conn)?;
        self.config_all(conn)?;
        self.refresh_focus(conn)?;
        conn.refresh(self)?;
        self.print_state();
        Ok(())
    }

    fn set_last_master_others_stack(&mut self) -> Res {
        self.get_mut_active_tag()
            .iter_mut()
            .for_each(|w| w.group = WindowGroup::Stack);

        if let Some(w) = self.get_mut_active_tag().last_mut() {
            w.group = WindowGroup::Master;
        };
        Ok(())
    }

    fn tile_windows<C: Connection>(&mut self, conn: &ConnectionHandler<C>) -> Res {
        println!("tiling tag {}", self.active_tag);

        let bar_height = self.bar.height;
        let (gap, ratio) = (self.tiling.gap, self.tiling.ratio);
        let (maxw, maxh) = (conn.screen.width_in_pixels, conn.screen.height_in_pixels);

        let stack_count = self.get_active_tag().len().clamp(1, 100) - 1;

        self.get_mut_active_tag()
            .iter_mut()
            .enumerate()
            .try_for_each(|(i, w)| -> Res {
                match w.group {
                    WindowGroup::Master => {
                        w.x = 0 + gap as i16;
                        w.y = 0 + gap as i16 + bar_height as i16;
                        w.width = if stack_count == 0 {
                            maxw - gap as u16 * 2
                        } else {
                            ((maxw as f32 * (1.0 - ratio)) - (gap as f32 * 2.0)) as u16
                        };
                        w.height = maxh - gap as u16 * 2 - bar_height;
                        Ok(())
                    }
                    WindowGroup::Stack => {
                        w.x = (maxw as f32 * (1.0 - ratio)) as i16;
                        w.y = if i == 0 {
                            (i * (maxh as usize / stack_count) + gap as usize) as i16
                                + bar_height as i16
                        } else {
                            (i * (maxh as usize / stack_count)) as i16
                        };
                        w.width = (maxw as f32 * ratio) as u16 - gap as u16;

                        w.height = if i == 0 {
                            (maxh as usize / stack_count) as u16 - gap as u16 * 2 - bar_height
                        } else {
                            (maxh as usize / stack_count) as u16 - gap as u16
                        };
                        Ok(())
                    }
                    _ => Ok(()),
                }
            })?;
        Ok(())
    }

    fn refresh_focus<C: Connection>(&self, conn: &ConnectionHandler<C>) -> Res {
        match self.tags[self.active_tag].focus {
            Some(w) => {
                let window = match self.get_window_state(w) {
                    Some(w) => w,
                    None => return Ok(()),
                };
                conn.set_focus_window(self.get_active_tag(), window)?;
            }
            None => {
                conn.set_focus_to_root()?;
            }
        };
        Ok(())
    }

    fn print_state(&self) {
        println!(
            "Manager state: active tag {} focus {:?}",
            self.active_tag, self.tags[self.active_tag].focus
        );
        self.tags
            .iter()
            .filter(|t| !t.windows.is_empty())
            .for_each(|t| {
                println!("tag {} windows:", t.tag);
                t.windows.iter().for_each(|w| w.print());
            });
    }
}
