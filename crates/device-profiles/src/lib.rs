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
    /// Pixels per inch of the screen: what turns its size in inches into the
    /// size in pixels the pages are made at.
    #[cfg_attr(feature = "serde", serde(default = "default_ppi"))]
    pub ppi: u32,
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

/// The density of the Xteink screens, used until a reader says otherwise.
pub const DEFAULT_PPI: u32 = 220;

#[cfg(feature = "serde")]
fn default_ppi() -> u32 {
    DEFAULT_PPI
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
        ppi: 220,
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
        ppi: 220,
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
        ppi: 256,
        builtin: true,
        support: SupportTable::UNKNOWN,
    }
}

/// The readers the app knows: what you can pick from when adding one. Adding
/// one copies it, so it is then yours to change or remove.
pub fn builtin() -> Vec<DeviceProfile> {
    vec![x4_pro(), x4(), x3()]
}

/// A known reader by id, as the app lists it.
pub fn known(id: &str) -> Option<DeviceProfile> {
    builtin().into_iter().find(|p| p.id == id)
}

/// How well `name` answers what was typed: `None` if it does not, otherwise
/// smaller is better. Case, spaces and punctuation do not matter, and every
/// word typed has to be in the name.
pub fn match_rank(query: &str, name: &str) -> Option<usize> {
    let squash = |s: &str| -> String {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    };
    let whole = squash(name);
    let q = squash(query);
    if q.is_empty() {
        return Some(0);
    }
    if let Some(at) = whole.find(&q) {
        return Some(at);
    }
    let words: Vec<String> = query.split_whitespace().map(squash).filter(|w| !w.is_empty()).collect();
    if !words.is_empty() && words.iter().all(|w| whole.contains(w.as_str())) {
        return Some(1000);
    }
    None
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
            ppi: DEFAULT_PPI,
            builtin: false,
            support: SupportTable::UNKNOWN,
        };
        profile.validate(existing)?;
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

    /// The screen's width and height in inches.
    pub fn size_inches(&self) -> (f64, f64) {
        let ppi = f64::from(self.ppi.max(1));
        (f64::from(self.width) / ppi, f64::from(self.height) / ppi)
    }

    /// `2.18 x 3.64 in`.
    pub fn inches_label(&self) -> String {
        let (w, h) = self.size_inches();
        format!("{w:.2} \u{d7} {h:.2} in")
    }

    /// `480 x 800`.
    pub fn size_label(&self) -> String {
        format!("{} \u{d7} {}", self.width, self.height)
    }
}

/// Find one of the user's readers.
pub fn find(id: &str, custom: &[DeviceProfile]) -> Option<DeviceProfile> {
    all(custom).into_iter().find(|p| p.id == id)
}

/// Every reader the user has.
pub fn all(custom: &[DeviceProfile]) -> Vec<DeviceProfile> {
    custom
        .iter()
        .cloned()
        .map(|mut c| {
            c.builtin = false;
            c
        })
        .collect()
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
    fn known_readers_are_found_by_id_and_nobody_has_any_to_begin_with() {
        for p in builtin() {
            assert_eq!(known(&p.id), Some(p));
        }
        assert_eq!(known("nope"), None);
        assert_eq!(find(X4_ID, &[]), None);
        let mine = vec![x4()];
        assert_eq!(find(X4_ID, &mine).map(|p| p.builtin), Some(false));
    }

    #[test]
    fn typing_finds_the_closest_reader() {
        assert_eq!(match_rank("x4", "Xteink X4 Pro"), Some(6));
        assert!(match_rank("x4 pro", "Xteink X4 Pro").is_some());
        assert_eq!(match_rank("xteink", "Xteink X3"), Some(0));
        assert_eq!(match_rank("PRO x4", "Xteink X4 Pro"), Some(1000));
        assert_eq!(match_rank("kobo", "Xteink X4"), None);
        assert_eq!(match_rank("", "Anything"), Some(0));
    }

    #[test]
    fn screens_are_described_in_inches() {
        let (w, h) = x4_pro().size_inches();
        assert_eq!((format!("{w:.2}"), format!("{h:.2}")), ("2.18".into(), "3.64".into()));
        assert_eq!(x4().inches_label(), "2.18 \u{d7} 3.64 in");
        // The X3's 528x792 at 256 ppi is the 3.7 inch screen it is sold with.
        let (w, h) = x3().size_inches();
        assert!(((w * w + h * h).sqrt() - 3.7).abs() < 0.05);
    }

    #[test]
    fn sizes_match_the_spec_sheets() {
        let size = |id| known(id).map(|p| (p.width, p.height));
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
            e(DeviceProfile::custom(&[x4()], "xteink x4", 480, 800)),
            DeviceError::NameTaken,
            "two readers cannot share a name"
        );
    }

    #[test]
    fn old_settings_names_map_to_ids() {
        assert_eq!(legacy_id("XteinkX4Pro"), Some(X4_PRO_ID));
        assert_eq!(legacy_id("XteinkX4"), Some(X4_ID));
        assert_eq!(legacy_id("other"), None);
    }
}
