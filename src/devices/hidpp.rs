//! Minimal HID++ 2.0 client (long reports through a LIGHTSPEED receiver).

use super::{hexdump, DeviceError, HidTransport};
use log::debug;
use std::time::{Duration, Instant};

pub const USAGE_PAGE: u16 = 0xFF00;
const SHORT: u8 = 0x10;
const LONG: u8 = 0x11;
const LONG_LEN: usize = 20;
/// First paired device on the receiver.
pub const DEV_INDEX: u8 = 0x01;
/// Any value 0x1..0xF. Marks our own requests.
const SW_ID: u8 = 0x0A;
const CALL_TIMEOUT: Duration = Duration::from_secs(2);
const READ_TIMEOUT_MS: i32 = 500;

/// Response parameters (bytes 4..20 of the long report).
pub type Params = [u8; LONG_LEN - 4];

/// Send one HID++ 2.0 long request and return the response parameters.
pub fn call(
    dev: &mut dyn HidTransport,
    feat_index: u8,
    function: u8,
    params: &[u8],
) -> Result<Params, DeviceError> {
    call_with_timeout(dev, feat_index, function, params, CALL_TIMEOUT)
}

pub fn call_with_timeout(
    dev: &mut dyn HidTransport,
    feat_index: u8,
    function: u8,
    params: &[u8],
    timeout: Duration,
) -> Result<Params, DeviceError> {
    let mut req = [0u8; LONG_LEN];
    req[0] = LONG;
    req[1] = DEV_INDEX;
    req[2] = feat_index;
    req[3] = ((function & 0x0F) << 4) | SW_ID;
    req[4..4 + params.len()].copy_from_slice(params);
    debug!("HID++ TX: {}", hexdump(&req));
    dev.write(&req)?;

    let deadline = Instant::now() + timeout;
    let mut buf = [0u8; LONG_LEN];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(DeviceError::Timeout);
        }
        let wait = (left.as_millis() as i32).clamp(1, READ_TIMEOUT_MS);
        let n = dev.read(&mut buf, wait)?;
        if n == 0 {
            continue;
        }
        debug!("HID++ RX: {}", hexdump(&buf[..n]));
        if n < 7 || !matches!(buf[0], SHORT | LONG) || buf[1] != DEV_INDEX {
            continue;
        }
        // HID++ 1.0 error from the receiver (for example: mouse not connected).
        if buf[2] == 0x8F {
            return Err(DeviceError::Receiver(buf[5]));
        }
        // HID++ 2.0 error.
        if buf[2] == 0xFF && buf[3] == feat_index {
            return Err(DeviceError::Device(buf[5]));
        }
        if buf[2] == feat_index && buf[3] == req[3] {
            let mut out = [0u8; LONG_LEN - 4];
            out.copy_from_slice(&buf[4..LONG_LEN]);
            return Ok(out);
        }
        // Other packets are notifications. Ignore them.
    }
}

/// Ask IRoot (feature index 0) for the index of a feature. 0 = not supported.
pub fn feature_index(dev: &mut dyn HidTransport, feature_id: u16) -> Result<u8, DeviceError> {
    let r = call(dev, 0x00, 0, &feature_id.to_be_bytes())?;
    Ok(r[0])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::mock::{packet, MockTransport};

    #[test]
    fn request_layout() {
        let resp = packet(20, &[(0, 0x11), (1, 0x01), (2, 0x00), (3, 0x0A), (4, 0x07)]);
        let mut dev = MockTransport::new(vec![resp]);
        assert_eq!(feature_index(&mut dev, 0x1004).unwrap(), 7);
        let mut expected = vec![0u8; 20];
        expected[..6].copy_from_slice(&[0x11, 0x01, 0x00, 0x0A, 0x10, 0x04]);
        assert_eq!(dev.tx[0], expected);
    }

    #[test]
    fn function_number_in_high_nibble() {
        let resp = packet(20, &[(0, 0x11), (1, 0x01), (2, 0x05), (3, 0x1A), (4, 55)]);
        let mut dev = MockTransport::new(vec![resp]);
        let r = call(&mut dev, 0x05, 1, &[]).unwrap();
        assert_eq!(r[0], 55);
        assert_eq!(dev.tx[0][3], 0x1A);
    }

    #[test]
    fn skips_notifications() {
        let notif = packet(20, &[(0, 0x11), (1, 0x01), (2, 0x04), (3, 0x00)]);
        let other_dev = packet(20, &[(0, 0x11), (1, 0x02), (2, 0x05), (3, 0x1A)]);
        let resp = packet(20, &[(0, 0x11), (1, 0x01), (2, 0x05), (3, 0x1A), (4, 9)]);
        let mut dev = MockTransport::new(vec![notif, other_dev, resp]);
        assert_eq!(call(&mut dev, 0x05, 1, &[]).unwrap()[0], 9);
    }

    #[test]
    fn receiver_error() {
        let err = packet(7, &[(0, 0x10), (1, 0x01), (2, 0x8F), (3, 0x00), (5, 0x09)]);
        let mut dev = MockTransport::new(vec![err]);
        assert_eq!(
            call(&mut dev, 0x00, 0, &[]),
            Err(DeviceError::Receiver(0x09))
        );
    }

    #[test]
    fn device_error() {
        let err = packet(20, &[(0, 0x11), (1, 0x01), (2, 0xFF), (3, 0x05), (5, 0x02)]);
        let mut dev = MockTransport::new(vec![err]);
        assert_eq!(call(&mut dev, 0x05, 0, &[]), Err(DeviceError::Device(0x02)));
    }

    #[test]
    fn timeout() {
        let mut dev = MockTransport::new(vec![]);
        assert_eq!(
            call_with_timeout(&mut dev, 0x05, 0, &[], Duration::from_millis(20)),
            Err(DeviceError::Timeout)
        );
    }
}
