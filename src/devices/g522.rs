//! G522 LIGHTSPEED headset: vendor-specific 64-byte protocol.

use super::{hexdump, DeviceError, HidTransport};
use crate::status::{Battery, ChargeState};
use log::debug;

/// Wireless (LIGHTSPEED dongle) mode.
pub const PID: u16 = 0x0B18;
pub const USAGE_PAGE: u16 = 0xFFA0;
const PACKET: usize = 64;
const PREFIX: [u8; 2] = [0x50, 0x23];
const READ_TIMEOUT_MS: i32 = 1000;
const MAX_PACKETS: usize = 4;

fn request() -> [u8; PACKET] {
    let mut req = [0u8; PACKET];
    for (i, v) in [
        (0, 0x50),
        (1, 0x23),
        (2, 0x0B),
        (4, 0x03),
        (5, 0x1A),
        (7, 0x03),
        (9, 0x05),
        (10, 0x0A),
    ] {
        req[i] = v;
    }
    req
}

pub fn read_battery(dev: &mut dyn HidTransport) -> Result<Battery, DeviceError> {
    let req = request();
    debug!("G522 TX: {} ...", hexdump(&req[..16]));
    dev.write(&req)?;

    let mut buf = [0u8; PACKET];
    for attempt in 0..MAX_PACKETS {
        let n = dev.read(&mut buf, READ_TIMEOUT_MS)?;
        if n == 0 {
            debug!("G522 RX: timeout");
            return Err(DeviceError::Timeout);
        }
        debug!("G522 RX[{attempt}]: {} ...", hexdump(&buf[..n.min(16)]));
        if n < 14 || buf[..2] != PREFIX {
            debug!("  -> unknown packet, skip");
            continue;
        }
        match buf[2] {
            0x03 => debug!("  -> ACK, skip"),
            0x05 if buf[7] == 0x00 => return Err(DeviceError::Off),
            0x05 => debug!("  -> power event, skip"),
            0x0B if buf[9] == 0x05 => {
                let level = buf[11];
                if level > 100 {
                    return Err(DeviceError::InvalidData(format!(
                        "level {level} is out of range"
                    )));
                }
                let charge = if buf[13] == 0x02 {
                    ChargeState::Charging
                } else {
                    ChargeState::Discharging
                };
                return Ok(Battery {
                    level,
                    charge,
                    detail: None,
                });
            }
            _ => debug!("  -> other packet, skip"),
        }
    }
    Err(DeviceError::InvalidData(
        "no battery packet received".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::mock::{packet, MockTransport};

    fn battery_packet(level: u8, state: u8) -> Vec<u8> {
        packet(
            64,
            &[
                (0, 0x50),
                (1, 0x23),
                (2, 0x0B),
                (9, 0x05),
                (11, level),
                (13, state),
            ],
        )
    }

    #[test]
    fn sends_request() {
        let mut dev = MockTransport::new(vec![battery_packet(50, 0)]);
        read_battery(&mut dev).unwrap();
        assert_eq!(dev.tx.len(), 1);
        assert_eq!(dev.tx[0].len(), 64);
        assert_eq!(
            &dev.tx[0][..11],
            &[0x50, 0x23, 0x0B, 0, 0x03, 0x1A, 0, 0x03, 0, 0x05, 0x0A]
        );
    }

    #[test]
    fn skips_ack_and_reads_battery() {
        let ack = packet(64, &[(0, 0x50), (1, 0x23), (2, 0x03)]);
        let mut dev = MockTransport::new(vec![ack, battery_packet(73, 0x02)]);
        let b = read_battery(&mut dev).unwrap();
        assert_eq!(b.level, 73);
        assert_eq!(b.charge, ChargeState::Charging);
    }

    #[test]
    fn discharging() {
        let mut dev = MockTransport::new(vec![battery_packet(10, 0x01)]);
        assert_eq!(
            read_battery(&mut dev).unwrap().charge,
            ChargeState::Discharging
        );
    }

    #[test]
    fn headset_off() {
        let off = packet(64, &[(0, 0x50), (1, 0x23), (2, 0x05), (7, 0x00)]);
        let mut dev = MockTransport::new(vec![off]);
        assert_eq!(read_battery(&mut dev), Err(DeviceError::Off));
    }

    #[test]
    fn power_event_is_skipped() {
        let on = packet(64, &[(0, 0x50), (1, 0x23), (2, 0x05), (7, 0x01)]);
        let mut dev = MockTransport::new(vec![on, battery_packet(42, 0)]);
        assert_eq!(read_battery(&mut dev).unwrap().level, 42);
    }

    #[test]
    fn out_of_range() {
        let mut dev = MockTransport::new(vec![battery_packet(101, 0)]);
        assert!(matches!(
            read_battery(&mut dev),
            Err(DeviceError::InvalidData(_))
        ));
    }

    #[test]
    fn timeout() {
        let mut dev = MockTransport::new(vec![]);
        assert_eq!(read_battery(&mut dev), Err(DeviceError::Timeout));
    }

    #[test]
    fn gives_up_after_four_packets() {
        let other = packet(64, &[(0, 0x01)]);
        let mut dev = MockTransport::new(vec![other; 5]);
        assert!(matches!(
            read_battery(&mut dev),
            Err(DeviceError::InvalidData(_))
        ));
        assert_eq!(dev.rx.len(), 1);
    }
}
