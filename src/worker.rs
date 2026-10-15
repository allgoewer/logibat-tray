//! Background worker: decides when to read which device.
//!
//! Each device has its own schedule:
//! - device answers                       -> next read in [`READ_INTERVAL`]
//! - dongle present, device does not answer -> next read in [`RETRY_INTERVAL`]
//!   (the first [`FAST_RETRIES`] tries after a device arrival use [`FAST_RETRY_INTERVAL`])
//! - dongle not present                   -> no timed read, wait for a device arrival

use crate::status::{DeviceKind, DeviceState, DeviceStatus};
use log::info;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

pub const READ_INTERVAL: Duration = Duration::from_secs(15 * 60);
pub const RETRY_INTERVAL: Duration = Duration::from_secs(60);
pub const FAST_RETRY_INTERVAL: Duration = Duration::from_secs(5);
pub const FAST_RETRIES: u8 = 2;
/// A KVM switch makes many arrival events. Wait until they stop.
pub const ARRIVAL_DEBOUNCE: Duration = Duration::from_secs(3);
pub const REMOVAL_DEBOUNCE: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// Read all devices now (menu "Refresh now").
    Refresh,
    /// A HID interface appeared.
    DeviceArrival,
    /// A HID interface disappeared.
    DeviceRemoval,
}

/// Access to the devices. Real implementation: [`crate::devices::hid`].
pub trait Backend {
    /// Update the device list (enumeration only, no device I/O).
    fn refresh(&mut self);
    /// True if the dongle of `kind` is present (after the last `refresh`).
    fn present(&self, kind: DeviceKind) -> bool;
    /// Read the battery of `kind` (after the last `refresh`).
    fn read(&mut self, kind: DeviceKind) -> DeviceState;
}

/// When to read a device next, after a read with result `state`.
pub fn next_read(state: &DeviceState, now: Instant, fast_retries: &mut u8) -> Option<Instant> {
    match state {
        DeviceState::Battery(_) => {
            *fast_retries = 0;
            Some(now + READ_INTERVAL)
        }
        DeviceState::Unreachable(_) | DeviceState::Unknown => {
            if *fast_retries > 0 {
                *fast_retries -= 1;
                Some(now + FAST_RETRY_INTERVAL)
            } else {
                Some(now + RETRY_INTERVAL)
            }
        }
        DeviceState::NotConnected => {
            *fast_retries = 0;
            None
        }
    }
}

struct Slot {
    status: DeviceStatus,
    next: Option<Instant>,
    fast_retries: u8,
}

pub struct Worker<B: Backend> {
    backend: B,
    slots: Vec<Slot>,
    arrival_at: Option<Instant>,
    removal_at: Option<Instant>,
}

impl<B: Backend> Worker<B> {
    /// New worker. All devices are due at `now` (read at startup).
    pub fn new(backend: B, now: Instant) -> Self {
        let slots = DeviceKind::ALL
            .iter()
            .map(|&kind| Slot {
                status: DeviceStatus::new(kind),
                next: Some(now),
                fast_retries: 0,
            })
            .collect();
        Self {
            backend,
            slots,
            arrival_at: None,
            removal_at: None,
        }
    }

    pub fn statuses(&self) -> Vec<DeviceStatus> {
        self.slots.iter().map(|s| s.status.clone()).collect()
    }

    /// Earliest time at which [`Worker::tick`] has work to do.
    pub fn deadline(&self) -> Option<Instant> {
        self.slots
            .iter()
            .filter_map(|s| s.next)
            .chain(self.arrival_at)
            .chain(self.removal_at)
            .min()
    }

    pub fn handle(&mut self, trigger: Trigger, now: Instant) {
        match trigger {
            Trigger::Refresh => {
                for slot in &mut self.slots {
                    slot.next = Some(now);
                }
            }
            Trigger::DeviceArrival => self.arrival_at = Some(now + ARRIVAL_DEBOUNCE),
            Trigger::DeviceRemoval => {
                self.removal_at.get_or_insert(now + REMOVAL_DEBOUNCE);
            }
        }
    }

    /// Do all work that is due. Returns true if a status was updated.
    pub fn tick(&mut self, now: Instant) -> bool {
        let mut updated = false;

        if self.removal_at.is_some_and(|t| t <= now) {
            self.removal_at = None;
            self.backend.refresh();
            for slot in &mut self.slots {
                if !self.backend.present(slot.status.kind)
                    && slot.status.state != DeviceState::NotConnected
                {
                    info!("{:?}: not connected", slot.status.kind);
                    slot.status.state = DeviceState::NotConnected;
                    slot.next = None;
                    slot.fast_retries = 0;
                    updated = true;
                }
            }
        }

        if self.arrival_at.is_some_and(|t| t <= now) {
            self.arrival_at = None;
            for slot in &mut self.slots {
                slot.next = Some(now);
                slot.fast_retries = FAST_RETRIES;
            }
        }

        if self.slots.iter().any(|s| s.next.is_some_and(|t| t <= now)) {
            self.backend.refresh();
            for slot in &mut self.slots {
                if !slot.next.is_some_and(|t| t <= now) {
                    continue;
                }
                let state = self.backend.read(slot.status.kind);
                if state != slot.status.state {
                    match &state {
                        DeviceState::Unreachable(why) => {
                            info!("{:?}: no answer ({why})", slot.status.kind)
                        }
                        other => info!("{:?}: {other:?}", slot.status.kind),
                    }
                }
                slot.next = next_read(&state, now, &mut slot.fast_retries);
                slot.status.state = state;
                updated = true;
            }
        }

        updated
    }
}

/// Worker loop. Runs until all senders of `rx` are dropped.
pub fn run<B: Backend>(backend: B, rx: Receiver<Trigger>, notify: impl Fn(Vec<DeviceStatus>)) {
    let mut worker = Worker::new(backend, Instant::now());
    loop {
        if worker.tick(Instant::now()) {
            notify(worker.statuses());
        }
        let msg = match worker.deadline() {
            Some(d) => rx.recv_timeout(d.saturating_duration_since(Instant::now())),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match msg {
            Ok(trigger) => worker.handle(trigger, Instant::now()),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::{Battery, ChargeState};
    use std::collections::HashMap;

    #[derive(Default)]
    struct FakeBackend {
        states: HashMap<&'static str, DeviceState>,
        reads: Vec<DeviceKind>,
    }

    fn key(kind: DeviceKind) -> &'static str {
        kind.short_name()
    }

    impl FakeBackend {
        fn set(&mut self, kind: DeviceKind, state: DeviceState) {
            self.states.insert(key(kind), state);
        }
    }

    impl Backend for FakeBackend {
        fn refresh(&mut self) {}
        fn present(&self, kind: DeviceKind) -> bool {
            !matches!(
                self.states.get(key(kind)),
                None | Some(DeviceState::NotConnected)
            )
        }
        fn read(&mut self, kind: DeviceKind) -> DeviceState {
            self.reads.push(kind);
            self.states
                .get(key(kind))
                .cloned()
                .unwrap_or(DeviceState::NotConnected)
        }
    }

    fn battery(level: u8) -> DeviceState {
        DeviceState::Battery(Battery {
            level,
            charge: ChargeState::Discharging,
            detail: None,
        })
    }

    fn off() -> DeviceState {
        DeviceState::Unreachable("off".into())
    }

    #[test]
    fn next_read_intervals() {
        let now = Instant::now();
        let mut fast = 0;
        assert_eq!(
            next_read(&battery(50), now, &mut fast),
            Some(now + READ_INTERVAL)
        );
        assert_eq!(
            next_read(&off(), now, &mut fast),
            Some(now + RETRY_INTERVAL)
        );
        assert_eq!(next_read(&DeviceState::NotConnected, now, &mut fast), None);
        let mut fast = 1;
        assert_eq!(
            next_read(&off(), now, &mut fast),
            Some(now + FAST_RETRY_INTERVAL)
        );
        assert_eq!(fast, 0);
    }

    #[test]
    fn per_device_schedule() {
        // Headset answers, mouse is off: headset every 15 min, mouse every 60 s.
        let t0 = Instant::now();
        let mut be = FakeBackend::default();
        be.set(DeviceKind::Headset, battery(80));
        be.set(DeviceKind::Mouse, off());
        let mut w = Worker::new(be, t0);

        assert!(w.tick(t0));
        assert_eq!(w.backend.reads.len(), 2);
        assert_eq!(w.deadline(), Some(t0 + RETRY_INTERVAL));

        let t1 = t0 + RETRY_INTERVAL;
        assert!(w.tick(t1));
        assert_eq!(w.backend.reads[2..], [DeviceKind::Mouse]);
        assert_eq!(w.deadline(), Some(t1 + RETRY_INTERVAL));

        // Mouse comes back.
        w.backend.set(DeviceKind::Mouse, battery(60));
        let t2 = t1 + RETRY_INTERVAL;
        w.tick(t2);
        assert_eq!(w.backend.reads[3..], [DeviceKind::Mouse]);
        assert_eq!(w.deadline(), Some(t0 + READ_INTERVAL));

        let t3 = t0 + READ_INTERVAL;
        w.tick(t3);
        assert_eq!(w.backend.reads[4..], [DeviceKind::Headset]);
    }

    #[test]
    fn not_connected_has_no_timer() {
        let t0 = Instant::now();
        let mut w = Worker::new(FakeBackend::default(), t0);
        w.tick(t0);
        assert_eq!(w.deadline(), None);
        assert!(w
            .statuses()
            .iter()
            .all(|s| s.state == DeviceState::NotConnected));
    }

    #[test]
    fn arrival_is_debounced() {
        let t0 = Instant::now();
        let mut w = Worker::new(FakeBackend::default(), t0);
        w.tick(t0);
        w.backend.set(DeviceKind::Headset, battery(30));
        w.handle(Trigger::DeviceArrival, t0);
        w.handle(Trigger::DeviceArrival, t0 + Duration::from_secs(1));
        let due = t0 + Duration::from_secs(1) + ARRIVAL_DEBOUNCE;
        assert_eq!(w.deadline(), Some(due));
        assert!(!w.tick(due - Duration::from_millis(1)));
        assert!(w.tick(due));
        assert_eq!(w.backend.reads.len(), 4);
        assert_eq!(w.statuses()[0].state, battery(30));
    }

    #[test]
    fn fast_retries_after_arrival() {
        let t0 = Instant::now();
        let mut be = FakeBackend::default();
        be.set(DeviceKind::Mouse, off());
        let mut w = Worker::new(be, t0);
        w.tick(t0);
        w.handle(Trigger::DeviceArrival, t0);
        let t1 = t0 + ARRIVAL_DEBOUNCE;
        w.tick(t1);
        assert_eq!(w.deadline(), Some(t1 + FAST_RETRY_INTERVAL));
        let t2 = t1 + FAST_RETRY_INTERVAL;
        w.tick(t2);
        assert_eq!(w.deadline(), Some(t2 + FAST_RETRY_INTERVAL));
        let t3 = t2 + FAST_RETRY_INTERVAL;
        w.tick(t3);
        assert_eq!(w.deadline(), Some(t3 + RETRY_INTERVAL));
    }

    #[test]
    fn removal_marks_not_connected_without_read() {
        let t0 = Instant::now();
        let mut be = FakeBackend::default();
        be.set(DeviceKind::Headset, battery(80));
        be.set(DeviceKind::Mouse, battery(70));
        let mut w = Worker::new(be, t0);
        w.tick(t0);
        w.backend.set(DeviceKind::Mouse, DeviceState::NotConnected);
        w.handle(Trigger::DeviceRemoval, t0);
        assert!(w.tick(t0 + REMOVAL_DEBOUNCE));
        assert_eq!(w.backend.reads.len(), 2);
        assert_eq!(w.statuses()[1].state, DeviceState::NotConnected);
        assert_eq!(w.statuses()[0].state, battery(80));
        assert_eq!(w.deadline(), Some(t0 + READ_INTERVAL));
    }

    #[test]
    fn refresh_reads_all() {
        let t0 = Instant::now();
        let mut be = FakeBackend::default();
        be.set(DeviceKind::Headset, battery(80));
        let mut w = Worker::new(be, t0);
        w.tick(t0);
        let t1 = t0 + Duration::from_secs(10);
        w.handle(Trigger::Refresh, t1);
        assert!(w.tick(t1));
        assert_eq!(w.backend.reads.len(), 4);
    }
}
