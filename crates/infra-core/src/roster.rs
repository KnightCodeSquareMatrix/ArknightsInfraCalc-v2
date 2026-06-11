use std::collections::HashMap;
use std::path::Path;

use csv::ReaderBuilder;

use crate::error::{Error, Result};

/// Player-owned operators: canonical name → elite level (0–2).
#[derive(Debug, Clone, Default)]
pub struct Roster {
    elite_by_name: HashMap<String, u8>,
}

impl Roster {
    pub fn from_elite_map(elite_by_name: HashMap<String, u8>) -> Self {
        Self { elite_by_name }
    }

    pub fn insert(&mut self, name: impl Into<String>, elite: u8) {
        self.elite_by_name.insert(name.into(), elite);
    }

    pub fn elite(&self, name: &str) -> Option<u8> {
        self.elite_by_name.get(name).copied()
    }

    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.elite_by_name.keys()
    }

    pub fn len(&self) -> usize {
        self.elite_by_name.len()
    }

    pub fn is_empty(&self) -> bool {
        self.elite_by_name.is_empty()
    }

    /// Load `roster.csv` rows for a facility (`trade`, `manufacture`, …).
    /// Duplicate names keep the higher elite.
    pub fn load_csv_for_facility(path: &Path, facility: &str) -> Result<Self> {
        let mut rdr = ReaderBuilder::new().from_path(path)?;
        let headers = rdr.headers()?.clone();
        let idx = |col: &str| -> Result<usize> {
            headers
                .iter()
                .position(|h| h == col)
                .ok_or_else(|| Error::msg(format!("roster.csv missing column {col}")))
        };
        let name_i = idx("name")?;
        let facility_i = idx("facility")?;
        let elite_i = idx("elite")?;

        let mut elite_by_name = HashMap::new();
        for rec in rdr.records() {
            let rec = rec?;
            if rec.get(facility_i).is_none_or(|f| f != facility) {
                continue;
            }
            let name = rec[name_i].trim().to_string();
            if name.is_empty() {
                continue;
            }
            let elite: u8 = rec[elite_i]
                .trim()
                .parse()
                .map_err(|_| Error::msg(format!("invalid elite for {name}")))?;
            elite_by_name
                .entry(name)
                .and_modify(|e: &mut u8| *e = (*e).max(elite))
                .or_insert(elite);
        }
        Ok(Self { elite_by_name })
    }
}

pub fn default_roster_path() -> Result<std::path::PathBuf> {
    crate::skill_table::data_path("roster.csv")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_trade_roster_takes_max_elite() {
        let path = default_roster_path().unwrap();
        let roster = Roster::load_csv_for_facility(&path, "trade").unwrap();
        assert_eq!(roster.elite("但书"), Some(2));
        assert_eq!(roster.elite("龙舌兰"), Some(2));
    }
}
