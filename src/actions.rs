use std::cmp::Reverse;

use x11rb::connection::Connection;
use x11rb::errors::ReplyOrIdError;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::*;
use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME};

use crate::state::*;

pub fn handle_event<C: Connection>(
    wm_state: &mut WindowManagerState<C>,
    event: Event,
) -> Result<(), ReplyOrIdError> {
    match event {
        Event::MapRequest(event) => handle_map(wm_state, event),
        Event::UnmapNotify(event) => unmap_window(wm_state, event),
        Event::ConfigureRequest(event) => config_event_window(wm_state, event),
        Event::EnterNotify(event) => set_focus_window(wm_state, event),
        _ => Ok(()),
    }
}

fn handle_map<C: Connection>(
    wm_state: &mut WindowManagerState<C>,
    event: MapRequestEvent,
) -> Result<(), ReplyOrIdError> {
    if let Some(window) = wm_state.find_window_by_id(event.window) {
        let window = *window;
        create_and_map_window(wm_state, &window)?;
    }
    Ok(())
}

pub fn create_and_map_window<C: Connection>(
    wm_state: &mut WindowManagerState<C>,
    window: &WindowState,
) -> Result<(), ReplyOrIdError> {
    println!("creating window: {}",window.window);
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
    Ok(())
}

fn unmap_window<C: Connection>(
    wm_state: &WindowManagerState<C>,
    event: UnmapNotifyEvent,
) -> Result<(), ReplyOrIdError> {
    if let Some(window) = wm_state.find_window_by_id(event.window) {
        println!("unmapping window: {}",window.window);
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
    }
    Ok(())
}

fn set_focus_window<C: Connection>(
    wm_state: &WindowManagerState<C>,
    event: EnterNotifyEvent,
) -> Result<(), ReplyOrIdError> {
    if let Some(state) = wm_state.find_window_by_id(event.event) {
        println!("setting focus to: {:?}", state.window);
        // Set the input focus (ignoring ICCCM's WM_PROTOCOLS / WM_TAKE_FOCUS)
        wm_state
            .connection
            .set_input_focus(InputFocus::PARENT, state.window, CURRENT_TIME)?;

        wm_state.windows.iter().for_each(|w| {
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
        println!("bar text: {:?} event id {}", window_name, state.window);
        wm_state.draw_bar(&[])?;
        wm_state.draw_bar(window_name)?;
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
    println!("configuring window: {}",event.window);
    wm_state.connection.configure_window(event.window, &aux)?;
    Ok(())
}

pub fn config_window<C: Connection>(
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
