use serde::{Deserialize, Serialize};

/// 制造爬升技能默认汇总窗口（20h 效率算术平均）。
pub const MANU_RAMP_HOURS: u32 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffRampStyle {
    /// 首小时 `initial`，此后每小时 +`per_hour`（芬/空构）。
    #[default]
    FirstHourThenHourly,
    /// 无首小时加成，第 h 小时末为 `h × per_hour`（阿罗玛·例行清扫）。
    HourlyOnly,
}

/// 第 `hour` 小时结束时的生产力/充能加成 %（`hour` ≥ 1）。
pub fn eff_at_hour(
    style: EffRampStyle,
    initial: f64,
    per_hour: f64,
    cap: f64,
    hour: u32,
) -> f64 {
    if hour == 0 {
        return 0.0;
    }
    match style {
        EffRampStyle::FirstHourThenHourly => {
            let extra = hour.saturating_sub(1) as f64 * per_hour;
            (initial + extra).min(cap)
        }
        EffRampStyle::HourlyOnly => (hour as f64 * per_hour).min(cap),
    }
}

/// 连续工作 `shift_hours` 小时末的纸面加成 %（发电站单点；支持小数小时）。
pub fn eff_ramp_at_shift_hours(
    style: EffRampStyle,
    initial: f64,
    per_hour: f64,
    cap: f64,
    shift_hours: f64,
) -> f64 {
    if shift_hours <= 0.0 {
        return 0.0;
    }
    let worked = shift_hours.min(24.0);
    match style {
        EffRampStyle::FirstHourThenHourly => {
            let extra = (worked - 1.0).max(0.0) * per_hour;
            (initial + extra).min(cap)
        }
        EffRampStyle::HourlyOnly => (worked * per_hour).min(cap),
    }
}

/// `hours` 小时内逐时加成算术平均（制造纸面等效效率）。
pub fn eff_ramp_avg_over_hours(
    style: EffRampStyle,
    initial: f64,
    per_hour: f64,
    cap: f64,
    hours: u32,
) -> f64 {
    if hours == 0 {
        return 0.0;
    }
    let sum: f64 = (1..=hours)
        .map(|h| eff_at_hour(style, initial, per_hour, cap, h))
        .sum();
    sum / f64::from(hours)
}

/// 制造爬升默认：20h 效率算术平均。
pub fn eff_ramp_avg_20h(style: EffRampStyle, initial: f64, per_hour: f64, cap: f64) -> f64 {
    eff_ramp_avg_over_hours(style, initial, per_hour, cap, MANU_RAMP_HOURS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fen_first_hour_curve() {
        let s = EffRampStyle::FirstHourThenHourly;
        assert!((eff_at_hour(s, 20.0, 1.0, 25.0, 1) - 20.0).abs() < f64::EPSILON);
        assert!((eff_at_hour(s, 20.0, 1.0, 25.0, 6) - 25.0).abs() < f64::EPSILON);
        assert!((eff_ramp_avg_20h(s, 20.0, 1.0, 25.0) - 24.25).abs() < 0.01);
    }

    #[test]
    fn kroos_slow_curve() {
        let s = EffRampStyle::FirstHourThenHourly;
        assert!((eff_at_hour(s, 15.0, 2.0, 25.0, 1) - 15.0).abs() < f64::EPSILON);
        assert!((eff_ramp_avg_20h(s, 15.0, 2.0, 25.0) - 23.5).abs() < 0.01);
    }

    #[test]
    fn aroma_hourly_only() {
        let s = EffRampStyle::HourlyOnly;
        assert!((eff_at_hour(s, 0.0, 2.0, 20.0, 1) - 2.0).abs() < f64::EPSILON);
        assert!((eff_at_hour(s, 0.0, 2.0, 20.0, 10) - 20.0).abs() < f64::EPSILON);
        assert!((eff_ramp_avg_20h(s, 0.0, 2.0, 20.0) - 15.5).abs() < 0.01);
    }

    #[test]
    fn power_float_shift_matches_legacy() {
        let s = EffRampStyle::FirstHourThenHourly;
        assert!((eff_ramp_at_shift_hours(s, 10.0, 1.0, 15.0, 1.0) - 10.0).abs() < f64::EPSILON);
        assert!((eff_ramp_at_shift_hours(s, 10.0, 1.0, 15.0, 6.0) - 15.0).abs() < f64::EPSILON);
    }
}
