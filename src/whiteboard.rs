use crate::ipc::{self, new_listener, Connection, Data};
use hbb_common::{
    allow_err,
    anyhow::anyhow,
    bail, log, sleep,
    tokio::{
        self,
        sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    },
    ResultType,
};
use lazy_static::lazy_static;
use serde_derive::{Deserialize, Serialize};
use softbuffer::{Context, Surface};
use std::{
    num::NonZeroU32,
    sync::{Arc, RwLock},
};
#[cfg(target_os = "macos")]
use tao::platform::macos::WindowBuilderExtMacOS;
#[cfg(target_os = "linux")]
use tao::platform::unix::WindowBuilderExtUnix;
#[cfg(target_os = "windows")]
use tao::platform::windows::WindowBuilderExtWindows;
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy, EventLoopWindowTarget},
    window::WindowBuilder,
};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, PixmapMut, Point, Rect, Stroke, Transform};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem},
    TrayIcon, TrayIconBuilder, TrayIconEvent as TrayEvent,
};
use ttf_parser::Face;

lazy_static! {
    static ref EVENT_PROXY: RwLock<Option<EventLoopProxy<CustomEvent>>> = RwLock::new(None);
    static ref TX_WHITEBOARD: RwLock<Option<UnboundedSender<CustomEvent>>> = RwLock::new(None);
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "t", content = "c")]
pub enum CustomEvent {
    Cursor(Cursor),
    Clear,
    Close,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "t")]
pub struct Cursor {
    pub x: f32,
    pub y: f32,
    pub text: String,
}

pub fn start_whiteboard() {
    std::thread::spawn(|| {
        allow_err!(start_whiteboard_());
    });
}

pub fn stop_whiteboard() {
    std::thread::spawn(|| {
        let mut whiteboard = TX_WHITEBOARD.write().unwrap();
        whiteboard.as_ref().map(|tx| {
            allow_err!(tx.send(CustomEvent::Close));
            // to-do: wait the clipboard process exiting.
            // Simple sleep for now.
            std::thread::sleep(std::time::Duration::from_millis(1_000));
        });
        whiteboard.take();
    });
}

pub fn update_whiteboard(e: CustomEvent) {
    TX_WHITEBOARD.read().unwrap().as_ref().map(|tx| {
        allow_err!(tx.send(e));
    });
}

#[tokio::main(flavor = "current_thread")]
async fn start_whiteboard_() -> ResultType<()> {
    let mut tx_whiteboard = TX_WHITEBOARD.write().unwrap();
    if tx_whiteboard.is_some() {
        log::warn!("Whiteboard already started");
        return Ok(());
    }

    loop {
        if !crate::platform::is_prelogin() {
            break;
        }
        sleep(1.).await;
    }
    let mut stream = None;
    if let Ok(s) = ipc::connect(1000, "_whiteboard").await {
        stream = Some(s);
    } else {
        #[allow(unused_mut)]
        #[allow(unused_assignments)]
        let mut args = vec!["--whiteboard"];
        #[allow(unused_mut)]
        #[cfg(target_os = "linux")]
        let mut user = None;

        let run_done;
        if crate::platform::is_root() {
            let mut res = Ok(None);
            for _ in 0..10 {
                #[cfg(not(any(target_os = "linux")))]
                {
                    log::debug!("Start whiteboard");
                    res = crate::platform::run_as_user(args.clone());
                }
                #[cfg(target_os = "linux")]
                {
                    log::debug!("Start whiteboard");
                    res = crate::platform::run_as_user(
                        args.clone(),
                        user.clone(),
                        None::<(&str, &str)>,
                    );
                }
                if res.is_ok() {
                    break;
                }
                log::error!("Failed to run whiteboard: {res:?}");
                sleep(1.).await;
            }
            if let Some(task) = res? {
                super::CHILD_PROCESS.lock().unwrap().push(task);
            }
            run_done = true;
        } else {
            run_done = false;
        }
        if !run_done {
            log::debug!("Start whiteboard");
            super::CHILD_PROCESS
                .lock()
                .unwrap()
                .push(crate::run_me(args)?);
        }
        for _ in 0..20 {
            sleep(0.3).await;
            if let Ok(s) = ipc::connect(1000, "_whiteboard").await {
                stream = Some(s);
                break;
            }
        }
        if stream.is_none() {
            bail!("Failed to connect to connection manager");
        }
    }

    let mut stream = stream.ok_or(anyhow!("none stream"))?;
    let (tx, mut rx) = unbounded_channel();
    tx_whiteboard.replace(tx);
    drop(tx_whiteboard);
    let _call_on_ret = crate::common::SimpleCallOnReturn {
        b: true,
        f: Box::new(move || {
            let _ = TX_WHITEBOARD.write().unwrap().take();
        }),
    };
    loop {
        tokio::select! {
            res = rx.recv() => {
                match res {
                    Some(data) => {
                        let is_close = matches!(data, CustomEvent::Close);
                        allow_err!(stream.send(&Data::Whiteboard(data)).await);
                        if is_close {
                            break;
                        }
                    }
                    None => {
                        bail!("expected");
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn run() {
    let (tx_exit, rx_exit) = unbounded_channel();
    std::thread::spawn(move || {
        start_ipc(rx_exit);
    });
    if let Err(e) = create_event_loop() {
        log::error!("Failed to create event loop: {}", e);
        tx_exit.send(()).ok();
        return;
    }
}

#[tokio::main(flavor = "current_thread")]
async fn start_ipc(mut rx_exit: UnboundedReceiver<()>) {
    match new_listener("_whiteboard").await {
        Ok(mut incoming) => loop {
            tokio::select! {
                _ = rx_exit.recv() => {
                    log::info!("Exiting IPC");
                    break;
                }
                res = incoming.next() => match res {
                    Some(result) => match result {
                        Ok(stream) => {
                            log::debug!("Got new connection");
                            tokio::spawn(handle_new_stream(Connection::new(stream)));
                        }
                        Err(err) => {
                            log::error!("Couldn't get whiteboard client: {:?}", err);
                        }
                    },
                    None => {
                        log::error!("Failed to get whiteboard client");
                    }
                }
            }
        },
        Err(err) => {
            log::error!("Failed to start whiteboard ipc server: {}", err);
        }
    }
}

async fn handle_new_stream(mut conn: Connection) {
    loop {
        tokio::select! {
            res = conn.next() => {
                match res {
                    Err(err) => {
                        log::info!("whiteboard ipc connection closed: {}", err);
                        break;
                    }
                    Ok(Some(data)) => {
                        match data {
                            Data::Whiteboard(evt) => {
                                let is_close = matches!(evt, CustomEvent::Close);
                                EVENT_PROXY.read().unwrap().as_ref().map(|ep| {
                                    allow_err!(ep.send_event(evt));
                                });
                                if is_close {
                                    log::info!("whiteboard ipc connection closed");
                                    break;
                                }
                            }
                            _ => {

                            }
                        }
                    }
                    Ok(None) => {
                        log::info!("whiteboard ipc connection closed");
                        break;
                    }
                }
            }
        }
    }
}

fn create_event_loop() -> ResultType<()> {
    let event_loop = EventLoopBuilder::<CustomEvent>::with_user_event().build();

    let mut window_builder = WindowBuilder::new()
        .with_title("Whiteboard Demo")
        .with_transparent(true)
        .with_always_on_top(true);

    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::WindowBuilderExtMacOS;
        // Create a borderless window, we will set all other properties manually.
        window_builder = window_builder
            .with_decorations(false)
            .with_has_shadow(false);
    }
    #[cfg(not(target_os = "macos"))]
    {
        // For other platforms, borderless fullscreen is fine.
        window_builder =
            window_builder.with_fullscreen(Some(tao::window::Fullscreen::Borderless(None)));
    }

    let window = Arc::new(window_builder.build::<CustomEvent>(&event_loop)?);
    window.set_ignore_cursor_events(true)?;

    #[cfg(target_os = "macos")]
    {
        use cocoa::appkit::{NSColor, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask};
        use cocoa::base::{id, nil, YES};
        use objc::{class, msg_send, sel, sel_impl};
        use raw_window_handle::{HasRawWindowHandle, RawWindowHandle};

        // Manually set size and position to cover the whole screen.
        if let Some(monitor) = window.current_monitor() {
            window.set_inner_size(monitor.size());
            window.set_outer_position(monitor.position());
        }

        // Based on working examples and direct system values, we now manually
        // set all the required properties on the native NSWindow object.
        if let Ok(handle) = window.raw_window_handle() {
            if let RawWindowHandle::AppKit(appkit_handle) = handle {
                let ns_view = appkit_handle.ns_view.as_ptr() as id;
                let ns_window: id = unsafe { msg_send![ns_view, window] };
                if ns_window != nil {
                    unsafe {
                        // 1. Set the style mask to be borderless.
                        let _: () = msg_send![ns_window, setStyleMask:
   NSWindowStyleMask::NSBorderlessWindowMask];

                        // 2. Set the window level to be the screen saver level (1000) minus one.
                        // This is an extremely high level to ensure it's on top of everything.
                        let level = 1000 - 1;
                        let _: () = msg_send![ns_window, setLevel: level as i64];

                        // 3. Set the collection behavior to appear on all spaces.
                        let behavior = NSWindowCollectionBehavior::NSWindowCollectionBehaviorCanJoinAllSpaces
                               | NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary;
                        let _: () = msg_send![ns_window, setCollectionBehavior: behavior];

                        // 4. Force the window to be transparent.
                        let clear_color: id = msg_send![class!(NSColor), clearColor];
                        let _: () = msg_send![ns_window, setBackgroundColor: clear_color];
                        let _: () = msg_send![ns_window, setOpaque: false];

                        // 5. Ensure the window ignores mouse events for click-through.
                        let _: () = msg_send![ns_window, setIgnoresMouseEvents: YES];
                    }
                }
            }
        }
    }

    let context = Context::new(window.clone()).map_err(|e| {
        log::error!("Failed to create context: {}", e);
        anyhow!(e.to_string())
    })?;
    let mut surface = Surface::new(&context, window.clone()).map_err(|e| {
        log::error!("Failed to create surface: {}", e);
        anyhow!(e.to_string())
    })?;

    let proxy = event_loop.create_proxy();
    EVENT_PROXY.write().unwrap().replace(proxy);
    let _call_on_ret = crate::common::SimpleCallOnReturn {
        b: true,
        f: Box::new(move || {
            let _ = EVENT_PROXY.write().unwrap().take();
        }),
    };

    println!("============================== create event loop");

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                println!("Close requested, exiting...");
                *control_flow = ControlFlow::Exit;
            }
            Event::MainEventsCleared => {
                window.request_redraw();
            }
            Event::UserEvent(evt) => match evt {
                CustomEvent::Cursor(cursor) => {
                    let (width, height) = {
                        let size = window.inner_size();
                        (size.width, size.height)
                    };

                    let (Some(width), Some(height)) =
                        (NonZeroU32::new(width), NonZeroU32::new(height))
                    else {
                        return;
                    };
                    if let Err(e) = surface.resize(width, height) {
                        log::error!("Failed to resize surface: {}", e);
                        return;
                    }

                    let mut buffer = match surface.buffer_mut() {
                        Ok(buf) => buf,
                        Err(e) => {
                            log::error!("Failed to get buffer: {}", e);
                            return;
                        }
                    };
                    let Some(mut pixmap) = PixmapMut::from_bytes(
                        bytemuck::cast_slice_mut(&mut buffer),
                        width.get(),
                        height.get(),
                    ) else {
                        log::error!("Failed to create pixmap from buffer");
                        return;
                    };
                    pixmap.fill(Color::TRANSPARENT);

                    let mut paint = Paint::default();
                    paint.anti_alias = true;

                    let mut pb = PathBuilder::new();
                    pb.move_to(cursor.x + 50.0, cursor.y);
                    pb.line_to(cursor.x, cursor.y + 100.0);
                    pb.line_to(cursor.x + 100.0, cursor.y + 100.0);
                    pb.close();
                    let Some(path) = pb.finish() else {
                        log::error!("Failed to create path");
                        return;
                    };
                    paint.set_color_rgba8(0, 100, 255, 255);
                    pixmap.stroke_path(
                        &path,
                        &paint,
                        &Stroke::default(),
                        Transform::identity(),
                        None,
                    );

                    if let Err(e) = buffer.present() {
                        log::error!("Failed to present surface: {}", e);
                        return;
                    }
                    window.request_redraw();
                }
                _ => {}
            },
            _ => (),
        }
    });
}
