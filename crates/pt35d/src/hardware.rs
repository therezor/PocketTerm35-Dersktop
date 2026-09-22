//! Everything the daemon reads out of sysfs, and the graceful absence of it.
//!
//! On the PocketTerm35 the RP2040 may own the backlight and the battery gauge
//! outright, in which case none of these paths exist. That is a supported
//! configuration: every getter returns `Option` and the bar simply hides the
//! widget. Phase 0's probe (`scripts/pt35-probe.sh`) records which case a given
//! unit is in.

use std::fs;
use std::path::{Path, PathBuf};

/// Discovered once at startup: re-scanning sysfs on every tick is wasted IO.
#[derive(Debug, Default, Clone)]
pub struct Hardware {
    pub battery: Option<PathBuf>,
    pub backlight: Option<PathBuf>,
    pub net: Vec<PathBuf>,
}

impl Hardware {
    pub fn probe() -> Self {
        Self::probe_in(Path::new("/sys"))
    }

    /// Same as [`Hardware::probe`] against an arbitrary sysfs root (tests).
    pub fn probe_in(sysfs: &Path) -> Self {
        let battery = list(&sysfs.join("class/power_supply"))
            .into_iter()
            .find(|p| read_trim(&p.join("type")).as_deref() == Some("Battery"));

        // Pick the backlight with the largest max_brightness — on a Pi there is
        // usually at most one, but a DSI panel can add a second, dummy device.
        let backlight = list(&sysfs.join("class/backlight"))
            .into_iter()
            .max_by_key(|p| read_u32(&p.join("max_brightness")).unwrap_or(0));

        let net = list(&sysfs.join("class/net"))
            .into_iter()
            .filter(|p| p.file_name().and_then(|n| n.to_str()) != Some("lo"))
            .collect();

        Self {
            battery,
            backlight,
            net,
        }
    }

    pub fn battery_percent(&self) -> Option<u8> {
        let base = self.battery.as_ref()?;
        if let Some(capacity) = read_u32(&base.join("capacity")) {
            return Some(capacity.min(100) as u8);
        }
        // No `capacity`: derive it from charge_now/charge_full if present.
        let now = read_u32(&base.join("charge_now"))?;
        let full = read_u32(&base.join("charge_full"))?;
        if full == 0 {
            return None;
        }
        Some(((now as u64 * 100 / full as u64).min(100)) as u8)
    }

    pub fn charging(&self) -> Option<bool> {
        let status = read_trim(&self.battery.as_ref()?.join("status"))?;
        Some(matches!(status.as_str(), "Charging" | "Full"))
    }

    pub fn brightness_percent(&self) -> Option<u8> {
        let base = self.backlight.as_ref()?;
        let max = read_u32(&base.join("max_brightness"))?;
        let now = read_u32(&base.join("brightness"))?;
        if max == 0 {
            return None;
        }
        Some(((now as u64 * 100 / max as u64).min(100)) as u8)
    }

    /// Apply an absolute percentage. Returns `false` when there is no backlight
    /// to talk to (RP2040-owned), so the caller can say so instead of lying.
    pub fn set_brightness_percent(&self, percent: u8) -> bool {
        let Some(base) = self.backlight.as_ref() else {
            return false;
        };
        let Some(max) = read_u32(&base.join("max_brightness")) else {
            return false;
        };
        let value = (max as u64 * percent.min(100) as u64 / 100).max(1);
        fs::write(base.join("brightness"), value.to_string()).is_ok()
    }

    /// A short description of the live connection: `wlan0` state, or the first
    /// interface that is up. `None` when nothing is connected.
    /// Link quality as a percentage, for the bar's signal meter. The kernel
    /// reports it out of 70 in `/proc/net/wireless`.
    pub fn network_signal(&self) -> Option<u8> {
        let text = std::fs::read_to_string("/proc/net/wireless").ok()?;
        let line = text.lines().nth(2)?;
        let quality = line.split_whitespace().nth(2)?.trim_end_matches('.');
        let quality: f32 = quality.parse().ok()?;
        Some(((quality / 70.0) * 100.0).clamp(0.0, 100.0) as u8)
    }

    pub fn network(&self) -> Option<String> {
        for iface in &self.net {
            if read_trim(&iface.join("operstate")).as_deref() == Some("up") {
                let name = iface.file_name()?.to_string_lossy().into_owned();
                return Some(name);
            }
        }
        None
    }
}

/// CPU frequency profiles. The Pi 5 in this chassis browns out under a full
/// load on a small USB-C supply, so `balanced` is the shipped default.
#[derive(Debug, Clone, Copy)]
pub struct CpuLimits {
    pub governor: &'static str,
    pub max_khz: Option<u32>,
}

pub fn cpu_limits(profile: pt35_common::ipc::CpuProfile) -> CpuLimits {
    use pt35_common::ipc::CpuProfile::*;
    match profile {
        Powersave => CpuLimits {
            governor: "powersave",
            max_khz: Some(1_500_000),
        },
        Balanced => CpuLimits {
            governor: "ondemand",
            max_khz: Some(1_800_000),
        },
        Performance => CpuLimits {
            governor: "performance",
            max_khz: None,
        },
    }
}

/// Write a CPU profile to every policy in cpufreq. Best-effort: on a kernel
/// without cpufreq write access this is a no-op and the caller reports it.
pub fn apply_cpu_profile(profile: pt35_common::ipc::CpuProfile) -> bool {
    let limits = cpu_limits(profile);
    let policies = list(Path::new("/sys/devices/system/cpu/cpufreq"));
    if policies.is_empty() {
        return false;
    }
    let mut ok = false;
    for policy in policies {
        ok |= fs::write(policy.join("scaling_governor"), limits.governor).is_ok();
        if let Some(khz) = limits.max_khz {
            let _ = fs::write(policy.join("scaling_max_freq"), khz.to_string());
        } else if let Some(max) = read_u32(&policy.join("cpuinfo_max_freq")) {
            let _ = fs::write(policy.join("scaling_max_freq"), max.to_string());
        }
    }
    ok
}

fn list(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    found.sort();
    found
}

fn read_trim(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn read_u32(path: &Path) -> Option<u32> {
    read_trim(path)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a throwaway sysfs tree; `name` keeps parallel tests apart.
    fn sysfs_with(name: &str, entries: &[(&str, &str)]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("pt35-sysfs-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (path, contents) in entries {
            let full = root.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
        root
    }

    #[test]
    fn reports_nothing_when_the_hardware_is_invisible() {
        let root = sysfs_with("empty", &[("class/net/lo/operstate", "unknown\n")]);
        let hw = Hardware::probe_in(&root);
        assert!(hw.battery.is_none());
        assert!(hw.backlight.is_none());
        assert_eq!(hw.battery_percent(), None);
        assert_eq!(hw.brightness_percent(), None);
        assert_eq!(hw.network(), None);
        assert!(
            !hw.set_brightness_percent(50),
            "must admit it cannot set brightness"
        );
    }

    #[test]
    fn reads_battery_and_backlight_when_present() {
        let root = sysfs_with(
            "full",
            &[
                ("class/power_supply/BAT0/type", "Battery\n"),
                ("class/power_supply/BAT0/capacity", "73\n"),
                ("class/power_supply/BAT0/status", "Charging\n"),
                ("class/backlight/rpi_backlight/max_brightness", "255\n"),
                ("class/backlight/rpi_backlight/brightness", "128\n"),
                ("class/net/wlan0/operstate", "up\n"),
            ],
        );
        let hw = Hardware::probe_in(&root);
        assert_eq!(hw.battery_percent(), Some(73));
        assert_eq!(hw.charging(), Some(true));
        assert_eq!(hw.brightness_percent(), Some(50));
        assert_eq!(hw.network().as_deref(), Some("wlan0"));
    }

    #[test]
    fn derives_capacity_from_charge_counters() {
        let root = sysfs_with(
            "charge",
            &[
                ("class/power_supply/BAT0/type", "Battery\n"),
                ("class/power_supply/BAT0/charge_now", "2500\n"),
                ("class/power_supply/BAT0/charge_full", "5000\n"),
            ],
        );
        assert_eq!(Hardware::probe_in(&root).battery_percent(), Some(50));
    }

    #[test]
    fn ignores_a_usb_charger_that_is_not_a_battery() {
        let root = sysfs_with(
            "usb",
            &[
                ("class/power_supply/usb/type", "USB\n"),
                ("class/power_supply/usb/online", "1\n"),
            ],
        );
        assert!(Hardware::probe_in(&root).battery.is_none());
    }
}
