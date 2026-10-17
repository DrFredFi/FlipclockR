//! sRGB colors stored as `#rrggbb` in the config.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub [u8; 3]);

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self([r, g, b])
    }

    pub fn parse_hex(s: &str) -> Option<Self> {
        let hex = s.strip_prefix('#')?;
        if hex.len() != 6 || !hex.is_ascii() {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        Some(Self([byte(0)?, byte(2)?, byte(4)?]))
    }

    pub fn to_hex(self) -> String {
        let [r, g, b] = self.0;
        format!("#{r:02x}{g:02x}{b:02x}")
    }

    /// Linear-light components, for shading.
    pub fn to_linear(self) -> [f32; 3] {
        self.0.map(|c| srgb_to_linear(f32::from(c) / 255.0))
    }
}

pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

impl Serialize for Rgb {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Rgb::parse_hex(&s).ok_or_else(|| serde::de::Error::custom(format!("invalid color {s:?}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_hex() {
        let c = Rgb::parse_hex("#1a2B3c").unwrap();
        assert_eq!(c, Rgb::new(0x1a, 0x2b, 0x3c));
        assert_eq!(c.to_hex(), "#1a2b3c");
    }

    #[test]
    fn rejects_malformed_hex() {
        assert!(Rgb::parse_hex("1a2b3c").is_none());
        assert!(Rgb::parse_hex("#12345").is_none());
        assert!(Rgb::parse_hex("#gg0000").is_none());
    }

    #[test]
    fn converts_to_linear() {
        assert_eq!(Rgb::new(0, 0, 0).to_linear(), [0.0; 3]);
        assert!((Rgb::new(255, 255, 255).to_linear()[0] - 1.0).abs() < 1e-6);
    }
}
