use std::cmp::Reverse;
use std::process::Command;
use std::process::exit;

use crate::keys::HotkeyAction;
use crate::state::*;
use x11rb::connection::Connection;
use x11rb::errors::ReplyError;
use x11rb::errors::ReplyOrIdError;
use x11rb::protocol::ErrorKind;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::*;
use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME};

type Res<'a, C> = Result<WindowManagerState<'a, C>, ReplyOrIdError>;

pub fn handle_event<C: Connection>(wm_state: WindowManagerState<C>, event: Event) -> Res<C> {
    match event {
        Event::MapRequest(event) => handle_map(wm_state, event),
        Event::UnmapNotify(event) => {
            handle_unmap_notify(&wm_state, event)?;
            Ok(wm_state)
        }
        Event::ConfigureRequest(event) => {
            config_event_window(&wm_state, event)?;
            Ok(wm_state)
        }
        Event::EnterNotify(event) => {
            set_focus_window(&wm_state, event)?;
            Ok(wm_state)
        }
        Event::KeyPress(event) => handle_keypress(wm_state, event),
        _ => Ok(wm_state),
    }
}

fn handle_map<C: Connection>(wm_state: WindowManagerState<C>, event: MapRequestEvent) -> Res<C> {
    Ok(
        if let Some(window) = wm_state.find_window_by_id(event.window) {
            let window = *window;
            create_and_map_window(wm_state, &window)?
        } else {
            wm_state
        },
    )
}

pub fn create_and_map_window<'a, C: Connection>(
    mut wm_state: WindowManagerState<'a, C>,
    window: &WindowState,
) -> Res<'a, C> {
    println!("creating window: {}", window.window);
    wm_state.connection.create_window(
        COPY_DEPTH_FROM_PARENT,
        window.frame_window,
        wm_state.screen.root,
        window.x,
        window.y,
        window.width,
        window.height,
        0,
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
            .background_pixel(wm_state.screen.white_pixel)
            .border_pixel(wm_state.screen.white_pixel),
    )?;
    wm_state.connection.change_window_attributes(
        window.window,
        &ChangeWindowAttributesAux::new().event_mask(EventMask::KEY_PRESS | EventMask::KEY_RELEASE),
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
    wm_state
        .sequences_to_ignore
        .push(Reverse(cookie.sequence_number() as u16));
    Ok(wm_state)
}

fn handle_unmap_notify<C: Connection>(
    wm_state: &WindowManagerState<C>,
    event: UnmapNotifyEvent,
) -> Result<(), ReplyOrIdError> {
    unmap_window(wm_state, event.window)?;
    Ok(())
}

pub fn unmap_window<C: Connection>(
    wm_state: &WindowManagerState<C>,
    window: Window,
) -> Result<(), ReplyOrIdError> {
    if let Some(window) = wm_state.find_window_by_id(window) {
        if !wm_state.get_active_window_group().contains(window) {
            println!("tried unmapping non active window");
            return Ok(());
        }
        println!("unmapping window: {}", window.window);
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

        if wm_state.get_active_window_group().len() == 1 {
            wm_state
                .connection
                .set_input_focus(InputFocus::NONE, 1 as u32, CURRENT_TIME)?;
        }
    }
    Ok(())
}

fn set_focus_window<C: Connection>(
    wm_state: &WindowManagerState<C>,
    event: EnterNotifyEvent,
) -> Result<(), ReplyOrIdError> {
    if let Some(state) = wm_state.find_window_by_id(event.event) {
        if !wm_state.get_active_window_group().contains(state) {
            println!("tried setting focus of unmapped window");
            return Ok(());
        }
        println!("setting focus to: {:?}", state.window);
        // Set the input focus (ignoring ICCCM's WM_PROTOCOLS / WM_TAKE_FOCUS)
        wm_state
            .connection
            .set_input_focus(InputFocus::PARENT, state.window, CURRENT_TIME)?;

        wm_state.get_active_window_group().iter().for_each(|w| {
            wm_state
                .connection
                .configure_window(
                    w.frame_window,
                    &ConfigureWindowAux::new().border_width(wm_state.mode.border_size as u32),
                )
                .unwrap();
            wm_state
                .connection
                .change_window_attributes(
                    w.frame_window,
                    &ChangeWindowAttributesAux::new().border_pixel(wm_state.screen.black_pixel),
                )
                .unwrap();
        });
        wm_state.connection.configure_window(
            state.frame_window,
            &ConfigureWindowAux::new()
                .stack_mode(StackMode::ABOVE)
                .border_width(wm_state.mode.border_size as u32),
        )?;
        wm_state.connection.change_window_attributes(
            state.frame_window,
            &ChangeWindowAttributesAux::new().border_pixel(wm_state.screen.white_pixel),
        )?;
        let window_name = &wm_state
            .connection
            .get_property(
                false,
                state.window,
                AtomEnum::WM_NAME,
                AtomEnum::STRING,
                0,
                u32::MAX,
            )?
            .reply()?
            .value;
        draw_bar(wm_state, &[])?;
        draw_bar(wm_state, window_name)?;
    }
    Ok(())
}

fn config_event_window<C: Connection>(
    wm_state: &WindowManagerState<C>,
    event: ConfigureRequestEvent,
) -> Result<(), ReplyOrIdError> {
    if let Some(_) = wm_state.find_window_by_id(event.window) {
        let aux = ConfigureWindowAux::from_configure_request(&event)
            .sibling(None)
            .stack_mode(None);
        println!("configuring window: {}", event.window);
        wm_state.connection.configure_window(event.window, &aux)?;
    }
    Ok(())
}

pub fn config_window<C: Connection>(
    connection: &C,
    window: &WindowState,
) -> Result<(), ReplyOrIdError> {
    println!("configing window");
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

fn draw_bar<C: Connection>(
    wm_state: &WindowManagerState<C>,
    text: &[u8],
) -> Result<(), ReplyOrIdError> {
    wm_state.connection.clear_area(
        false,
        wm_state.bar.frame_window,
        wm_state.bar.x,
        wm_state.bar.y,
        wm_state.bar.width,
        wm_state.bar.height,
    )?;
    wm_state.connection.image_text8(
        wm_state.bar.frame_window,
        wm_state.graphics_context,
        5,
        10,
        text,
    )?;
    Ok(())
}

pub fn set_font<C: Connection>(
    connection: &C,
    id_font: u32,
    id_graphics_context: u32,
    screen: &Screen,
    graphics_context: &CreateGCAux,
) -> Result<(), ReplyOrIdError> {
    connection.open_font(id_font, b"fixed")?;
    connection.create_gc(id_graphics_context, screen.root, &graphics_context)?;
    connection.close_font(id_font)?;
    Ok(())
}

fn handle_keypress<C: Connection>(
    mut wm_state: WindowManagerState<C>,
    event: KeyPressEvent,
) -> Res<C> {
    println!(
        "handling keypress with code {} and modifier {:?}",
        event.detail, event.state
    );

    if let Some(hotkey) = wm_state
        .key_state
        .hotkeys
        .iter()
        // .inspect(|h| {
        //     println!(
        //         "hotkey code {:?} mask {:?} sym {:?}",
        //         h.code, h.mask, h.main_key
        //     )
        // })
        .find(|h| event.state == h.mask && event.detail as u32 == h.code.raw())
    {
        match hotkey.action {
            HotkeyAction::SpawnAlacritty => {
                Command::new("alacritty").spawn().expect("woah");
            }
            HotkeyAction::ExitFocusedWindow => {
                wm_state
                    .connection
                    .kill_client(wm_state.connection.get_input_focus()?.reply()?.focus)?;
            }
            HotkeyAction::SwitchTag(n) => {
                println!("switching to tag {n}");
                wm_state = wm_state.change_active_tag(n).unwrap();
            }
        }
    }
    Ok(wm_state)
}

pub fn become_window_manager<C: Connection>(
    connection: &C,
    screen: &Screen,
) -> Result<(), ReplyError> {
    let change = ChangeWindowAttributesAux::default().event_mask(
        EventMask::SUBSTRUCTURE_REDIRECT
            | EventMask::SUBSTRUCTURE_NOTIFY
            | EventMask::KEY_PRESS
            | EventMask::KEY_RELEASE,
    );
    let result = connection
        .change_window_attributes(screen.root, &change)?
        .check();
    connection.set_input_focus(InputFocus::NONE, 1 as u32, CURRENT_TIME)?;
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
