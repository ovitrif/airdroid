use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::command_path::resolve_program;

#[derive(Debug, Clone)]
pub struct Scrcpy {
    path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrcpyRunMode {
    Background,
    Foreground,
}

impl Scrcpy {
    pub fn resolve(override_path: Option<PathBuf>, skip_check: bool) -> Result<Self> {
        let path = if skip_check {
            override_path.unwrap_or_else(|| PathBuf::from("scrcpy"))
        } else {
            resolve_program("scrcpy", override_path)?
        };

        Ok(Self { path })
    }

    pub fn launch(&self, serial: &str, options: &ScrcpyOptions) -> Result<()> {
        let status = Command::new(&self.path)
            .args(default_args(serial, options))
            .status()
            .with_context(|| format!("failed to run {}", self.path.display()))?;

        if status.success() {
            return Ok(());
        }

        bail!("scrcpy exited with status {status}");
    }

    pub fn launch_background(&self, serial: &str, options: &ScrcpyOptions) -> Result<u32> {
        let child = self.spawn(serial, options, ScrcpyRunMode::Background)?;
        Ok(child.id())
    }

    pub fn spawn(
        &self,
        serial: &str,
        options: &ScrcpyOptions,
        mode: ScrcpyRunMode,
    ) -> Result<Child> {
        let mut command = Command::new(&self.path);
        command.args(default_args(serial, options));

        if mode == ScrcpyRunMode::Background {
            command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
        }

        let child = command
            .spawn()
            .with_context(|| format!("failed to run {}", self.path.display()))?;
        Ok(child)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrcpyOptions {
    pub no_audio: bool,
    pub stay_awake: bool,
    pub borderless: bool,
    pub always_on_top: bool,
    pub window_title: String,
}

impl Default for ScrcpyOptions {
    fn default() -> Self {
        Self {
            no_audio: true,
            stay_awake: true,
            borderless: true,
            always_on_top: false,
            window_title: "Pixel 10 Pro".to_string(),
        }
    }
}

pub fn default_args(serial: &str, options: &ScrcpyOptions) -> Vec<OsString> {
    let mut args = vec![OsString::from("-s"), OsString::from(serial)];

    if options.no_audio {
        args.push(OsString::from("--no-audio"));
    }

    if options.stay_awake {
        args.push(OsString::from("--stay-awake"));
    }

    if options.borderless {
        args.push(OsString::from("--window-borderless"));
    }

    if options.always_on_top {
        args.push(OsString::from("--always-on-top"));
    }

    if !options.window_title.is_empty() {
        args.push(OsString::from("--window-title"));
        args.push(OsString::from(&options.window_title));
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_raycast_equivalent_scrcpy_args() {
        let args = default_args("192.168.1.23:40233", &ScrcpyOptions::default());
        let args: Vec<_> = args
            .iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();

        assert_eq!(
            args,
            vec![
                "-s",
                "192.168.1.23:40233",
                "--no-audio",
                "--stay-awake",
                "--window-borderless",
                "--window-title",
                "Pixel 10 Pro",
            ]
        );
    }

    #[test]
    fn builds_plain_window_scrcpy_args() {
        let options = ScrcpyOptions {
            borderless: false,
            always_on_top: true,
            window_title: "Ovi Pixel".to_string(),
            ..ScrcpyOptions::default()
        };
        let args = default_args("device", &options);
        let args: Vec<_> = args
            .iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();

        assert_eq!(
            args,
            vec![
                "-s",
                "device",
                "--no-audio",
                "--stay-awake",
                "--always-on-top",
                "--window-title",
                "Ovi Pixel",
            ]
        );
    }
}
