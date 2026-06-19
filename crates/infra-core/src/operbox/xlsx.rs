//! 一图流「干员练度表」xlsx → OperBox（与 `scripts/xlsx_to_operbox.py` 同列语义）。

use std::collections::HashMap;
use std::path::Path;

use calamine::{open_workbook_auto, Data, Reader};

use super::{OperBox, OperBoxEntry};
use crate::error::{Error, Result};

const COL_NAME: &str = "干员名称";
const COL_OWN: &str = "是否已招募";
const COL_RARITY: &str = "星级";
const COL_LEVEL: &str = "等级";
const COL_ELITE: &str = "精英化等级";
const COL_POTENTIAL: &str = "潜能等级";

const REQUIRED: [&str; 6] = [
    COL_NAME,
    COL_OWN,
    COL_RARITY,
    COL_LEVEL,
    COL_ELITE,
    COL_POTENTIAL,
];

struct ColumnMap {
    name: usize,
    own: usize,
    rarity: usize,
    level: usize,
    elite: usize,
    potential: usize,
}

pub fn from_xlsx_path(path: &Path) -> Result<OperBox> {
    let mut workbook = open_workbook_auto(path).map_err(|e| {
        Error::msg(format!("xlsx open {}: {e}", path.display()))
    })?;
    let sheet_name = workbook
        .sheet_names()
        .first()
        .cloned()
        .ok_or_else(|| Error::msg(format!("xlsx has no sheets: {}", path.display())))?;
    let range = workbook
        .worksheet_range(&sheet_name)
        .map_err(|e| Error::msg(format!("xlsx read sheet {sheet_name}: {e}")))?;

    let mut rows = range.rows();
    let header = rows
        .next()
        .ok_or_else(|| Error::msg(format!("xlsx sheet {sheet_name} is empty")))?;
    let cols = column_map(header, &sheet_name)?;

    let mut entries = Vec::new();
    for (i, row) in rows.enumerate() {
        let name = cell_string(row.get(cols.name)).unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        entries.push(OperBoxEntry {
            id: format!("xlsx_{i:04}"),
            name,
            elite: cell_u8(row.get(cols.elite)),
            level: cell_u32(row.get(cols.level)),
            own: cell_bool(row.get(cols.own)),
            potential: cell_u8(row.get(cols.potential)),
            rarity: cell_u8(row.get(cols.rarity)),
        });
    }

    if entries.is_empty() {
        return Err(Error::msg(format!(
            "xlsx {}: no operator rows after header",
            path.display()
        )));
    }

    Ok(OperBox::from_entries(entries))
}

fn column_map(header: &[Data], sheet: &str) -> Result<ColumnMap> {
    let mut index: HashMap<String, usize> = HashMap::new();
    for (i, cell) in header.iter().enumerate() {
        if let Some(label) = cell_string(Some(cell)) {
            if !label.is_empty() {
                index.insert(label, i);
            }
        }
    }
    let mut missing: Vec<&'static str> = Vec::new();
    for col in REQUIRED {
        if !index.contains_key(col) {
            missing.push(col);
        }
    }
    if !missing.is_empty() {
        return Err(Error::msg(format!(
            "xlsx sheet {sheet}: missing columns: {missing:?}"
        )));
    }
    Ok(ColumnMap {
        name: index[COL_NAME],
        own: index[COL_OWN],
        rarity: index[COL_RARITY],
        level: index[COL_LEVEL],
        elite: index[COL_ELITE],
        potential: index[COL_POTENTIAL],
    })
}

fn cell_string(cell: Option<&Data>) -> Option<String> {
    match cell {
        None | Some(Data::Empty) => None,
        Some(Data::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        Some(Data::Float(f)) => Some(trim_float(*f)),
        Some(Data::Int(i)) => Some(i.to_string()),
        Some(Data::Bool(b)) => Some(b.to_string()),
        Some(Data::DateTime(_)) => None,
        Some(Data::DateTimeIso(_)) => None,
        Some(Data::DurationIso(_)) => None,
        Some(Data::Error(_)) => None,
    }
}

fn trim_float(f: f64) -> String {
    if (f - f.round()).abs() < f64::EPSILON {
        format!("{}", f.round() as i64)
    } else {
        format!("{f}")
    }
}

fn cell_u8(cell: Option<&Data>) -> u8 {
    match cell {
        None | Some(Data::Empty) => 0,
        Some(Data::Int(i)) => (*i).clamp(0, 255) as u8,
        Some(Data::Float(f)) => f.round().clamp(0.0, 255.0) as u8,
        Some(Data::Bool(b)) => u8::from(*b),
        Some(Data::String(s)) => parse_u8_str(s.trim()),
        _ => 0,
    }
}

fn cell_u32(cell: Option<&Data>) -> u32 {
    match cell {
        None | Some(Data::Empty) => 0,
        Some(Data::Int(i)) => (*i).max(0) as u32,
        Some(Data::Float(f)) => f.round().max(0.0) as u32,
        Some(Data::Bool(b)) => u32::from(*b),
        Some(Data::String(s)) => parse_u32_str(s.trim()),
        _ => 0,
    }
}

fn cell_bool(cell: Option<&Data>) -> bool {
    match cell {
        None | Some(Data::Empty) => false,
        Some(Data::Bool(b)) => *b,
        Some(Data::Int(i)) => *i != 0,
        Some(Data::Float(f)) => *f != 0.0,
        Some(Data::String(s)) => parse_bool_str(s.trim()),
        _ => false,
    }
}

fn parse_u8_str(s: &str) -> u8 {
    s.parse::<u8>().unwrap_or(0)
}

fn parse_u32_str(s: &str) -> u32 {
    s.parse::<u32>().unwrap_or(0)
}

fn parse_bool_str(s: &str) -> bool {
    matches!(
        s,
        "1" | "true" | "True" | "TRUE" | "是" | "Y" | "y" | "yes" | "Yes" | "YES"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn knightcode_xlsx() -> Option<PathBuf> {
        let p = PathBuf::from(r"c:\Users\KnightCode\Downloads\干员练度表.xlsx");
        p.exists().then_some(p)
    }

    #[test]
    fn parse_bool_and_int_cells() {
        assert!(!cell_bool(Some(&Data::Empty)));
        assert!(cell_bool(Some(&Data::Bool(true))));
        assert!(cell_bool(Some(&Data::Int(1))));
        assert!(cell_bool(Some(&Data::String("是".into()))));
        assert_eq!(cell_u8(Some(&Data::Float(2.0))), 2);
        assert_eq!(cell_u32(Some(&Data::String("90".into()))), 90);
    }

    #[test]
    fn load_yituliu_xlsx_when_present() {
        let Some(path) = knightcode_xlsx() else {
            eprintln!("skip: 干员练度表.xlsx not on disk");
            return;
        };
        let box_ = from_xlsx_path(&path).unwrap();
        assert!(box_.entries.len() >= 400);
        assert!(box_.owned_count() >= 300);
        assert!(box_.owns("巫恋"));
        assert!(box_.owns("但书"));
    }
}
