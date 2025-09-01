use super::{
    server::EVENT_PROXY,
    win_linux::{create_font_face, draw_text, Ripple},
    Cursor, CustomEvent,
};
use crate::ipc::{Connection, Data};
use hbb_common::{
    allow_err, bail, log,
    tokio::{self, sync::mpsc::unbounded_channel},
    ResultType,
};
use softbuffer::{Context, Surface};
use std::{collections::HashMap, num::NonZeroU32, sync::Arc, time::Instant};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, PixmapMut, Stroke, Transform};
use ttf_parser::Face;
use winit::raw_window_handle::{
    DisplayHandle, HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use winit::{
    application::ApplicationHandler,
    dpi::{PhysicalPosition, PhysicalSize},
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    platform::x11::{WindowAttributesExtX11, WindowType},
    window::{Window, WindowId, WindowLevel},
};
use x11::xfixes::{XFixesCreateRegion, XFixesDestroyRegion, XFixesSetWindowShapeRegion};

const SHAPE_INPUT: std::ffi::c_int = 2;

pub fn run() {
    let event_loop = match EventLoop::<(String, CustomEvent)>::with_user_event().build() {
        Ok(el) => el,
        Err(e) => {
            log::error!("Failed to create event loop: {}", e);
            return;
        }
    };

    let event_loop_proxy = event_loop.create_proxy();
    EVENT_PROXY.write().unwrap().replace(event_loop_proxy);

    let (tx_exit, rx_exit) = unbounded_channel();
    std::thread::spawn(move || {
        super::server::start_ipc(rx_exit);
    });

    let mut app = WhiteboardApplication::default();
    if let Err(e) = event_loop.run_app(&mut app) {
        log::error!("Failed to run app: {}", e);
        tx_exit.send(()).ok();
        return;
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
                            Data::Whiteboard((k, evt)) => {
                                if matches!(evt, CustomEvent::Exit) {
                                    log::info!("whiteboard ipc connection closed");
                                    break;
                                } else {
                                    EVENT_PROXY.read().unwrap().as_ref().map(|ep| {
                                        allow_err!(ep.send_event((k, evt)));
                                    });
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
    EVENT_PROXY.read().unwrap().as_ref().map(|ep| {
        allow_err!(ep.send_event(("".to_string(), CustomEvent::Exit)));
    });
}

struct WindowState {
    window: Arc<Window>,
    // Drawing context.
    //
    // With OpenGL it could be EGLDisplay.
    context: Option<Context<DisplayHandle<'static>>>,
    // NOTE: This surface must be dropped before the `Window`.
    surface: Surface<DisplayHandle<'static>, Arc<Window>>,
    face: Option<Face<'static>>,
    ripples: Vec<Ripple>,
    last_cursors: HashMap<String, Cursor>,
}

#[derive(Default)]
struct WhiteboardApplication {
    windows: Vec<WindowState>,
    close_requested: bool,
}

impl ApplicationHandler<(String, CustomEvent)> for WhiteboardApplication {
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, (k, evt): (String, CustomEvent)) {
        match evt {
            CustomEvent::Cursor(cursor) => {
                if let Some(state) = self.windows.first_mut() {
                    if cursor.btns != 0 {
                        state.ripples.push(Ripple {
                            x: cursor.x,
                            y: cursor.y,
                            start_time: Instant::now(),
                        });
                    }
                    state.last_cursors.insert(k, cursor);
                    state.window.request_redraw();
                }
            }
            CustomEvent::Exit => {
                self.close_requested = true;
            }
            _ => {}
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let (x, y, w, h) = match super::server::get_displays_rect() {
            Ok(r) => r,
            Err(err) => {
                log::error!("Failed to get displays rect: {}", err);
                self.close_requested = true;
                return;
            }
        };

        let window_attributes = Window::default_attributes()
            .with_title("RustDesk whiteboard")
            .with_inner_size(PhysicalSize::new(w, h))
            .with_position(PhysicalPosition::new(x, y))
            .with_decorations(false)
            .with_transparent(true)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_x11_window_type(vec![WindowType::Utility])
            .with_override_redirect(true);

        let window = match event_loop.create_window(window_attributes) {
            Ok(window) => Arc::new(window),
            Err(e) => {
                log::error!("Failed to create window: {}", e);
                self.close_requested = true;
                return;
            }
        };

        let display = match window.display_handle() {
            Ok(d) => d,
            Err(e) => {
                log::error!("Failed to get display handle: {}", e);
                self.close_requested = true;
                return;
            }
        };
        let rwh = match window.window_handle() {
            Ok(w) => w,
            Err(e) => {
                log::error!("Failed to get window handle: {}", e);
                self.close_requested = true;
                return;
            }
        };

        // Both the following block and `window.set_cursor_hittest(false)` in `draw()` are necessary. 
        // No idea why these two blocks can make `set_cursor_hittest(false)` work properly (cursor events passthrough).
        match (rwh.as_raw(), display.as_raw()) {
            (RawWindowHandle::Xlib(xlib_window), RawDisplayHandle::Xlib(xlib_display)) => {
                unsafe {
                    let xwindow = xlib_window.window;
                    if let Some(display_ptr) = xlib_display.display {
                        let xdisplay = display_ptr.as_ptr() as *mut x11::xlib::Display;
                        // Mouse event passthrough
                        let empty_region = XFixesCreateRegion(xdisplay, std::ptr::null_mut(), 0);
                        XFixesSetWindowShapeRegion(
                            xdisplay,
                            xwindow,
                            SHAPE_INPUT,
                            0,
                            0,
                            empty_region,
                        );
                        XFixesDestroyRegion(xdisplay, empty_region);
                    }
                }
            }
            _ => {
                log::error!("Unsupported windowing system for shape extension");
                self.close_requested = true;
                return;
            }
        }

        // SAFETY: we drop the context right before the event loop is stopped, thus making it safe.
        let context = match Context::new(unsafe {
            std::mem::transmute::<DisplayHandle<'_>, DisplayHandle<'static>>(display)
        }) {
            Ok(ctx) => Some(ctx),
            Err(e) => {
                log::error!("Failed to create context: {}", e);
                self.close_requested = true;
                return;
            }
        };

        let Some(ctx) = context.as_ref() else {
            // unreachable
            self.close_requested = true;
            return;
        };

        let surface = match Surface::new(ctx, window.clone()) {
            Ok(s) => s,
            Err(e) => {
                log::error!("Failed to create surface: {}", e);
                self.close_requested = true;
                return;
            }
        };
        let face = match create_font_face() {
            Ok(face) => Some(face),
            Err(err) => {
                log::error!("Failed to create font face: {}", err);
                None
            }
        };

        let state = WindowState {
            window,
            context,
            surface,
            face,
            ripples: Vec::new(),
            last_cursors: HashMap::new(),
        };

        self.windows.push(state);
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                self.close_requested = true;
            }
            WindowEvent::RedrawRequested => {
                let Some(state) = self.windows.iter_mut().find(|w| w.window.id() == window_id)
                else {
                    log::error!("No window found for id: {:?}", window_id);
                    return;
                };
                if let Err(err) = state.draw() {
                    log::error!("Failed to draw window: {}", err);
                }
            }
            _ => (),
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if !self.close_requested {
            for state in self.windows.iter() {
                state.window.request_redraw();
            }
        } else {
            event_loop.exit();
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        // We must drop the context here.
        for state in self.windows.iter_mut() {
            state.context = None;
        }
    }
}

impl WindowState {
    fn draw(&mut self) -> ResultType<()> {
        let (width, height) = {
            let size = self.window.inner_size();
            (size.width, size.height)
        };

        let (Some(width), Some(height)) = (NonZeroU32::new(width), NonZeroU32::new(height)) else {
            bail!("Invalid window size, {width}x{height}")
        };
        if let Err(e) = self.surface.resize(width, height) {
            bail!("Failed to resize surface: {}", e);
        }

        let mut buffer = match self.surface.buffer_mut() {
            Ok(buf) => buf,
            Err(e) => {
                bail!("Failed to get buffer: {}", e);
            }
        };

        let Some(mut pixmap) = PixmapMut::from_bytes(
            bytemuck::cast_slice_mut(&mut buffer),
            width.get(),
            height.get(),
        ) else {
            bail!("Failed to create pixmap from buffer");
        };
        pixmap.fill(Color::TRANSPARENT);

        let ripple_duration = std::time::Duration::from_millis(500);
        self.ripples
            .retain(|r| r.start_time.elapsed() < ripple_duration);

        for ripple in &self.ripples {
            let elapsed = ripple.start_time.elapsed();
            let progress = elapsed.as_secs_f32() / ripple_duration.as_secs_f32();
            let radius = 45.0 * progress;
            let alpha = 1.0 - progress;

            let mut ripple_paint = Paint::default();
            ripple_paint.set_color_rgba8(255, 0, 0, (alpha * 128.0) as u8);
            ripple_paint.anti_alias = true;

            let mut ripple_pb = PathBuilder::new();
            let (rx, ry) = (ripple.x as f64, ripple.y as f64);
            ripple_pb.push_circle(rx as f32, ry as f32, radius as f32);
            if let Some(path) = ripple_pb.finish() {
                pixmap.fill_path(
                    &path,
                    &ripple_paint,
                    FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
        }

        for cursor in self.last_cursors.values() {
            let (x, y) = (cursor.x as f64, cursor.y as f64);
            let (x, y) = (x as f32, y as f32);
            let size = 1.5 as f32;

            let mut pb = PathBuilder::new();
            pb.move_to(x, y);
            pb.line_to(x, y + 16.0 * size);
            pb.line_to(x + 4.0 * size, y + 13.0 * size);
            pb.line_to(x + 7.0 * size, y + 20.0 * size);
            pb.line_to(x + 9.0 * size, y + 19.0 * size);
            pb.line_to(x + 6.0 * size, y + 12.0 * size);
            pb.line_to(x + 11.0 * size, y + 12.0 * size);
            pb.close();

            if let Some(path) = pb.finish() {
                let mut arrow_paint = Paint::default();
                arrow_paint.set_color_rgba8(
                    (cursor.argb >> 16 & 0xFF) as u8,
                    (cursor.argb >> 8 & 0xFF) as u8,
                    (cursor.argb & 0xFF) as u8,
                    (cursor.argb >> 24 & 0xFF) as u8,
                );
                arrow_paint.anti_alias = true;
                pixmap.fill_path(
                    &path,
                    &arrow_paint,
                    FillRule::Winding,
                    Transform::identity(),
                    None,
                );

                let mut black_paint = Paint::default();
                black_paint.set_color_rgba8(0, 0, 0, 255);
                black_paint.anti_alias = true;
                let mut stroke = Stroke::default();
                stroke.width = 1.0 as f32;
                pixmap.stroke_path(&path, &black_paint, &stroke, Transform::identity(), None);

                self.face.as_ref().map(|face| {
                    draw_text(
                        &mut pixmap,
                        face,
                        &cursor.text,
                        x + 24.0 * size,
                        y + 24.0 * size,
                        &arrow_paint,
                        24.0 as f32,
                    );
                });
            }
        }

        self.window.pre_present_notify();

        if let Err(e) = buffer.present() {
            log::error!("Failed to present buffer: {}", e);
        }

        self.window.set_cursor_hittest(false).ok();

        Ok(())
    }
}
