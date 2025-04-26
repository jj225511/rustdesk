use crate::clipboard::{update_clipboard, ClipboardSide};
use hbb_common::{bail, message_proto::*, ResultType};
use std::{
    fs::File,
    sync::Mutex,
    time::{Duration, Instant},
};

#[cfg(not(feature = "flutter"))]
lazy_static::lazy_static! {
    static ref SCREENSHOT: Mutex<Screenshot> = Default::default();
}

pub enum ScreenshotAction {
    SaveAs(String),
    CopyToClipboard,
    Discard,
}

impl Default for ScreenshotAction {
    fn default() -> Self {
        Self::Discard
    }
}

impl From<&str> for ScreenshotAction {
    fn from(value: &str) -> Self {
        let mut vs = value.split(":");
        match vs.next() {
            Some("0") => {
                if let Some(p) = vs.next() {
                    Self::SaveAs(p.to_owned())
                } else {
                    Self::default()
                }
            }
            Some("1") => Self::CopyToClipboard,
            Some("2") => Self::default(),
            _ => Self::default(),
        }
    }
}

impl Into<String> for ScreenshotAction {
    fn into(self) -> String {
        match self {
            Self::SaveAs(p) => format!("0:{p}"),
            Self::CopyToClipboard => "1".to_owned(),
            Self::Discard => "2".to_owned(),
        }
    }
}

pub struct Screenshot {
    display: Option<i32>,
    req_tm: Instant,
    rgba: Option<scrap::ImageRgb>,
}

impl Default for Screenshot {
    fn default() -> Self {
        Self {
            display: None,
            req_tm: Instant::now(),
            rgba: None,
        }
    }
}

impl Screenshot {
    pub fn set_take_screenshot(&mut self, display: i32) {
        self.display.replace(display);
        self.req_tm = Instant::now();
    }

    pub fn try_take_screenshot(&mut self, display: i32, rgba: &scrap::ImageRgb) -> bool {
        // Don't handle screenshot request before 3 seconds ago
        if self.req_tm.elapsed() > Duration::from_millis(3_000) {
            self.display = None;
            return false;
        }
        if let Some(d) = &self.display {
            if *d != display {
                return false;
            }

            self.rgba = Some(rgba.clone());
            self.display = None;
            return true;
        }
        false
    }

    pub fn handle_screenshot(&mut self, action: &str) -> String {
        match self.handle_screenshot_(action) {
            Ok(()) => "".to_owned(),
            Err(e) => e.to_string(),
        }
    }

    fn handle_screenshot_(&mut self, action: &str) -> ResultType<()> {
        let Some(rgba) = self.rgba.take() else {
            bail!("No cached screenshot");
        };
        match ScreenshotAction::from(action) {
            ScreenshotAction::SaveAs(p) => {
                repng::encode(File::create(p)?, rgba.w as u32, rgba.h as u32, &rgba.raw)?;
            }
            ScreenshotAction::CopyToClipboard => {
                let clips = vec![Clipboard {
                    compress: false,
                    width: rgba.w as _,
                    height: rgba.h as _,
                    content: rgba.raw.into(),
                    format: ClipboardFormat::ImageRgba.into(),
                    ..Default::default()
                }];
                update_clipboard(clips, ClipboardSide::Client);
            }
            ScreenshotAction::Discard => {}
        }
        Ok(())
    }
}

#[cfg(not(feature = "flutter"))]
pub fn set_take_screenshot(display: i32) {
    SCREENSHOT.lock().unwrap().set_take_screenshot(display);
}

#[cfg(not(feature = "flutter"))]
pub fn try_take_screenshot(display: i32, rgba: &scrap::ImageRgb) -> bool {
    SCREENSHOT
        .lock()
        .unwrap()
        .try_take_screenshot(display, rgba)
}

#[cfg(not(feature = "flutter"))]
pub fn handle_screenshot(action: &str) -> String {
    SCREENSHOT.lock().unwrap().handle_screenshot(action)
}
