use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::{Command, ExitStatus};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::command_path::resolve_program;

const PAIRING_SERVICE_TYPE: &str = "_adb-tls-pairing._tcp";
const CONNECT_SERVICE_TYPE: &str = "_adb-tls-connect._tcp";

#[derive(Debug, Clone)]
pub struct Adb {
    path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct CommandResult {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

impl CommandResult {
    pub fn combined_output(&self) -> String {
        let stdout = self.stdout.trim();
        let stderr = self.stderr.trim();

        match (stdout.is_empty(), stderr.is_empty()) {
            (true, true) => String::new(),
            (false, true) => stdout.to_string(),
            (true, false) => stderr.to_string(),
            (false, false) => format!("{stdout}\n{stderr}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdbDevice {
    pub serial: String,
    pub state: DeviceState,
    pub product: Option<String>,
    pub model: Option<String>,
    pub device: Option<String>,
    pub transport_id: Option<String>,
}

impl AdbDevice {
    pub fn display_name(&self) -> String {
        match &self.model {
            Some(model) => format!("{} {}", self.serial, model.replace('_', " ")),
            None => self.serial.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceState {
    Device,
    Offline,
    Unauthorized,
    Other(String),
}

impl DeviceState {
    fn from_adb(value: &str) -> Self {
        match value {
            "device" => Self::Device,
            "offline" => Self::Offline,
            "unauthorized" => Self::Unauthorized,
            other => Self::Other(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MdnsService {
    pub instance: String,
    pub service_type: String,
    pub address: String,
}

impl MdnsService {
    pub fn is_pairing_service(&self) -> bool {
        normalize_service_type(&self.service_type) == PAIRING_SERVICE_TYPE
    }

    pub fn is_connect_service(&self) -> bool {
        normalize_service_type(&self.service_type) == CONNECT_SERVICE_TYPE
    }
}

impl Adb {
    pub fn resolve(override_path: Option<PathBuf>) -> Result<Self> {
        Ok(Self {
            path: resolve_program("adb", override_path)?,
        })
    }

    pub fn version(&self) -> Result<CommandResult> {
        let output = self.run(["version"])?;
        ensure_success("adb version", output)
    }

    pub fn mdns_check(&self) -> Result<CommandResult> {
        self.run(["mdns", "check"])
    }

    pub fn reset_server(&self) -> Result<()> {
        let _kill_output = self.run(["kill-server"])?;
        thread::sleep(Duration::from_secs(1));

        let start_output = self.run(["start-server"])?;
        ensure_success("adb start-server", start_output)?;

        Ok(())
    }

    pub fn devices(&self) -> Result<Vec<AdbDevice>> {
        let output = ensure_success("adb devices -l", self.run(["devices", "-l"])?)?;
        Ok(parse_devices(&output.stdout))
    }

    pub fn mdns_services(&self) -> Result<Vec<MdnsService>> {
        let output = ensure_success("adb mdns services", self.run(["mdns", "services"])?)?;
        Ok(parse_mdns_services(&output.stdout))
    }

    pub fn pair(&self, endpoint: &str, secret: &str) -> Result<CommandResult> {
        let output = self.run(["pair", endpoint, secret])?;
        let combined = output.combined_output();

        if !output.status.success() {
            bail!("adb pair failed: {}", fallback_message(&combined));
        }

        if output_looks_failed(&combined) {
            bail!("adb pair failed: {}", fallback_message(&combined));
        }

        Ok(output)
    }

    pub fn connect(&self, endpoint: &str) -> Result<CommandResult> {
        let output = self.run(["connect", endpoint])?;
        let combined = output.combined_output();

        if !output.status.success() {
            bail!("adb connect failed: {}", fallback_message(&combined));
        }

        if output_looks_failed(&combined) {
            bail!("adb connect failed: {}", fallback_message(&combined));
        }

        Ok(output)
    }

    pub fn reconnect_offline(&self) -> Result<CommandResult> {
        self.run(["reconnect", "offline"])
    }

    pub fn keepalive(&self, serial: &str) -> Result<()> {
        ensure_success("adb shell true", self.run(["-s", serial, "shell", "true"])?)?;

        Ok(())
    }

    pub fn stay_awake(&self, serial: &str) -> Result<()> {
        ensure_success(
            "adb shell svc power stayon true",
            self.run(["-s", serial, "shell", "svc", "power", "stayon", "true"])?,
        )?;

        Ok(())
    }

    pub fn wifi_status(&self, serial: &str) -> Result<String> {
        let output = self.run(["-s", serial, "shell", "cmd", "wifi", "status"])?;
        let combined = output.combined_output();

        if output.status.success() && !combined.trim().is_empty() {
            return Ok(summarize_wifi_status(&combined));
        }

        let output = ensure_success(
            "adb shell dumpsys wifi",
            self.run(["-s", serial, "shell", "dumpsys", "wifi"])?,
        )?;

        Ok(summarize_wifi_status(&output.combined_output()))
    }

    pub fn dump_ui_hierarchy(&self, serial: &str) -> Result<String> {
        let direct_output =
            self.run(["-s", serial, "exec-out", "uiautomator", "dump", "/dev/tty"])?;
        let direct_text = direct_output.combined_output();

        if direct_output.status.success() && direct_text.contains("<hierarchy") {
            return Ok(direct_text);
        }

        ensure_success(
            "adb shell uiautomator dump",
            self.run([
                "-s",
                serial,
                "shell",
                "uiautomator",
                "dump",
                "/sdcard/airadb-window.xml",
            ])?,
        )?;

        let cat_output = ensure_success(
            "adb exec-out cat /sdcard/airadb-window.xml",
            self.run(["-s", serial, "exec-out", "cat", "/sdcard/airadb-window.xml"])?,
        )?;

        Ok(cat_output.combined_output())
    }

    fn run<I, S>(&self, args: I) -> Result<CommandResult>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = Command::new(&self.path)
            .args(args)
            .output()
            .with_context(|| format!("failed to run {}", self.path.display()))?;

        Ok(CommandResult {
            status: output.status,
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

pub fn connect_service_candidates(
    services: &[MdnsService],
    pairing_address: &str,
    baseline_services: &HashSet<MdnsService>,
) -> Vec<MdnsService> {
    let pairing_host = endpoint_host(pairing_address);
    let connect_services: Vec<_> = services
        .iter()
        .filter(|service| service.is_connect_service())
        .cloned()
        .collect();

    let same_host_new: Vec<_> = connect_services
        .iter()
        .filter(|service| {
            endpoint_host(&service.address) == pairing_host && !baseline_services.contains(service)
        })
        .cloned()
        .collect();

    if !same_host_new.is_empty() {
        return same_host_new;
    }

    let same_host: Vec<_> = connect_services
        .iter()
        .filter(|service| endpoint_host(&service.address) == pairing_host)
        .cloned()
        .collect();

    if !same_host.is_empty() {
        return same_host;
    }

    let new_services: Vec<_> = connect_services
        .iter()
        .filter(|service| !baseline_services.contains(service))
        .cloned()
        .collect();

    if !new_services.is_empty() {
        return new_services;
    }

    connect_services
}

pub fn matching_ready_device(
    devices: &[AdbDevice],
    expected_serial: &str,
    baseline_serials: &HashSet<String>,
) -> Option<AdbDevice> {
    let expected_host = endpoint_host(expected_serial);
    let ready_devices: Vec<_> = devices
        .iter()
        .filter(|device| device.state == DeviceState::Device)
        .cloned()
        .collect();

    if let Some(device) = ready_devices
        .iter()
        .find(|device| device.serial == expected_serial)
        .cloned()
    {
        return Some(device);
    }

    if let Some(device) = ready_devices
        .iter()
        .find(|device| endpoint_host(&device.serial) == expected_host)
        .cloned()
    {
        return Some(device);
    }

    let new_ready_devices: Vec<_> = ready_devices
        .into_iter()
        .filter(|device| !baseline_serials.contains(&device.serial))
        .collect();

    if new_ready_devices.len() == 1 {
        return Some(new_ready_devices[0].clone());
    }

    None
}

pub fn connect_serial_from_output(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let line = line.trim();
        let serial = line
            .strip_prefix("connected to ")
            .or_else(|| line.strip_prefix("already connected to "))?;

        serial.split_whitespace().next().map(ToString::to_string)
    })
}

pub fn endpoint_host(endpoint: &str) -> String {
    let endpoint = endpoint.trim();

    if let Some((host, _port)) = endpoint.rsplit_once(':') {
        return host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string();
    }

    endpoint.to_string()
}

pub fn connect_services(services: &[MdnsService]) -> HashSet<MdnsService> {
    services
        .iter()
        .filter(|service| service.is_connect_service())
        .cloned()
        .collect()
}

pub fn ready_device_serials(devices: &[AdbDevice]) -> HashSet<String> {
    devices
        .iter()
        .filter(|device| device.state == DeviceState::Device)
        .map(|device| device.serial.clone())
        .collect()
}

pub fn parse_devices(output: &str) -> Vec<AdbDevice> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim();

            if line.is_empty() || line.starts_with("List of devices") {
                return None;
            }

            let mut parts = line.split_whitespace();
            let serial = parts.next()?.to_string();
            let state = DeviceState::from_adb(parts.next()?);

            let mut product = None;
            let mut model = None;
            let mut device = None;
            let mut transport_id = None;

            for token in parts {
                if let Some(value) = token.strip_prefix("product:") {
                    product = Some(value.to_string());
                } else if let Some(value) = token.strip_prefix("model:") {
                    model = Some(value.to_string());
                } else if let Some(value) = token.strip_prefix("device:") {
                    device = Some(value.to_string());
                } else if let Some(value) = token.strip_prefix("transport_id:") {
                    transport_id = Some(value.to_string());
                }
            }

            Some(AdbDevice {
                serial,
                state,
                product,
                model,
                device,
                transport_id,
            })
        })
        .collect()
}

pub fn parse_mdns_services(output: &str) -> Vec<MdnsService> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim();

            if line.is_empty() || line.starts_with("List of discovered") {
                return None;
            }

            let mut parts = line.split_whitespace();
            let instance = parts.next()?.to_string();
            let service_type = normalize_service_type(parts.next()?);
            let address = parts.next()?.to_string();

            Some(MdnsService {
                instance,
                service_type,
                address,
            })
        })
        .collect()
}

fn normalize_service_type(service_type: &str) -> String {
    service_type.trim_end_matches('.').to_string()
}

fn ensure_success(command: &str, output: CommandResult) -> Result<CommandResult> {
    if output.status.success() {
        return Ok(output);
    }

    bail!(
        "{command} failed: {}",
        fallback_message(&output.combined_output())
    )
}

fn output_looks_failed(output: &str) -> bool {
    let output = output.to_ascii_lowercase();
    output.contains("failed")
        || output.contains("unable")
        || output.contains("cannot")
        || output.contains("error:")
}

fn fallback_message(output: &str) -> String {
    let output = output.trim();

    if output.is_empty() {
        "command exited without output".to_string()
    } else {
        output.to_string()
    }
}

fn summarize_wifi_status(output: &str) -> String {
    let mut lines = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        let lower = line.to_ascii_lowercase();

        if line.is_empty() {
            continue;
        }

        if lower.contains("ssid")
            || lower.contains("bssid")
            || lower.contains("rssi")
            || lower.contains("frequency")
            || lower.contains("link speed")
            || lower.contains("wifi is")
            || lower.contains("wi-fi is")
            || lower.contains("connected")
        {
            lines.push(line.to_string());
        }

        if lines.len() >= 8 {
            break;
        }
    }

    if lines.is_empty() {
        output.lines().take(6).collect::<Vec<_>>().join(" | ")
    } else {
        lines.join(" | ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_adb_devices() {
        let devices = parse_devices(
            r#"
List of devices attached
R5CT123ABC device product:foo model:Pixel_8 device:bar transport_id:1
192.168.1.50:42131 device product:foo model:Pixel_8_Pro device:bar
emulator-5554 offline
ZY22 unauthorized
"#,
        );

        assert_eq!(devices.len(), 4);
        assert_eq!(devices[0].serial, "R5CT123ABC");
        assert_eq!(devices[0].state, DeviceState::Device);
        assert_eq!(devices[0].model.as_deref(), Some("Pixel_8"));
        assert_eq!(devices[1].serial, "192.168.1.50:42131");
        assert_eq!(devices[2].state, DeviceState::Offline);
        assert_eq!(devices[3].state, DeviceState::Unauthorized);
    }

    #[test]
    fn parses_mdns_services() {
        let services = parse_mdns_services(
            r#"
List of discovered mdns services
studio-AbCdEf1234 _adb-tls-pairing._tcp. 192.168.1.23:37199
adb-XYZ-QXjCrW _adb-tls-connect._tcp 192.168.1.23:40233
ignored _printer._tcp 192.168.1.10:1234
"#,
        );

        assert_eq!(services.len(), 3);
        assert!(services[0].is_pairing_service());
        assert!(services[1].is_connect_service());
        assert_eq!(services[1].address, "192.168.1.23:40233");
    }

    #[test]
    fn extracts_connect_serial() {
        assert_eq!(
            connect_serial_from_output("connected to 192.168.1.23:40233\n").as_deref(),
            Some("192.168.1.23:40233")
        );
        assert_eq!(
            connect_serial_from_output("already connected to 192.168.1.23:40233\n").as_deref(),
            Some("192.168.1.23:40233")
        );
        assert_eq!(connect_serial_from_output("nope"), None);
    }

    #[test]
    fn extracts_endpoint_host() {
        assert_eq!(endpoint_host("192.168.1.23:40233"), "192.168.1.23");
        assert_eq!(endpoint_host("[fe80::1]:40233"), "fe80::1");
    }

    #[test]
    fn connect_candidates_prefer_new_same_host_services() {
        let old_same_host = MdnsService {
            instance: "adb-old".to_string(),
            service_type: CONNECT_SERVICE_TYPE.to_string(),
            address: "192.168.1.23:11111".to_string(),
        };
        let new_same_host = MdnsService {
            instance: "adb-new".to_string(),
            service_type: CONNECT_SERVICE_TYPE.to_string(),
            address: "192.168.1.23:22222".to_string(),
        };
        let new_other_host = MdnsService {
            instance: "adb-other".to_string(),
            service_type: CONNECT_SERVICE_TYPE.to_string(),
            address: "192.168.1.99:33333".to_string(),
        };
        let baseline = HashSet::from([old_same_host.clone()]);

        let candidates = connect_service_candidates(
            &[old_same_host, new_other_host, new_same_host.clone()],
            "192.168.1.23:37199",
            &baseline,
        );

        assert_eq!(candidates, vec![new_same_host]);
    }

    #[test]
    fn matching_ready_device_prefers_expected_serial_then_host() {
        let baseline = HashSet::from(["R5CT123ABC".to_string()]);
        let devices = parse_devices(
            r#"
List of devices attached
R5CT123ABC device product:foo model:Old_Device device:bar transport_id:1
192.168.1.23:40233 device product:foo model:Pixel_8_Pro device:bar
"#,
        );

        let matched = matching_ready_device(&devices, "192.168.1.23:40100", &baseline)
            .expect("expected matching same-host wireless device");

        assert_eq!(matched.serial, "192.168.1.23:40233");
    }

    #[test]
    fn summarizes_wifi_status_lines() {
        let summary = summarize_wifi_status(
            r#"
Wifi is enabled
Random line
SSID: "Lab"
BSSID: 12:34:56:78:90:ab
RSSI: -66
Frequency: 5180
Link speed: 433Mbps
"#,
        );

        assert_eq!(
            summary,
            "Wifi is enabled | SSID: \"Lab\" | BSSID: 12:34:56:78:90:ab | RSSI: -66 | Frequency: 5180 | Link speed: 433Mbps"
        );
    }
}
