//! Battery read-out for the supported Logitech devices.
//!
//! The protocol code only talks to the [`HidTransport`] trait, so it can be
//! tested without hardware. The real HID access is in [`hid`] (Windows only).

pub mod g522;
#[cfg(windows)]
pub mod hid;
pub mod hidpp;
pub mod superlight2;

use crate::status::{Battery, DeviceKind};

/// Logitech USB vendor ID.
pub const VID: u16 = 0x046D;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DeviceError {
    #[error("I/O error: {0}")]
    Io(String),
    #[error("device is off")]
    Off,
    #[error("no answer (timeout)")]
    Timeout,
    #[error("receiver error 0x{0:02x} (device off or not paired?)")]
    Receiver(u8),
    #[error("device error 0x{0:02x}")]
    Device(u8),
    #[error("device supports no known battery feature")]
    NoBatteryFeature,
    #[error("invalid data: {0}")]
    InvalidData(String),
}

/// Minimal HID interface used by the protocol code.
pub trait HidTransport {
    /// Write one output report. The first byte is the report ID.
    fn write(&mut self, data: &[u8]) -> Result<(), DeviceError>;
    /// Read one input report. Returns the number of bytes, 0 on timeout.
    fn read(&mut self, buf: &mut [u8], timeout_ms: i32) -> Result<usize, DeviceError>;
}

/// Where to find a device: dongle PIDs and the HID collection to use.
#[derive(Debug, Clone, Copy)]
pub struct DeviceSpec {
    pub kind: DeviceKind,
    pub pids: &'static [u16],
    pub usage_page: u16,
    /// Preferred usage. Other usages on the same usage page are a fallback.
    pub usage: u16,
}

pub const SPECS: [DeviceSpec; 2] = [
    DeviceSpec {
        kind: DeviceKind::Headset,
        pids: &[g522::PID],
        usage_page: g522::USAGE_PAGE,
        usage: 0x0001,
    },
    DeviceSpec {
        kind: DeviceKind::Mouse,
        pids: superlight2::RECEIVER_PIDS,
        usage_page: hidpp::USAGE_PAGE,
        // 0x0002 = HID++ long-report collection (needed on Windows).
        usage: 0x0002,
    },
];

pub fn spec(kind: DeviceKind) -> &'static DeviceSpec {
    SPECS
        .iter()
        .find(|s| s.kind == kind)
        .expect("spec for each kind")
}

/// Read the battery of `kind` through an open transport.
pub fn read_battery(kind: DeviceKind, dev: &mut dyn HidTransport) -> Result<Battery, DeviceError> {
    match kind {
        DeviceKind::Headset => g522::read_battery(dev),
        DeviceKind::Mouse => superlight2::read_battery(dev),
    }
}

pub fn hexdump(buf: &[u8]) -> String {
    buf.iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
pub mod mock {
    use super::*;
    use std::collections::VecDeque;

    /// Transport that returns scripted packets. An empty packet means timeout.
    #[derive(Default)]
    pub struct MockTransport {
        pub rx: VecDeque<Vec<u8>>,
        pub tx: Vec<Vec<u8>>,
    }

    impl MockTransport {
        pub fn new(rx: Vec<Vec<u8>>) -> Self {
            Self {
                rx: rx.into(),
                tx: Vec::new(),
            }
        }
    }

    impl HidTransport for MockTransport {
        fn write(&mut self, data: &[u8]) -> Result<(), DeviceError> {
            self.tx.push(data.to_vec());
            Ok(())
        }

        fn read(&mut self, buf: &mut [u8], _timeout_ms: i32) -> Result<usize, DeviceError> {
            match self.rx.pop_front() {
                Some(p) => {
                    let n = p.len().min(buf.len());
                    buf[..n].copy_from_slice(&p[..n]);
                    Ok(n)
                }
                None => Ok(0),
            }
        }
    }

    /// Build a packet of `len` bytes with the given bytes set.
    pub fn packet(len: usize, bytes: &[(usize, u8)]) -> Vec<u8> {
        let mut p = vec![0u8; len];
        for &(i, v) in bytes {
            p[i] = v;
        }
        p
    }
}
