//! Target e-reader devices and what we know about their file support.
//!
//! This is data, not logic: when hardware testing settles an open question
//! (see [`Support`]), only the tables here change.

/// A supported e-reader model.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Device {
    XteinkX4,
    XteinkX4Pro,
}

/// Firmware running on the device.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Firmware {
    /// The firmware the device ships with.
    Stock,
    /// The open-source CrossPoint Reader firmware.
    CrossPoint,
}

/// How well we know that a device/firmware combination opens XTC/XTCH files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Support {
    /// Stated in the firmware's own documentation or source.
    Documented,
    /// Widely reported by the community, not documented by the vendor.
    CommunityReported,
    /// Nothing found either way; needs a hardware test.
    Unverified,
}

/// Static description of a device's screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceProfile {
    pub device: Device,
    pub id: &'static str,
    pub name: &'static str,
    pub width: u32,
    pub height: u32,
}

pub const X4: DeviceProfile = DeviceProfile {
    device: Device::XteinkX4,
    id: "xteink-x4",
    name: "Xteink X4",
    width: 480,
    height: 800,
};

pub const X4_PRO: DeviceProfile = DeviceProfile {
    device: Device::XteinkX4Pro,
    id: "xteink-x4-pro",
    name: "Xteink X4 Pro",
    width: 480,
    height: 800,
};

/// All known devices.
pub fn all() -> &'static [DeviceProfile] {
    &[X4, X4_PRO]
}

/// Look a device up by its stable id (for example from a settings file).
pub fn by_id(id: &str) -> Option<DeviceProfile> {
    all().iter().copied().find(|p| p.id == id)
}

impl Device {
    pub fn profile(self) -> DeviceProfile {
        match self {
            Device::XteinkX4 => X4,
            Device::XteinkX4Pro => X4_PRO,
        }
    }

    /// Whether this device/firmware pair is known to open `.xtc`/`.xtch` files.
    ///
    /// CrossPoint's README lists both devices and its parser reads both
    /// formats. Stock X4 support is community-reported; stock X4 Pro is
    /// unverified (Xteink's X4 Pro spec lists only EPUB/TXT/JPG/BMP).
    pub fn xtc_support(self, firmware: Firmware) -> Support {
        match (self, firmware) {
            (_, Firmware::CrossPoint) => Support::Documented,
            (Device::XteinkX4, Firmware::Stock) => Support::CommunityReported,
            (Device::XteinkX4Pro, Firmware::Stock) => Support::Unverified,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_round_trips() {
        for p in all() {
            assert_eq!(by_id(p.id), Some(*p));
            assert_eq!(p.device.profile(), *p);
        }
        assert_eq!(by_id("nope"), None);
    }

    #[test]
    fn all_current_devices_share_one_page_size() {
        assert!(all().iter().all(|p| (p.width, p.height) == (480, 800)));
    }

    #[test]
    fn stock_x4_pro_is_not_claimed_to_work() {
        assert_eq!(
            Device::XteinkX4Pro.xtc_support(Firmware::Stock),
            Support::Unverified
        );
        assert_eq!(
            Device::XteinkX4Pro.xtc_support(Firmware::CrossPoint),
            Support::Documented
        );
    }
}
