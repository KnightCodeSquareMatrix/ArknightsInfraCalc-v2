use serde::{Deserialize, Serialize};

/// 制造爬升技能默认连续积分窗口。
pub const MANU_RAMP_HOURS: u32 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffRampStyle {
    /// 首小时保持 `initial`，此后连续每小时 +`per_hour`（芬/空构）。
    #[default]
    FirstHourThenHourly,
    /// 制造首小时为零，此后连续每小时 +`per_hour`（阿罗玛·例行清扫）。
    HourlyOnly,
}

/// 第 `hour` 小时结束时的生产力/充能加成 %（`hour` ≥ 1）。
pub fn eff_at_hour(style: EffRampStyle, initial: f64, per_hour: f64, cap: f64, hour: u32) -> f64 {
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

/// 制造连续暖机在进驻后 `elapsed` 小时的加成；每班从零重新计时。
pub fn manufacture_eff_ramp_at(
    style: EffRampStyle,
    initial: f64,
    per_hour: f64,
    cap: f64,
    elapsed: f64,
) -> f64 {
    let start = match style {
        EffRampStyle::FirstHourThenHourly => initial,
        EffRampStyle::HourlyOnly => 0.0,
    };
    (start + per_hour * (elapsed - 1.0).max(0.0)).min(cap)
}

/// 制造暖机线性段与封顶段的交点；非正斜率也按同一个 min(cap) 函数处理。
pub fn manufacture_eff_ramp_cap_time(
    style: EffRampStyle,
    initial: f64,
    per_hour: f64,
    cap: f64,
) -> Option<f64> {
    let start = match style {
        EffRampStyle::FirstHourThenHourly => initial,
        EffRampStyle::HourlyOnly => 0.0,
    };
    if per_hour == 0.0 {
        return None;
    }
    let elapsed = 1.0 + (cap - start) / per_hour;
    (elapsed.is_finite() && elapsed > 1.0).then_some(elapsed)
}

/// 真实 elapsed 区间的解析平均；先切首小时和封顶，再积分仿射段。
/// 非正区间返回起点值，不除以零；不会逐小时循环或截断长班。
pub fn manufacture_eff_ramp_avg(
    style: EffRampStyle,
    initial: f64,
    per_hour: f64,
    cap: f64,
    start: f64,
    end: f64,
) -> f64 {
    if end <= start {
        return manufacture_eff_ramp_at(style, initial, per_hour, cap, start);
    }
    let mut cuts = [start, end, end, end];
    let mut count = 2;
    if start < 1.0 && 1.0 < end {
        cuts[count] = 1.0;
        count += 1;
    }
    if let Some(time) = manufacture_eff_ramp_cap_time(style, initial, per_hour, cap) {
        if start < time && time < end {
            cuts[count] = time;
            count += 1;
        }
    }
    cuts[..count].sort_by(f64::total_cmp);
    // 归一化权重避免有限超长窗口的面积乘积溢出。
    cuts[..count]
        .windows(2)
        .map(|window| {
            let midpoint = window[0] + (window[1] - window[0]) / 2.0;
            manufacture_eff_ramp_at(style, initial, per_hour, cap, midpoint)
                * ((window[1] - window[0]) / (end - start))
        })
        .sum()
}

/// `hours` 小时内连续加成积分平均（制造纸面等效效率）。
pub fn eff_ramp_avg_over_hours(
    style: EffRampStyle,
    initial: f64,
    per_hour: f64,
    cap: f64,
    hours: u32,
) -> f64 {
    manufacture_eff_ramp_avg(style, initial, per_hour, cap, 0.0, f64::from(hours))
}

/// 制造爬升默认：20h 连续积分平均。
pub fn eff_ramp_avg_20h(style: EffRampStyle, initial: f64, per_hour: f64, cap: f64) -> f64 {
    eff_ramp_avg_over_hours(style, initial, per_hour, cap, MANU_RAMP_HOURS)
}
