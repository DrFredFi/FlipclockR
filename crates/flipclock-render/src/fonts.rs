//! Installed system fonts: family listing, weight listing and loading.

use std::collections::BTreeSet;
use std::sync::Arc;

use fontdb::{Database, Family, Query, Stretch, Style, Weight};

/// Font bytes plus the face index inside them, ready for swash.
#[derive(Clone)]
pub struct LoadedFont {
    pub data: Arc<Vec<u8>>,
    pub index: usize,
    /// Requested weight, applied as the `wght` axis on variable fonts.
    pub weight: u16,
    /// Stable identity, used to detect changes.
    pub key: String,
}

impl LoadedFont {
    pub fn font_ref(&self) -> Option<swash::FontRef<'_>> {
        swash::FontRef::from_index(&self.data, self.index)
    }
}

impl std::fmt::Debug for LoadedFont {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.key)
    }
}

pub struct FontCatalog {
    db: Database,
    families: Vec<String>,
}

impl FontCatalog {
    pub fn load_system() -> Self {
        let mut db = Database::new();
        db.load_system_fonts();
        let families: BTreeSet<String> = db
            .faces()
            .filter_map(|f| f.families.first().map(|(name, _)| name.clone()))
            .collect();
        log::info!("{} font faces, {} families", db.len(), families.len());
        Self {
            db,
            families: families.into_iter().collect(),
        }
    }

    /// Installed family names, sorted.
    pub fn families(&self) -> &[String] {
        &self.families
    }

    /// Weights available for `family` (upright faces), including the range of a
    /// variable `wght` axis in steps of 100.
    pub fn weights(&self, family: &str) -> Vec<u16> {
        let mut weights = BTreeSet::new();
        for face in self.db.faces() {
            if face.style != Style::Normal || !face.families.iter().any(|(n, _)| n == family) {
                continue;
            }
            weights.insert(face.weight.0);
            let range = self
                .db
                .with_face_data(face.id, |data, index| wght_axis(data, index as usize))
                .flatten();
            if let Some((min, max)) = range {
                let mut w = (min / 100.0).ceil() as u16 * 100;
                while f32::from(w) <= max {
                    weights.insert(w);
                    w += 100;
                }
            }
        }
        weights.into_iter().collect()
    }

    /// Load `family` at the closest available weight. An empty or missing family
    /// falls back to the system sans-serif font, then to any font at all.
    pub fn resolve(&self, family: &str, weight: u16) -> Option<LoadedFont> {
        let named = [Family::Name(family)];
        let sans = [Family::SansSerif];
        let candidates: [&[Family]; 2] = [&named, &sans];
        let id = candidates
            .iter()
            .filter(|families| !(family.is_empty() && matches!(families[0], Family::Name(_))))
            .find_map(|families| {
                let id = self.db.query(&Query {
                    families,
                    weight: Weight(weight),
                    stretch: Stretch::Normal,
                    style: Style::Normal,
                })?;
                // fontdb falls back across families on its own; insist on the named one.
                if let Family::Name(name) = families[0] {
                    let face = self.db.face(id)?;
                    if !face.families.iter().any(|(n, _)| n == name) {
                        return None;
                    }
                }
                Some(id)
            })
            .or_else(|| self.db.faces().next().map(|f| f.id))?;
        let face = self.db.face(id)?;
        let key = format!("{}#{}@{}", face.post_script_name, face.index, weight);
        self.db.with_face_data(id, |data, index| LoadedFont {
            data: Arc::new(data.to_vec()),
            index: index as usize,
            weight,
            key: key.clone(),
        })
    }
}

fn wght_axis(data: &[u8], index: usize) -> Option<(f32, f32)> {
    let font = swash::FontRef::from_index(data, index)?;
    font.variations()
        .find(|v| v.tag() == swash::tag_from_bytes(b"wght"))
        .map(|v| (v.min_value(), v.max_value()))
}
