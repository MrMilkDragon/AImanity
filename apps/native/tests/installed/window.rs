use super::Result;
#[cfg(windows)]
use enigo::{Button, Coordinate, Direction, Enigo, Keyboard, Mouse, Settings};
use std::{
    fs::File,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
#[cfg(windows)]
use tokio::time::Instant;
use tokio::{
    process::{Child, Command},
    time::{sleep, timeout},
};
#[cfg(windows)]
use windows_sys::Win32::{
    System::Threading::CREATE_NO_WINDOW,
    UI::WindowsAndMessaging::{PostMessageW, SetForegroundWindow, WM_CLOSE},
};
#[cfg(windows)]
use xcap::Monitor;
use xcap::Window;

pub(super) struct Client {
    child: Child,
    #[cfg(windows)]
    input: Enigo,
    evidence: PathBuf,
    window: Window,
}

impl Client {
    pub async fn launch(exe: &Path, profile: &Path, evidence: &Path, log: &Path) -> Result<Self> {
        #[cfg(windows)]
        let mut command = Command::new(exe);
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        #[cfg(target_os = "macos")]
        let mut command = Command::new("/usr/bin/sandbox-exec");
        #[cfg(target_os = "macos")]
        command.args(["-p", "(version 1)(allow default)(deny network-outbound)(allow network-outbound (remote ip \"localhost:*\"))"]).arg(exe);
        let mut child = command
            .env("AIMANITY_DATA_DIR", profile)
            .stderr(File::create(log)?)
            .stdout(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let pid = child.id().ok_or("No native process")?;
        let window = timeout(Duration::from_secs(60), async {
            loop {
                if let Some(status) = child.try_wait()? {
                    return Err(format!("Client exited: {status}").into());
                }
                if let Some(win) = Window::all()?
                    .into_iter()
                    .find(|w| w.pid().ok() == Some(pid) && w.width().is_ok_and(|v| v > 400))
                {
                    return Result::Ok(win);
                }
                sleep(Duration::from_millis(150)).await;
            }
        })
        .await??;
        #[cfg(target_os = "macos")]
        {
            use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
            let process =
                NSRunningApplication::runningApplicationWithProcessIdentifier(pid.try_into()?)
                    .ok_or("Cannot find the installed workbench process")?;
            #[allow(deprecated)]
            process.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps);
            timeout(Duration::from_secs(8), async {
                while !window.is_focused()? {
                    sleep(Duration::from_millis(50)).await;
                }
                Result::Ok(())
            })
            .await??;
        }
        #[cfg(windows)]
        unsafe {
            SetForegroundWindow(window.id()? as usize as _);
        }
        Ok(Self {
            child,
            #[cfg(windows)]
            input: Enigo::new(&Settings::default())?,
            evidence: evidence.into(),
            window,
        })
    }
    #[cfg(target_os = "macos")]
    fn pixels(&self) -> Result<(image::RgbaImage, i32, i32)> {
        let frame = self.window.capture_image()?;
        let w = self.window.width()?;
        let h = self.window.height()?;
        let pixels = if frame.dimensions() == (w, h) {
            frame
        } else {
            image::imageops::resize(&frame, w, h, image::imageops::FilterType::Triangle)
        };
        Ok((pixels, self.window.x()?, self.window.y()?))
    }
    #[cfg(windows)]
    fn pixels(&self) -> Result<(image::RgbaImage, i32, i32)> {
        let (x, y) = (self.window.x()?, self.window.y()?);
        let (w, h) = (
            i32::try_from(self.window.width()?)?,
            i32::try_from(self.window.height()?)?,
        );
        let monitor = Monitor::from_point(x + w / 2, y + h / 2)?;
        let (mx, my) = (monitor.x()?, monitor.y()?);
        let left = x.max(mx);
        let top = y.max(my);
        let right = (x + w).min(mx + i32::try_from(monitor.width()?)?);
        let bottom = (y + h).min(my + i32::try_from(monitor.height()?)?);
        let pixels = monitor.capture_region(
            (left - mx).try_into()?,
            (top - my).try_into()?,
            (right - left).try_into()?,
            (bottom - top).try_into()?,
        )?;
        Ok((pixels, left, top))
    }
    #[cfg(target_os = "macos")]
    pub async fn capture_workbench(&self, name: &str) -> Result<()> {
        timeout(Duration::from_secs(60), async {
            loop {
                if self.window.title()? == "AImanity" {
                    let (frame, _, _) = self.pixels()?;
                    let (w, h) = frame.dimensions();
                    let composer_visible = w > h
                        && (h * 88 / 100..h * 96 / 100).any(|y| {
                            (w * 80 / 100..w * 94 / 100)
                                .step_by(2)
                                .any(|x| frame.get_pixel(x, y)[0] < 150)
                        });
                    if composer_visible {
                        frame.save(self.evidence.join(name))?;
                        return Result::Ok(());
                    }
                }
                sleep(Duration::from_millis(50)).await;
            }
        })
        .await?
    }
    #[cfg(windows)]
    pub fn capture(&self, name: &str) -> Result<()> {
        self.pixels()?.0.save(self.evidence.join(name))?;
        Ok(())
    }
    pub async fn choose_english(&mut self) -> Result<()> {
        let (left, top, width, height, button_y) = timeout(Duration::from_secs(60), async {
            loop {
                let (frame, left, top) = self.pixels()?;
                let (w, h) = frame.dimensions();
                let button = (h * 78 / 100..h * 96 / 100).find(|&y| {
                    [44, 56]
                        .iter()
                        .all(|x| frame.get_pixel(w * x / 100, y)[0] < 64)
                        && frame.get_pixel(w / 3, y)[0] > 190
                });
                if let Some(y) = button {
                    frame.save(self.evidence.join("language.png"))?;
                    return Result::Ok((left, top, w as i32, h as i32, y as i32));
                }
                sleep(Duration::from_millis(150)).await;
            }
        })
        .await??;
        self.click(left + width * 64 / 100, top + height * 36 / 100)?;
        timeout(Duration::from_secs(15), async {
            loop {
                let (frame, _, _) = self.pixels()?;
                let (w, h) = frame.dimensions();
                let selected = (h * 33 / 100..h * 38 / 100)
                    .any(|y| (w * 52 / 100..w * 55 / 100).any(|x| frame.get_pixel(x, y)[0] < 64));
                if selected {
                    frame.save(self.evidence.join("english.png"))?;
                    return Result::Ok(());
                }
                sleep(Duration::from_millis(100)).await;
            }
        })
        .await??;
        self.click(left + width / 2, top + button_y + 4)
    }
    fn click(&mut self, x: i32, y: i32) -> Result<()> {
        if !self.window.is_focused()? {
            return Err("The workbench lost focus".into());
        }
        #[cfg(target_os = "macos")]
        {
            use core_graphics::{
                event::{CGEvent, CGEventTapLocation, CGEventType, CGMouseButton, EventField},
                event_source::{CGEventSource, CGEventSourceStateID},
                geometry::CGPoint,
            };
            let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
                .map_err(|_| "No session input source")?;
            let point = CGPoint::new(f64::from(x), f64::from(y));
            for kind in [
                CGEventType::MouseMoved,
                CGEventType::LeftMouseDown,
                CGEventType::LeftMouseUp,
            ] {
                let event =
                    CGEvent::new_mouse_event(source.clone(), kind, point, CGMouseButton::Left)
                        .map_err(|_| "Cannot post a workbench click")?;
                event.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, 1);
                event.post(CGEventTapLocation::HID);
            }
        }
        #[cfg(windows)]
        {
            self.input.move_mouse(x, y, Coordinate::Abs)?;
            self.input.button(Button::Left, Direction::Click)?;
        }
        Ok(())
    }
    #[cfg(windows)]
    pub async fn workspace(&mut self) -> Result<()> {
        timeout(Duration::from_secs(60), async {
            while self.window.title()? != "AImanity" {
                if let Some(status) = self.child.try_wait()? {
                    return Err(format!("Client exited: {status}").into());
                }
                sleep(Duration::from_millis(150)).await;
            }
            Result::Ok(())
        })
        .await?
    }
    #[cfg(windows)]
    pub async fn word(&mut self, word: &str, click: bool) -> Result<()> {
        let until = Instant::now() + Duration::from_secs(35);
        let image = self.evidence.join("window.png");
        let ocr = std::env::var_os("AIMANITY_QA_OCR").ok_or("Windows OCR tool missing")?;
        while Instant::now() < until {
            if let Some(status) = self.child.try_wait()? {
                return Err(format!("Client exited: {status}").into());
            }
            let (pixels, left, top) = self.pixels()?;
            let mut ink = image::imageops::grayscale(&pixels);
            for shade in ink.as_mut() {
                *shade = if *shade < 185 { 0 } else { 255 };
            }
            image::imageops::resize(
                &ink,
                ink.width() * 2,
                ink.height() * 2,
                image::imageops::FilterType::Nearest,
            )
            .save(&image)?;
            let output = Command::new(&ocr)
                .creation_flags(CREATE_NO_WINDOW)
                .arg(&image)
                .args(["stdout", "-l", "eng", "--psm", "11", "tsv"])
                .output()
                .await?;
            if !output.status.success() {
                return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
            }
            let tsv = std::str::from_utf8(&output.stdout)?;
            std::fs::write(self.evidence.join("window.tsv"), tsv)?;
            let target = tsv
                .lines()
                .filter_map(|line| {
                    let fields: Vec<_> = line.splitn(12, '\t').collect();
                    if fields.first() != Some(&"5")
                        || !fields
                            .get(11)?
                            .trim_matches(|c: char| !c.is_alphanumeric())
                            .eq_ignore_ascii_case(word)
                    {
                        return None;
                    }
                    let n = |i: usize| fields[i].parse::<i32>().ok();
                    Some((n(6)? + n(8)? / 2, n(7)? + n(9)? / 2))
                })
                .min_by_key(|&(_, y)| y);
            if let Some((x, y)) = target {
                if click {
                    self.click(left + x / 2, top + y / 2)?;
                }
                return Ok(());
            }
            sleep(Duration::from_millis(250)).await;
        }
        Err(format!("The installed client did not display {word}").into())
    }
    #[cfg(windows)]
    pub fn type_name(&mut self, value: &str) -> Result<()> {
        if !self.window.is_focused()? {
            return Err("The installed client lost focus".into());
        }
        self.input.text(value)?;
        Ok(())
    }
    pub async fn close(&mut self) -> Result<()> {
        #[cfg(target_os = "macos")]
        self.click(self.window.x()? + 14, self.window.y()? + 14)?;
        #[cfg(windows)]
        if unsafe { PostMessageW(self.window.id()? as usize as _, WM_CLOSE, 0, 0) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let status = timeout(Duration::from_secs(120), self.child.wait()).await??;
        if !status.success() {
            return Err(format!("Client shutdown: {status}").into());
        }
        Ok(())
    }
    #[cfg(windows)]
    pub async fn create_guest(&mut self, kind: &str) -> Result<()> {
        self.word("Add", true).await?;
        self.word("Name", false).await?;
        self.type_name(kind)?;
        if kind == "Desktop" {
            self.word("Android", true).await?;
            self.word("Linux", true).await?;
        }
        self.word("installation", true).await
    }
    #[cfg(windows)]
    pub async fn frame(&mut self, kind: &str, log: &Path) -> Result<()> {
        let until = Instant::now() + Duration::from_secs(1500);
        let marker = format!(" kind={kind}");
        loop {
            if let Some(status) = self.child.try_wait()? {
                return Err(format!("Client exited: {status}").into());
            }
            if let Some(line) = std::fs::read_to_string(log)?
                .lines()
                .find(|s| s.contains("native_metric first_frame ") && s.ends_with(&marker))
            {
                println!("{line}");
                return Ok(());
            }
            if Instant::now() >= until {
                return Err(format!("No rendered {kind} frame").into());
            }
            sleep(Duration::from_millis(500)).await;
        }
    }
}
