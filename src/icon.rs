//! Tray icon: two small battery bars. Left = headset, right = mouse.
//!
//! Fill height = battery level. Fill color: green >= 50 %, amber 20..49 %,
//! red < 20 %, blue while charging. A device without a battery value is drawn
//! as a gray, empty outline.

use crate::status::{DeviceState, DeviceStatus};

pub const SIZE: usize = 32;

type Rgba = [u8; 4];

const GREEN: Rgba = [0x3c, 0xc8, 0x50, 0xff];
const AMBER: Rgba = [0xf0, 0xb0, 0x20, 0xff];
const RED: Rgba = [0xe8, 0x38, 0x30, 0xff];
const BLUE: Rgba = [0x38, 0x98, 0xf0, 0xff];
const GRAY: Rgba = [0x80, 0x80, 0x80, 0xff];
const OUTLINE_DARK_TASKBAR: Rgba = [0xe8, 0xe8, 0xe8, 0xff];
const OUTLINE_LIGHT_TASKBAR: Rgba = [0x30, 0x30, 0x30, 0xff];

// Geometry of one bar (x relative to the bar origin).
const BAR_W: usize = 14;
const BAR_X: [usize; 2] = [1, 17];
const CAP_W: usize = 6;
const CAP_TOP: usize = 1;
const BODY_TOP: usize = 5;
const BODY_BOTTOM: usize = SIZE; // exclusive
const LINE: usize = 2;

pub fn level_color(level: u8, charging: bool) -> Rgba {
    if charging {
        BLUE
    } else if level >= 50 {
        GREEN
    } else if level >= 20 {
        AMBER
    } else {
        RED
    }
}

struct Canvas(Vec<u8>);

impl Canvas {
    fn new() -> Self {
        Canvas(vec![0; SIZE * SIZE * 4])
    }

    fn rect(&mut self, x0: usize, y0: usize, x1: usize, y1: usize, c: Rgba) {
        for y in y0..y1.min(SIZE) {
            for x in x0..x1.min(SIZE) {
                let i = (y * SIZE + x) * 4;
                self.0[i..i + 4].copy_from_slice(&c);
            }
        }
    }
}

fn draw_bar(cv: &mut Canvas, x: usize, status: &DeviceStatus, outline: Rgba) {
    let (line_color, fill) = match &status.state {
        DeviceState::Battery(b) => (
            outline,
            Some((
                b.level.min(100),
                level_color(b.level, b.charge.is_charging()),
            )),
        ),
        _ => (GRAY, None),
    };

    // Cap.
    let cap_x = x + (BAR_W - CAP_W) / 2;
    cv.rect(cap_x, CAP_TOP, cap_x + CAP_W, BODY_TOP, line_color);
    // Body outline.
    cv.rect(x, BODY_TOP, x + BAR_W, BODY_TOP + LINE, line_color);
    cv.rect(x, BODY_BOTTOM - LINE, x + BAR_W, BODY_BOTTOM, line_color);
    cv.rect(x, BODY_TOP, x + LINE, BODY_BOTTOM, line_color);
    cv.rect(
        x + BAR_W - LINE,
        BODY_TOP,
        x + BAR_W,
        BODY_BOTTOM,
        line_color,
    );

    if let Some((level, color)) = fill {
        // One pixel gap between outline and fill.
        let inner_top = BODY_TOP + LINE + 1;
        let inner_bottom = BODY_BOTTOM - LINE - 1;
        let inner_h = inner_bottom - inner_top;
        let mut h = (usize::from(level) * inner_h).div_ceil(100);
        if level > 0 {
            h = h.max(1);
        }
        cv.rect(
            x + LINE + 1,
            inner_bottom - h,
            x + BAR_W - LINE - 1,
            inner_bottom,
            color,
        );
    }
}

/// Render the icon as 32x32 RGBA. `statuses[0]` is drawn left, `statuses[1]` right.
pub fn render(statuses: &[DeviceStatus], light_taskbar: bool) -> Vec<u8> {
    let outline = if light_taskbar {
        OUTLINE_LIGHT_TASKBAR
    } else {
        OUTLINE_DARK_TASKBAR
    };
    let mut cv = Canvas::new();
    for (status, &x) in statuses.iter().zip(BAR_X.iter()) {
        draw_bar(&mut cv, x, status, outline);
    }
    cv.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::{Battery, ChargeState, DeviceKind};

    fn pixel(img: &[u8], x: usize, y: usize) -> Rgba {
        let i = (y * SIZE + x) * 4;
        img[i..i + 4].try_into().unwrap()
    }

    fn with_battery(kind: DeviceKind, level: u8, charge: ChargeState) -> DeviceStatus {
        DeviceStatus {
            kind,
            state: DeviceState::Battery(Battery {
                level,
                charge,
                detail: None,
            }),
        }
    }

    #[test]
    fn colors() {
        assert_eq!(level_color(100, false), GREEN);
        assert_eq!(level_color(50, false), GREEN);
        assert_eq!(level_color(49, false), AMBER);
        assert_eq!(level_color(20, false), AMBER);
        assert_eq!(level_color(19, false), RED);
        assert_eq!(level_color(5, true), BLUE);
    }

    #[test]
    fn size_and_fill() {
        let img = render(
            &[
                with_battery(DeviceKind::Headset, 100, ChargeState::Discharging),
                with_battery(DeviceKind::Mouse, 10, ChargeState::Discharging),
            ],
            false,
        );
        assert_eq!(img.len(), SIZE * SIZE * 4);
        let cx_left = BAR_X[0] + BAR_W / 2;
        let cx_right = BAR_X[1] + BAR_W / 2;
        // Full bar: fill at top of the inner area.
        assert_eq!(pixel(&img, cx_left, BODY_TOP + LINE + 1), GREEN);
        // 10 %: fill only at the bottom.
        assert_eq!(pixel(&img, cx_right, BODY_BOTTOM - LINE - 2), RED);
        assert_eq!(pixel(&img, cx_right, BODY_TOP + LINE + 1), [0, 0, 0, 0]);
        // Outline.
        assert_eq!(pixel(&img, BAR_X[0], 20), OUTLINE_DARK_TASKBAR);
    }

    #[test]
    fn not_connected_is_gray_and_empty() {
        let img = render(
            &[
                DeviceStatus::new(DeviceKind::Headset),
                DeviceStatus {
                    kind: DeviceKind::Mouse,
                    state: DeviceState::NotConnected,
                },
            ],
            true,
        );
        for &x in &BAR_X {
            assert_eq!(pixel(&img, x, 20), GRAY);
            assert_eq!(pixel(&img, x + BAR_W / 2, 20), [0, 0, 0, 0]);
        }
    }

    /// Write all icon variants as raw RGBA files to `$ICON_PREVIEW_DIR`.
    /// Run: `ICON_PREVIEW_DIR=out cargo test dump_preview -- --ignored`
    #[test]
    #[ignore]
    fn dump_preview() {
        let dir = std::env::var("ICON_PREVIEW_DIR").expect("ICON_PREVIEW_DIR");
        std::fs::create_dir_all(&dir).unwrap();
        let states = |kind| {
            vec![
                (
                    "none",
                    DeviceStatus {
                        kind,
                        state: DeviceState::NotConnected,
                    },
                ),
                ("100", with_battery(kind, 100, ChargeState::Discharging)),
                ("60", with_battery(kind, 60, ChargeState::Discharging)),
                ("35", with_battery(kind, 35, ChargeState::Discharging)),
                ("10", with_battery(kind, 10, ChargeState::Discharging)),
                ("chg40", with_battery(kind, 40, ChargeState::Charging)),
            ]
        };
        for (hn, h) in states(DeviceKind::Headset) {
            for (mn, m) in states(DeviceKind::Mouse) {
                for (theme, light) in [("dark", false), ("light", true)] {
                    let img = render(&[h.clone(), m.clone()], light);
                    std::fs::write(format!("{dir}/{theme}_{hn}_{mn}.rgba"), img).unwrap();
                }
            }
        }
    }
}
