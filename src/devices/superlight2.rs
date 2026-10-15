//! G PRO X SUPERLIGHT 2 mouse (HID++ 2.0 through the LIGHTSPEED receiver).

use super::{hidpp, DeviceError, HidTransport};
use crate::status::{Battery, ChargeState};
use log::debug;

/// The Superlight 2 receiver is 0xC54D. Some units ship with the older 0xC547.
pub const RECEIVER_PIDS: &[u16] = &[0xC54D, 0xC547];

const FEATURE_UNIFIED_BATTERY: u16 = 0x1004;
const FEATURE_BATTERY_VOLTAGE: u16 = 0x1001;

fn charge_state(s: u8) -> ChargeState {
    match s {
        0 => ChargeState::Discharging,
        1 => ChargeState::Charging,
        2 => ChargeState::ChargingSlow,
        3 => ChargeState::Full,
        4 => ChargeState::Error,
        other => ChargeState::Other(other),
    }
}

/// Rough Li-Po discharge curve. Only an estimate.
pub fn voltage_to_percent(mv: u16) -> u8 {
    const CURVE: [(u16, u8); 13] = [
        (4186, 100),
        (4067, 90),
        (3989, 80),
        (3922, 70),
        (3859, 60),
        (3811, 50),
        (3778, 40),
        (3751, 30),
        (3717, 20),
        (3671, 10),
        (3646, 5),
        (3579, 2),
        (3500, 0),
    ];
    if mv >= CURVE[0].0 {
        return 100;
    }
    for w in CURVE.windows(2) {
        let ((v_hi, p_hi), (v_lo, p_lo)) = (w[0], w[1]);
        if (v_lo..v_hi).contains(&mv) {
            let pct = f64::from(p_lo)
                + f64::from(p_hi - p_lo) * f64::from(mv - v_lo) / f64::from(v_hi - v_lo);
            return pct.round() as u8;
        }
    }
    0
}

pub fn read_battery(dev: &mut dyn HidTransport) -> Result<Battery, DeviceError> {
    let idx = hidpp::feature_index(dev, FEATURE_UNIFIED_BATTERY)?;
    if idx != 0 {
        debug!("UNIFIED_BATTERY (0x1004) at feature index {idx}");
        // Function 1 = get_status: [soc %, level flags, charging state, ext power].
        let r = hidpp::call(dev, idx, 1, &[])?;
        if r[0] > 100 {
            return Err(DeviceError::InvalidData(format!(
                "level {} is out of range",
                r[0]
            )));
        }
        return Ok(Battery {
            level: r[0],
            charge: charge_state(r[2]),
            detail: None,
        });
    }

    let idx = hidpp::feature_index(dev, FEATURE_BATTERY_VOLTAGE)?;
    if idx != 0 {
        debug!("BATTERY_VOLTAGE (0x1001) at feature index {idx}");
        // Function 0 = get_battery_voltage: [mV hi, mV lo, flags].
        let r = hidpp::call(dev, idx, 0, &[])?;
        let mv = u16::from_be_bytes([r[0], r[1]]);
        let charge = if r[2] & 0x80 != 0 {
            ChargeState::Charging
        } else {
            ChargeState::Discharging
        };
        return Ok(Battery {
            level: voltage_to_percent(mv),
            charge,
            detail: Some(format!("{mv} mV")),
        });
    }

    Err(DeviceError::NoBatteryFeature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::mock::{packet, MockTransport};

    fn resp(feat: u8, func: u8, params: &[u8]) -> Vec<u8> {
        let mut p = packet(
            20,
            &[(0, 0x11), (1, 0x01), (2, feat), (3, (func << 4) | 0x0A)],
        );
        p[4..4 + params.len()].copy_from_slice(params);
        p
    }

    #[test]
    fn unified_battery() {
        let mut dev =
            MockTransport::new(vec![resp(0, 0, &[0x06]), resp(0x06, 1, &[64, 0x08, 1, 1])]);
        let b = read_battery(&mut dev).unwrap();
        assert_eq!(b.level, 64);
        assert_eq!(b.charge, ChargeState::Charging);
        assert_eq!(b.detail, None);
    }

    #[test]
    fn falls_back_to_voltage() {
        let mut dev = MockTransport::new(vec![
            resp(0, 0, &[0x00]),
            resp(0, 0, &[0x07]),
            resp(0x07, 0, &[0x0F, 0x23, 0x00]), // 3875 mV
        ]);
        let b = read_battery(&mut dev).unwrap();
        assert_eq!(b.level, voltage_to_percent(3875));
        assert_eq!(b.charge, ChargeState::Discharging);
        assert_eq!(b.detail.as_deref(), Some("3875 mV"));
        assert_eq!(&dev.tx[1][4..6], &[0x10, 0x01]);
    }

    #[test]
    fn voltage_charging_flag() {
        let mut dev = MockTransport::new(vec![
            resp(0, 0, &[0x00]),
            resp(0, 0, &[0x07]),
            resp(0x07, 0, &[0x10, 0x00, 0x80]),
        ]);
        assert_eq!(
            read_battery(&mut dev).unwrap().charge,
            ChargeState::Charging
        );
    }

    #[test]
    fn no_battery_feature() {
        let mut dev = MockTransport::new(vec![resp(0, 0, &[0]), resp(0, 0, &[0])]);
        assert_eq!(read_battery(&mut dev), Err(DeviceError::NoBatteryFeature));
    }

    #[test]
    fn voltage_curve() {
        assert_eq!(voltage_to_percent(4200), 100);
        assert_eq!(voltage_to_percent(4186), 100);
        assert_eq!(voltage_to_percent(4067), 90);
        assert_eq!(voltage_to_percent(3811), 50);
        assert_eq!(voltage_to_percent(3500), 0);
        assert_eq!(voltage_to_percent(3400), 0);
        // Python: round(50 + 10 * (3835 - 3811) / (3859 - 3811)) = 55
        assert_eq!(voltage_to_percent(3835), 55);
    }
}
