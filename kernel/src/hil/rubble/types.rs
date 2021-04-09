//! Rubble-specific data structures.
//!
//! These are relatively simple structs that are essentially replicated from the
//! Rubble codebase, but need to be specified here so we do not need to rely on
//! an external dependency for interface reasons.
//!
//! All of these structures are represented identically to their Rubble
//! counterparts. However, most methods are removed: these structures only have
//! most basic methods needed to create and access their internal data, and
//! offer fewer guarantees than their rubble counterparts because of that.
//!
//! These facilitate communication with the interfaces defined in the
//! [`crate::hil::rubble`] module.
use core::convert::{TryFrom, TryInto};

use crate::hil::time::{Frequency, Time};

/// Specifies whether a device address is randomly generated or a LAN MAC address.
///
/// Clone of `rubble::link::device_address::AddressKind`.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum AddressKind {
    /// Publicly registered IEEE 802-2001 LAN MAC address.
    Public,
    /// Randomly generated address.
    Random,
}

/// A Bluetooth device address.
///
/// Clone of `rubble::link::device_address::DeviceAddress`.
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct DeviceAddress {
    pub bytes: [u8; 6],
    pub kind: AddressKind,
}

/// Specifies when the Link Layer's `update` method should be called the next time.
///
/// Clone of `rubble::link::NextUpdate`.
#[derive(Debug, Clone)]
pub enum NextUpdate {
    /// Disable timer and do not call `update`.
    Disable,

    /// Keep the previously configured time.
    Keep,

    /// Call `update` at the given time.
    ///
    /// If time is in the past, this is a bug and the implementation may panic.
    At(Instant),
}

/// A point in time, relative to an unspecfied epoch, specified in microseconds.
///
/// This has microsecond resolution and may wrap around after >1 hour. Apart from the wraparound, it
/// is monotonic.
///
/// Clone of `rubble::timer::Instant`.
#[derive(Debug, Copy, Clone)]
pub struct Instant {
    microseconds: u32,
}

impl Instant {
    /// Creates an `Instant` from raw microseconds since an arbitrary implementation-defined
    /// reference point.
    pub fn from_raw_micros(microseconds: u32) -> Self {
        Instant { microseconds }
    }

    pub fn raw_micros(&self) -> u32 {
        self.microseconds
    }

    pub fn from_alarm_time<A: Time>(raw: u32) -> Self {
        // Frequency::frequency() returns NOW_UNIT / second, and we want
        // microseconds. `now / frequency` gives us seconds, so
        // `now * 1000_000 / frequency` is microseconds

        // multiply before dividing to be as accurate as possible, and use u64 to
        // overflow.
        Instant {
            microseconds: ((raw as u64 * 1000_000u64) / A::Frequency::frequency() as u64)
                .try_into()
                .unwrap(),
        }
    }

    pub fn to_alarm_time<A: Time>(&self, _alarm: &A) -> u32 {
        // instant.raw_micros() is microseconds, and we want NOW_UNIT.
        // Frequency::frequency() returns NOW_UNIT / second, so `raw_micros * frequency` gives us
        // `NOW_UNIT * microseconds / seconds`. `microseconds = 1000_000 seconds`,
        // so `raw_micros * frequency / 1000_000` is NOW_UNIT.
        u32::try_from(self.microseconds as u64 * A::Frequency::frequency() as u64 / 1000_000u64)
            .unwrap()
    }
}
/// A duration with microsecond resolution.
///
/// This can represent a maximum duration of about 1 hour. Overflows will result in a panic, but
/// shouldn't happen since the BLE stack doesn't deal with durations that large.
///
/// Clone of `rubble::timer::Duration`
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Duration(u32);

impl Duration {
    /// Creates a `Duration` from a number of microseconds.
    pub fn from_micros(micros: u32) -> Self {
        Duration(micros)
    }

    /// Creates a `Duration` representing the given number of milliseconds.
    pub fn from_millis(millis: u16) -> Self {
        Duration(u32::from(millis) * 1_000)
    }

    /// Creates a `Duration` representing a number of seconds.
    pub fn from_secs(secs: u16) -> Self {
        Duration(u32::from(secs) * 1_000_000)
    }

    /// Returns the number of whole seconds that fit in `self`.
    pub fn whole_secs(&self) -> u32 {
        self.0 / 1_000_000
    }

    /// Returns the number of whole milliseconds that fit in `self`.
    pub fn whole_millis(&self) -> u32 {
        self.0 / 1_000
    }

    /// Returns the number of microseconds represented by `self`.
    pub fn as_micros(&self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::hil::time::{Freq32KHz, Ticks32};

    struct VAlarm;
    impl Time for VAlarm {
        type Frequency = Freq32KHz;
        type Ticks = Ticks32;

        fn now(&self) -> Ticks32 {
            panic!()
        }
    }

    #[test]
    fn time_roundtrip() {
        for &start in &[0, 3120, 10000, 22500, 9514094] {
            let rubble = Instant::from_alarm_time::<VAlarm>(start);
            let end = rubble.to_alarm_time(&VAlarm);
            assert!((start as i32 - end as i32).abs() < 10);
        }
    }
}
