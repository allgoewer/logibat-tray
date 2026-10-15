//! Real HID access through hidapi (Windows).

use super::{read_battery, DeviceError, DeviceSpec, HidTransport, VID};
use crate::status::DeviceState;
use hidapi::{HidApi, HidDevice};
use log::debug;
use std::ffi::CString;

struct Transport(HidDevice);

impl HidTransport for Transport {
    fn write(&mut self, data: &[u8]) -> Result<(), DeviceError> {
        self.0
            .write(data)
            .map(|_| ())
            .map_err(|e| DeviceError::Io(e.to_string()))
    }

    fn read(&mut self, buf: &mut [u8], timeout_ms: i32) -> Result<usize, DeviceError> {
        self.0
            .read_timeout(buf, timeout_ms)
            .map_err(|e| DeviceError::Io(e.to_string()))
    }
}

/// Path of the matching HID collection, or `None` if the dongle is not present.
/// Uses the device list of the last `refresh_devices()`.
pub fn find_path(api: &HidApi, spec: &DeviceSpec) -> Option<CString> {
    let candidates: Vec<_> = api
        .device_list()
        .filter(|d| {
            d.vendor_id() == VID
                && spec.pids.contains(&d.product_id())
                && d.usage_page() == spec.usage_page
        })
        .collect();
    candidates
        .iter()
        .find(|d| d.usage() == spec.usage)
        .or(candidates.first())
        .map(|d| d.path().to_owned())
}

/// Refresh the device list. Enumeration does not talk to the devices.
pub fn refresh(api: &mut HidApi) {
    if let Err(e) = api.refresh_devices() {
        log::warn!("HID enumeration failed: {e}");
    }
}

/// Read one device. `api` must be refreshed before.
pub fn read_device(api: &HidApi, spec: &DeviceSpec) -> DeviceState {
    let Some(path) = find_path(api, spec) else {
        return DeviceState::NotConnected;
    };
    debug!("{:?}: open {:?}", spec.kind, path);
    let dev = match api.open_path(&path) {
        Ok(d) => d,
        Err(e) => return DeviceState::Unreachable(format!("open failed: {e}")),
    };
    let mut transport = Transport(dev);
    match read_battery(spec.kind, &mut transport) {
        Ok(b) => DeviceState::Battery(b),
        Err(e) => DeviceState::Unreachable(e.to_string()),
    }
}
