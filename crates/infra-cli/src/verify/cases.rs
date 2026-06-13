use std::path::Path;

use csv::ReaderBuilder;
use infra_core::Error;

#[derive(Debug)]
pub struct UnitAnchorCase {
    pub case_id: String,
    pub trade_level: u8,
    pub fixture: String,
    pub expect_unit_trade: f64,
    pub expect_gsl_unit_gold: f64,
    pub tolerance_pct: f64,
}

pub fn load_unit_anchor_cases(path: &Path) -> Result<Vec<UnitAnchorCase>, Error> {
    let mut rdr = ReaderBuilder::new().from_path(path)?;
    let mut out = Vec::new();
    for rec in rdr.records() {
        let rec = rec?;
        out.push(UnitAnchorCase {
            case_id: rec[0].to_string(),
            trade_level: rec[1].parse().unwrap_or(3),
            fixture: rec[2].to_string(),
            expect_unit_trade: rec[3].parse().unwrap_or(0.0),
            expect_gsl_unit_gold: rec[4].parse().unwrap_or(0.0),
            tolerance_pct: rec[5].parse().unwrap_or(10.0),
        });
    }
    Ok(out)
}

#[derive(Debug)]
pub struct RegressionCase {
    pub case_id: String,
    pub expect_trade_pct: f64,
    pub expect_gold_pct: f64,
    pub expect_shortcut: String,
    pub tolerance: f64,
    pub trade_level: u8,
    pub operators: String,
}

pub fn load_regression_cases(path: &Path) -> Result<Vec<RegressionCase>, Error> {
    let mut rdr = ReaderBuilder::new().from_path(path)?;
    let mut out = Vec::new();
    for rec in rdr.records() {
        let rec = rec?;
        out.push(RegressionCase {
            case_id: rec[0].to_string(),
            expect_shortcut: rec[1].to_string(),
            operators: rec[2].to_string(),
            trade_level: rec[3].parse().unwrap_or(3),
            expect_trade_pct: rec[4].parse().unwrap_or(0.0),
            expect_gold_pct: rec[5].parse().unwrap_or(0.0),
            tolerance: rec[7].parse().unwrap_or(0.5),
        });
    }
    Ok(out)
}
