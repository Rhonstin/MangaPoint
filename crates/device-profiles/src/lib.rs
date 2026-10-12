//! Target e-reader devices and what we know about their file support.
//!
//! This is data, not logic. The built-in devices are listed here; users can
//! add their own (any page size) and those travel in the settings file. When
//! hardware testing settles an open question (see [`Support`]), only the
//! tables here change.

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

/// Smallest and largest page edge, in pixels. The XTC page table stores sizes
/// as 16-bit numbers; the lower bound just keeps a typo from making a device
/// no page can be made for.
pub const MIN_EDGE: u32 = 64;
pub const MAX_EDGE: u32 = 4096;

/// A device's screen and what we know about its file support.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceProfile {
    /// Stable key, kept in settings files and series.
    pub id: String,
    pub name: String,
    /// Width of the page as the reader shows it, in pixels (portrait).
    pub width: u32,
    pub height: u32,
    /// Ships with the app, so it cannot be edited or removed.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub builtin: bool,
    /// What we know about XTC support on this device.
    #[cfg_attr(feature = "serde", serde(skip, default = "unknown_support"))]
    pub support: SupportTable,
}

/// Support per firmware.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SupportTable {
    pub stock: Support,
    pub crosspoint: Support,
}

#[cfg(feature = "serde")]
fn unknown_support() -> SupportTable {
    SupportTable::UNKNOWN
}

impl SupportTable {
    /// A device we have never seen: nothing is known either way.
    pub const UNKNOWN: SupportTable = SupportTable {
        stock: Support::Unverified,
        crosspoint: Support::Unverified,
    };
}

pub const X4_ID: &str = "xteink-x4";
pub const X4_PRO_ID: &str = "xteink-x4-pro";
pub const X3_ID: &str = "xteink-x3";

/// The 4.3" 480x800 panel in the X4. CrossPoint's README lists it and its
/// parser reads both formats; stock support is community-reported.
pub fn x4() -> DeviceProfile {
    DeviceProfile {
        id: X4_ID.into(),
        name: "Xteink X4".into(),
        width: 480,
        height: 800,
        builtin: true,
        support: SupportTable {
            stock: Support::CommunityReported,
            crosspoint: Support::Documented,
        },
    }
}

/// The same panel as the X4, with a touchscreen and front light. Xteink's own
/// spec lists only EPUB/TXT/JPG/BMP, so stock support is unverified.
pub fn x4_pro() -> DeviceProfile {
    DeviceProfile {
        id: X4_PRO_ID.into(),
        name: "Xteink X4 Pro".into(),
        width: 480,
        height: 800,
        builtin: true,
        support: SupportTable {
            stock: Support::Unverified,
            crosspoint: Support::Documented,
        },
    }
}

/// The smaller Xteink X3, 528x792. Community converters target it; neither
/// firmware has been checked against it here.
pub fn x3() -> DeviceProfile {
    DeviceProfile {
        id: X3_ID.into(),
        name: "Xteink X3".into(),
        width: 528,
        height: 792,
        builtin: true,
        support: SupportTable::UNKNOWN,
    }
}

/// The devices that ship with the app.
pub fn builtin() -> Vec<DeviceProfile> {
    vec![x4_pro(), x4(), x3()]
}

/// Why a custom device was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceError {
    EmptyName,
    NameTaken,
    BadSize,
}

impl std::fmt::Display for DeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeviceError::EmptyName => write!(f, "give the device a name"),
            DeviceError::NameTaken => write!(f, "a device with that name already exists"),
            DeviceError::BadSize => write!(
                f,
                "width and height must each be between {MIN_EDGE} and {MAX_EDGE} pixels"
            ),
        }
    }
}

impl std::error::Error for DeviceError {}

impl DeviceProfile {
    /// A new user-made device. Support is unknown until the user has tried it.
    pub fn custom(
        existing: &[DeviceProfile],
        name: &str,
        width: u32,
        height: u32,
    ) -> Result<DeviceProfile, DeviceError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(DeviceError::EmptyName);
        }
        let profile = DeviceProfile {
            id: fresh_id(existing, name),
            name: name.chars().take(40).collect(),
            width,
            height,
            builtin: false,
            support: SupportTable::UNKNOWN,
        };
        profile.validate(&all(existing))?;
        Ok(profile)
    }

    /// Check size and name against the other devices in the list.
    pub fn validate(&self, others: &[DeviceProfile]) -> Result<(), DeviceError> {
        if self.name.trim().is_empty() {
            return Err(DeviceError::EmptyName);
        }
        let ok = |n: u32| (MIN_EDGE..=MAX_EDGE).contains(&n);
        if !ok(self.width) || !ok(self.height) {
            return Err(DeviceError::BadSize);
        }
        if others
            .iter()
            .any(|o| o.id != self.id && o.name.trim().eq_ignore_ascii_case(self.name.trim()))
        {
            return Err(DeviceError::NameTaken);
        }
        Ok(())
    }

    /// Whether this device/firmware pair is known to open `.xtc`/`.xtch` files.
    pub fn xtc_support(&self, firmware: Firmware) -> Support {
        match firmware {
            Firmware::Stock => self.support.stock,
            Firmware::CrossPoint => self.support.crosspoint,
        }
    }

    /// `480 x 800`.
    pub fn size_label(&self) -> String {
        format!("{} \u{d7} {}", self.width, self.height)
    }
}

/// Find a device in `custom` or the built-ins. Built-ins are listed first, so
/// a custom device can never shadow one.
pub fn find(id: &str, custom: &[DeviceProfile]) -> Option<DeviceProfile> {
    builtin()
        .into_iter()
        .chain(custom.iter().cloned().map(|mut c| {
            c.builtin = false;
            c
        }))
        .find(|p| p.id == id)
}

/// Every device: built-ins, then the user's own.
pub fn all(custom: &[DeviceProfile]) -> Vec<DeviceProfile> {
    let mut out = builtin();
    out.extend(custom.iter().cloned().map(|mut c| {
        c.builtin = false;
        c
    }));
    out
}

/// Settings files written before devices became data stored the enum's name.
pub fn legacy_id(old: &str) -> Option<&'static str> {
    match old {
        "XteinkX4" => Some(X4_ID),
        "XteinkX4Pro" => Some(X4_PRO_ID),
        _ => None,
    }
}

fn fresh_id(existing: &[DeviceProfile], name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let base = format!("custom-{}", if slug.is_empty() { "device" } else { &slug });
    let taken = |id: &str| builtin().iter().chain(existing).any(|p| p.id == id);
    if !taken(&base) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|id| !taken(id))
        .expect("an unused id exists")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_ins_are_found_by_id() {
        for p in builtin() {
            assert_eq!(find(&p.id, &[]), Some(p));
        }
        assert_eq!(find("nope", &[]), None);
    }

    #[test]
    fn sizes_match_the_spec_sheets() {
        let size = |id| find(id, &[]).map(|p| (p.width, p.height));
        assert_eq!(size(X4_ID), Some((480, 800)));
        assert_eq!(size(X4_PRO_ID), Some((480, 800)), "same panel as the X4");
        assert_eq!(size(X3_ID), Some((528, 792)));
    }

    #[test]
    fn stock_x4_pro_is_not_claimed_to_work() {
        let p = x4_pro();
        assert_eq!(p.xtc_support(Firmware::Stock), Support::Unverified);
        assert_eq!(p.xtc_support(Firmware::CrossPoint), Support::Documented);
    }

    #[test]
    fn custom_devices_get_unique_ids_and_unknown_support() {
        let a = DeviceProfile::custom(&[], "My Reader", 600, 1024).unwrap();
        assert_eq!(a.id, "custom-my-reader");
        assert!(!a.builtin);
        assert_eq!(a.xtc_support(Firmware::CrossPoint), Support::Unverified);
        let b = DeviceProfile::custom(std::slice::from_ref(&a), "My  Reader!", 600, 1024);
        // Different name text but the same slug: still gets its own id.
        assert_eq!(b.unwrap().id, "custom-my-reader-2");
    }

    #[test]
    fn custom_devices_are_checked() {
        let e = |r: Result<DeviceProfile, DeviceError>| r.unwrap_err();
        assert_eq!(
            e(DeviceProfile::custom(&[], "  ", 480, 800)),
            DeviceError::EmptyName
        );
        assert_eq!(
            e(DeviceProfile::custom(&[], "A", 10, 800)),
            DeviceError::BadSize
        );
        assert_eq!(
            e(DeviceProfile::custom(&[], "A", 480, 9000)),
            DeviceError::BadSize
        );
        assert_eq!(
            e(DeviceProfile::custom(&[], "xteink x4", 480, 800)),
            DeviceError::NameTaken,
            "a custom device cannot reuse a built-in's name"
        );
    }

    #[test]
    fn a_custom_device_cannot_shadow_a_built_in() {
        let mut fake = x4();
        fake.name = "Sneaky".into();
        fake.width = 100;
        let found = find(X4_ID, &[fake]).unwrap();
        assert_eq!(found.width, 480);
        assert!(found.builtin);
    }

    #[test]
    fn old_settings_names_map_to_ids() {
        assert_eq!(legacy_id("XteinkX4Pro"), Some(X4_PRO_ID));
        assert_eq!(legacy_id("XteinkX4"), Some(X4_ID));
        assert_eq!(legacy_id("other"), None);
    }
}
