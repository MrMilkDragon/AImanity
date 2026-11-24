use super::Result;
use enigo::{Button, Coordinate, Direction, Enigo, Keyboard, Mouse, Settings};
use std::{
    fs::File,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    process::{Child, Command},
    time::{Instant, sleep, timeout},
};
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, SetForegroundWindow, WM_CLOSE};
use xcap::{Monitor, Window};

pub(super) struct Client {
    child: Child,
    input: Enigo,
    evidence: PathBuf,
    window: Window,
}

impl Client {
    pub async fn launch(exe: &Path, profile: &Path, evidence: &Path, log: &Path) -> Result<Self> {
        let mut child = Command::new(exe)
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
        unsafe {
            SetForegroundWindow(window.id()? as usize as _);
        }
        Ok(Self {
            child,
            input: Enigo::new(&Settings::default())?,
            evidence: evidence.into(),
            window,
        })
    }
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
    pub fn capture(&self, name: &str) -> Result<()> {
        self.pixels()?.0.save(self.evidence.join(name))?;
        Ok(())
    }
    pub async fn word(&mut self, word: &str, click: bool) -> Result<()> {
        let until = Instant::now() + Duration::from_secs(35);
        let image = self.evidence.join("window.png");
        let ocr = std::env::var_os("AIMANITY_QA_OCR").ok_or("Windows OCR tool missing")?;
        while Instant::now() < until {
            if let Some(status) = self.child.try_wait()? {
                return Err(format!("Client exited: {status}").into());
            }
            let (pixels, left, top) = self.pixels()?;
            image::imageops::resize(
                &pixels,
                pixels.width() * 2,
                pixels.height() * 2,
                image::imageops::FilterType::Triangle,
            )
            .save(&image)?;
            let output = Command::new(&ocr)
                .arg(&image)
                .args(["stdout", "-l", "eng", "--psm", "11", "tsv"])
                .output()
                .await?;
            if !output.status.success() {
                return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
            }
            let tsv = std::str::from_utf8(&output.stdout)?;
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
                    if !self.window.is_focused()? {
                        return Err("The installed client lost focus".into());
                    }
                    self.input
                        .move_mouse(left + x / 2, top + y / 2, Coordinate::Abs)?;
                    self.input.button(Button::Left, Direction::Click)?;
                }
                return Ok(());
            }
            sleep(Duration::from_millis(250)).await;
        }
        Err(format!("The installed client did not display {word}").into())
    }
    pub fn type_name(&mut self, value: &str) -> Result<()> {
        if !self.window.is_focused()? {
            return Err("The installed client lost focus".into());
        }
        self.input.text(value)?;
        Ok(())
    }
    pub async fn close(&mut self) -> Result<()> {
        if unsafe { PostMessageW(self.window.id()? as usize as _, WM_CLOSE, 0, 0) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let status = timeout(Duration::from_secs(120), self.child.wait()).await??;
        if !status.success() {
            return Err(format!("Client shutdown: {status}").into());
        }
        Ok(())
    }
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
