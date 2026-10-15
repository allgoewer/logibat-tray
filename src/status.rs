//! Device status shared between the worker, the icon renderer and the tooltip.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Headset,
    Mouse,
}

impl DeviceKind {
    pub const ALL: [DeviceKind; 2] = [DeviceKind::Headset, DeviceKind::Mouse];

    /// Short name for the tooltip (Windows limits tray tooltips to 127 chars).
    pub fn short_name(self) -> &'static str {
        match self {
            DeviceKind::Headset => "G522",
            DeviceKind::Mouse => "PRO X SL2",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChargeState {
    Discharging,
    Charging,
    ChargingSlow,
    Full,
    Error,
    Other(u8),
}

impl ChargeState {
    pub fn is_charging(self) -> bool {
        matches!(self, ChargeState::Charging | ChargeState::ChargingSlow)
    }
}

impl fmt::Display for ChargeState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChargeState::Discharging => f.write_str("discharging"),
            ChargeState::Charging => f.write_str("charging"),
            ChargeState::ChargingSlow => f.write_str("charging slowly"),
            ChargeState::Full => f.write_str("full"),
            ChargeState::Error => f.write_str("charge error"),
            ChargeState::Other(s) => write!(f, "state 0x{s:02x}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Battery {
    /// 0..=100
    pub level: u8,
    pub charge: ChargeState,
    /// Optional additional information, for example the voltage.
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceState {
    /// No read-out yet (app start).
    Unknown,
    /// The dongle / receiver is not present (for example the KVM is switched away).
    NotConnected,
    /// The dongle is present, but the device does not answer (off, asleep, error).
    Unreachable(String),
    Battery(Battery),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceStatus {
    pub kind: DeviceKind,
    pub state: DeviceState,
}

impl DeviceStatus {
    pub fn new(kind: DeviceKind) -> Self {
        Self {
            kind,
            state: DeviceState::Unknown,
        }
    }

    /// One tooltip line, for example `G522: 80% (discharging)`.
    pub fn tooltip_line(&self) -> String {
        let name = self.kind.short_name();
        match &self.state {
            DeviceState::Unknown => format!("{name}: reading..."),
            DeviceState::NotConnected => format!("{name}: not connected"),
            DeviceState::Unreachable(_) => format!("{name}: off / no answer"),
            DeviceState::Battery(b) => match &b.detail {
                Some(d) => format!("{name}: {}% ({}, {d})", b.level, b.charge),
                None => format!("{name}: {}% ({})", b.level, b.charge),
            },
        }
    }
}

/// Complete tooltip text. `updated` is a time stamp like `14:05`.
pub fn tooltip(statuses: &[DeviceStatus], updated: Option<&str>) -> String {
    let mut lines: Vec<String> = statuses.iter().map(DeviceStatus::tooltip_line).collect();
    if let Some(t) = updated {
        lines.push(format!("Updated {t}"));
    }
    let mut text = lines.join("\n");
    // Windows tray tooltips hold at most 127 UTF-16 units (all text here is ASCII).
    text.truncate(127);
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltip_lines() {
        let statuses = [
            DeviceStatus {
                kind: DeviceKind::Headset,
                state: DeviceState::Battery(Battery {
                    level: 80,
                    charge: ChargeState::Discharging,
                    detail: None,
                }),
            },
            DeviceStatus {
                kind: DeviceKind::Mouse,
                state: DeviceState::NotConnected,
            },
        ];
        assert_eq!(
            tooltip(&statuses, Some("14:05")),
            "G522: 80% (discharging)\nPRO X SL2: not connected\nUpdated 14:05"
        );
    }

    #[test]
    fn tooltip_is_truncated() {
        let s = DeviceStatus {
            kind: DeviceKind::Mouse,
            state: DeviceState::Battery(Battery {
                level: 5,
                charge: ChargeState::Discharging,
                detail: Some("x".repeat(200)),
            }),
        };
        assert_eq!(tooltip(&[s], None).len(), 127);
    }
}
