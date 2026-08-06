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
    let mut workbook = open_workbook_auto(path)
        .map_err(|e| Error::msg(format!("xlsx open {}: {e}", path.display())))?;
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
        let row_number = i + 2;
        let name = cell_string(row.get(cols.name)).unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        entries.push(OperBoxEntry {
            id: format!("xlsx_{i:04}"),
            name,
            elite: cell_u8(row.get(cols.elite))
                .map_err(|error| cell_parse_error(path, row_number, COL_ELITE, error))?,
            level: cell_u32(row.get(cols.level))
                .map_err(|error| cell_parse_error(path, row_number, COL_LEVEL, error))?,
            own: cell_bool(row.get(cols.own))
                .map_err(|error| cell_parse_error(path, row_number, COL_OWN, error))?,
            potential: cell_u8(row.get(cols.potential))
                .map_err(|error| cell_parse_error(path, row_number, COL_POTENTIAL, error))?,
            rarity: cell_u8(row.get(cols.rarity))
                .map_err(|error| cell_parse_error(path, row_number, COL_RARITY, error))?,
        });
    }

    if entries.is_empty() {
        return Err(Error::msg(format!(
            "xlsx {}: no operator rows after header",
            path.display()
        )));
    }

    OperBox::from_entries(entries)
        .map_err(|error| Error::msg(format!("xlsx {}: {error}", path.display())))
}

fn cell_parse_error(path: &Path, row: usize, field: &str, error: Error) -> Error {
    Error::msg(format!(
        "xlsx {} row {row} field {field}: {error}",
        path.display()
    ))
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

fn cell_u8(cell: Option<&Data>) -> Result<u8> {
    match cell {
        None | Some(Data::Empty) => Ok(0),
        Some(Data::Int(value)) => u8::try_from(*value)
            .map_err(|_| Error::msg(format!("expected integer 0..=255, got {value}"))),
        Some(Data::Float(value))
            if value.is_finite()
                && value.fract() == 0.0
                && (0.0..=u8::MAX as f64).contains(value) =>
        {
            Ok(*value as u8)
        }
        Some(Data::String(value)) => value
            .trim()
            .parse::<u8>()
            .map_err(|_| Error::msg(format!("expected integer 0..=255, got {value:?}"))),
        value => Err(Error::msg(format!(
            "expected integer 0..=255, got {value:?}"
        ))),
    }
}

fn cell_u32(cell: Option<&Data>) -> Result<u32> {
    match cell {
        None | Some(Data::Empty) => Ok(0),
        Some(Data::Int(value)) => u32::try_from(*value)
            .map_err(|_| Error::msg(format!("expected integer 0..={}, got {value}", u32::MAX))),
        Some(Data::Float(value))
            if value.is_finite()
                && value.fract() == 0.0
                && (0.0..=u32::MAX as f64).contains(value) =>
        {
            Ok(*value as u32)
        }
        Some(Data::String(value)) => value
            .trim()
            .parse::<u32>()
            .map_err(|_| Error::msg(format!("expected integer 0..={}, got {value:?}", u32::MAX))),
        value => Err(Error::msg(format!(
            "expected integer 0..={}, got {value:?}",
            u32::MAX
        ))),
    }
}

fn cell_bool(cell: Option<&Data>) -> Result<bool> {
    match cell {
        None | Some(Data::Empty) => Ok(false),
        Some(Data::Bool(value)) => Ok(*value),
        Some(Data::Int(0)) => Ok(false),
        Some(Data::Int(1)) => Ok(true),
        Some(Data::Float(value)) if *value == 0.0 => Ok(false),
        Some(Data::Float(value)) if *value == 1.0 => Ok(true),
        Some(Data::String(value)) => parse_bool_str(value.trim())
            .ok_or_else(|| Error::msg(format!("expected boolean, got {value:?}"))),
        value => Err(Error::msg(format!("expected boolean, got {value:?}"))),
    }
}

fn parse_bool_str(s: &str) -> Option<bool> {
    match s {
        "1" | "true" | "True" | "TRUE" | "是" | "Y" | "y" | "yes" | "Yes" | "YES" => Some(true),
        "0" | "false" | "False" | "FALSE" | "否" | "N" | "n" | "no" | "No" | "NO" => Some(false),
        _ => None,
    }
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
    fn parse_bool_and_int_cells_strictly() {
        assert!(!cell_bool(Some(&Data::Empty)).unwrap());
        assert!(cell_bool(Some(&Data::Bool(true))).unwrap());
        assert!(cell_bool(Some(&Data::Int(1))).unwrap());
        assert!(!cell_bool(Some(&Data::String("否".into()))).unwrap());
        assert!(cell_bool(Some(&Data::String("unknown".into()))).is_err());
        assert_eq!(cell_u8(Some(&Data::Empty)).unwrap(), 0);
        assert_eq!(cell_u32(None).unwrap(), 0);
        assert_eq!(cell_u8(Some(&Data::Float(2.0))).unwrap(), 2);
        assert!(cell_u8(Some(&Data::Float(2.4))).is_err());
        assert_eq!(cell_u32(Some(&Data::String("90".into()))).unwrap(), 90);
        assert!(cell_u32(Some(&Data::String("90.5".into()))).is_err());
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
