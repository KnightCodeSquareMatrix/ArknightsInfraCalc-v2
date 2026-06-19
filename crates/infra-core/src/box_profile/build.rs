use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::Result;
use crate::instances::OperatorInstances;
use crate::layout::BaseBlueprint;
use crate::operbox::{default_operbox_full_e2_path, OperBox};
use crate::roster::OperatorProgress;
use crate::skill_table::SkillTable;
use crate::tier::PromotionTier;

use super::eval::{default_schedule_export_path, run_schedule_eval_probe};
use super::probe::{run_user_rotation_probe, LayoutProbe};

#[derive(Debug, Clone)]
pub struct BoxProfileOptions {
    pub top_k: usize,
    /// baseline 练度（默认 full_e2，用于公孙 schedule eval）。
    pub baseline_operbox: Option<PathBuf>,
    /// 公孙参考排班 JSON（默认 `schedule_export.json`）。
    pub baseline_schedule: Option<PathBuf>,
    pub gap_warn: f64,
    pub gap_critical: f64,
}

impl Default for BoxProfileOptions {
    fn default() -> Self {
        Self {
            top_k: 10,
            baseline_operbox: None,
            baseline_schedule: None,
            gap_warn: 0.08,
            gap_critical: 0.20,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GapSeverity {
    Ok,
    Warn,
    Critical,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComboSnapshot {
    pub operators: Vec<String>,
    pub score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trade_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gold_pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DomainMetric {
    pub id: &'static str,
    pub label: &'static str,
    pub current: ComboSnapshot,
    pub baseline: ComboSnapshot,
    pub gap_ratio: f64,
    pub severity: GapSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    PromoteTierUp,
    Acquire,
    Note,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProfileAction {
    pub priority: String,
    pub kind: ActionKind,
    pub operator: String,
    pub domain_id: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_elite: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier_up_requirement: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OperboxSummary {
    pub owned: usize,
    #[serde(alias = "elite2")]
    pub tier_up_owned: usize,
    pub trade_pool_ready: usize,
    pub manu_pool_ready: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RotationSnapshot {
    pub daily_trade: f64,
    pub daily_manu: f64,
    pub daily_power: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BoxProfile {
    pub schema_version: u32,
    pub layout_label: String,
    pub operbox_label: String,
    pub baseline_label: String,
    pub summary: OperboxSummary,
    pub domains: Vec<DomainMetric>,
    pub rotation: RotationSnapshot,
    pub baseline_rotation: RotationSnapshot,
    pub actions: Vec<ProfileAction>,
    pub flags: Vec<String>,
    pub narration_hints: Vec<String>,
}

pub fn build_box_profile(
    blueprint: &BaseBlueprint,
    operbox: &OperBox,
    instances: &OperatorInstances,
    table: &SkillTable,
    layout_label: &str,
    operbox_label: &str,
    options: &BoxProfileOptions,
) -> Result<BoxProfile> {
    let schedule_path = options
        .baseline_schedule
        .clone()
        .unwrap_or_else(|| default_schedule_export_path().expect("schedule_export path"));
    let full_e2_path = options
        .baseline_operbox
        .clone()
        .unwrap_or_else(|| default_operbox_full_e2_path().expect("baseline operbox path"));
    let baseline_operbox = OperBox::load(&full_e2_path)?;
    let baseline_label = format!(
        "{} (full_e2)",
        schedule_path.display()
    );

    // current：用户练度 → solver 排班；baseline：公孙固定编制 + 顶配练度 eval。
    let current = run_user_rotation_probe(blueprint, operbox, instances, table, options.top_k)?;
    let baseline = run_schedule_eval_probe(
        blueprint,
        &baseline_operbox,
        instances,
        table,
        &schedule_path,
    )?;

    let domains = build_domains(&current, &baseline, options);
    let actions = build_actions(operbox, &domains);
    let flags = build_flags(&domains);
    let narration_hints = build_narration_hints(&domains, &current, &baseline);
    let rotation = rotation_snapshot(&current);
    let baseline_rotation = rotation_snapshot(&baseline);

    Ok(BoxProfile {
        schema_version: 2,
        layout_label: layout_label.to_string(),
        operbox_label: operbox_label.to_string(),
        baseline_label,
        summary: OperboxSummary {
            owned: current.owned,
            tier_up_owned: current.tier_up_owned,
            trade_pool_ready: current.trade_pool_ready,
            manu_pool_ready: current.manu_pool_ready,
        },
        domains,
        rotation,
        baseline_rotation,
        actions,
        flags,
        narration_hints,
    })
}

pub(super) fn rotation_snapshot(probe: &LayoutProbe) -> RotationSnapshot {
    RotationSnapshot {
        daily_trade: probe.rotation.daily.trade,
        daily_manu: probe.rotation.daily.manu,
        daily_power: probe.rotation.daily.power,
    }
}

fn gap_ratio(current: f64, baseline: f64) -> f64 {
    if baseline.abs() < f64::EPSILON {
        0.0
    } else {
        (current - baseline) / baseline
    }
}

fn severity(gap: f64, warn: f64, critical: f64) -> GapSeverity {
    if gap <= -critical {
        GapSeverity::Critical
    } else if gap <= -warn {
        GapSeverity::Warn
    } else {
        GapSeverity::Ok
    }
}

pub(super) fn build_domains(
    current: &LayoutProbe,
    baseline: &LayoutProbe,
    options: &BoxProfileOptions,
) -> Vec<DomainMetric> {
    let mut out = Vec::new();

    if let (Some(g), Some(bg)) = (
        current.trade_report.gold_order_line.as_ref(),
        baseline.trade_report.gold_order_line.as_ref(),
    ) {
        let gap = gap_ratio(g.score, bg.score);
        out.push(DomainMetric {
            id: "trade_gold",
            label: "贸易·赤金线",
            current: ComboSnapshot {
                operators: g.names.clone(),
                score: g.score,
                trade_pct: Some(g.trade_pct),
                gold_pct: Some(g.gold_pct),
            },
            baseline: ComboSnapshot {
                operators: bg.names.clone(),
                score: bg.score,
                trade_pct: Some(bg.trade_pct),
                gold_pct: Some(bg.gold_pct),
            },
            gap_ratio: gap,
            severity: severity(gap, options.gap_warn, options.gap_critical),
        });
    }

    if let (Some(o), Some(bo)) = (
        current.trade_report.originium_order_line.as_ref(),
        baseline.trade_report.originium_order_line.as_ref(),
    ) {
        let gap = gap_ratio(o.score, bo.score);
        out.push(DomainMetric {
            id: "trade_originium",
            label: "贸易·订单/源石线",
            current: ComboSnapshot {
                operators: o.names.clone(),
                score: o.score,
                trade_pct: Some(o.trade_pct),
                gold_pct: None,
            },
            baseline: ComboSnapshot {
                operators: bo.names.clone(),
                score: bo.score,
                trade_pct: Some(bo.trade_pct),
                gold_pct: None,
            },
            gap_ratio: gap,
            severity: severity(gap, options.gap_warn, options.gap_critical),
        });
    }

    {
        let c = &current.manu_report.best;
        let b = &baseline.manu_report.best;
        let gap = gap_ratio(c.composite_score, b.composite_score);
        out.push(DomainMetric {
            id: "manu_composite",
            label: "制造·综合",
            current: ComboSnapshot {
                operators: c.names.clone(),
                score: c.composite_score,
                trade_pct: None,
                gold_pct: None,
            },
            baseline: ComboSnapshot {
                operators: b.names.clone(),
                score: b.composite_score,
                trade_pct: None,
                gold_pct: None,
            },
            gap_ratio: gap,
            severity: severity(gap, options.gap_warn, options.gap_critical),
        });
    }

    if let (Some(g), Some(bg)) = (
        current.manu_report.gold_line.as_ref(),
        baseline.manu_report.gold_line.as_ref(),
    ) {
        let gap = gap_ratio(g.composite_score, bg.composite_score);
        out.push(DomainMetric {
            id: "manu_gold",
            label: "制造·赤金产线",
            current: ComboSnapshot {
                operators: g.names.clone(),
                score: g.composite_score,
                trade_pct: None,
                gold_pct: None,
            },
            baseline: ComboSnapshot {
                operators: bg.names.clone(),
                score: bg.composite_score,
                trade_pct: None,
                gold_pct: None,
            },
            gap_ratio: gap,
            severity: severity(gap, options.gap_warn, options.gap_critical),
        });
    }

    if let (Some(e), Some(be)) = (
        current.manu_report.battle_record_line.as_ref(),
        baseline.manu_report.battle_record_line.as_ref(),
    ) {
        let gap = gap_ratio(e.composite_score, be.composite_score);
        out.push(DomainMetric {
            id: "manu_exp",
            label: "制造·经验产线",
            current: ComboSnapshot {
                operators: e.names.clone(),
                score: e.composite_score,
                trade_pct: None,
                gold_pct: None,
            },
            baseline: ComboSnapshot {
                operators: be.names.clone(),
                score: be.composite_score,
                trade_pct: None,
                gold_pct: None,
            },
            gap_ratio: gap,
            severity: severity(gap, options.gap_warn, options.gap_critical),
        });
    }

    let gap = gap_ratio(current.rotation.daily.trade, baseline.rotation.daily.trade);
    out.push(DomainMetric {
        id: "rotation_trade",
        label: "轮休·24h贸易加权",
        current: ComboSnapshot {
            operators: vec![],
            score: current.rotation.daily.trade,
            trade_pct: None,
            gold_pct: None,
        },
        baseline: ComboSnapshot {
            operators: vec![],
            score: baseline.rotation.daily.trade,
            trade_pct: None,
            gold_pct: None,
        },
        gap_ratio: gap,
        severity: severity(gap, options.gap_warn, options.gap_critical),
    });

    let gap = gap_ratio(current.rotation.daily.manu, baseline.rotation.daily.manu);
    out.push(DomainMetric {
        id: "rotation_manu",
        label: "轮休·24h制造加权",
        current: ComboSnapshot {
            operators: vec![],
            score: current.rotation.daily.manu,
            trade_pct: None,
            gold_pct: None,
        },
        baseline: ComboSnapshot {
            operators: vec![],
            score: baseline.rotation.daily.manu,
            trade_pct: None,
            gold_pct: None,
        },
        gap_ratio: gap,
        severity: severity(gap, options.gap_warn, options.gap_critical),
    });

    out
}

pub(super) fn build_actions(operbox: &OperBox, domains: &[DomainMetric]) -> Vec<ProfileAction> {
    let mut actions = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for domain in domains {
        if domain.severity == GapSeverity::Ok {
            continue;
        }
        let priority = match domain.severity {
            GapSeverity::Critical => "P0",
            GapSeverity::Warn => "P1",
            GapSeverity::Ok => continue,
        };

        for name in &domain.baseline.operators {
            if !seen.insert((name.clone(), domain.id.to_string())) {
                continue;
            }
            if !operbox.owns(name) {
                actions.push(ProfileAction {
                    priority: priority.to_string(),
                    kind: ActionKind::Acquire,
                    operator: name.clone(),
                    domain_id: domain.id.to_string(),
                    message: format!(
                        "获取「{}」——{}参考最优组合需要（现未拥有）",
                        name, domain.label
                    ),
                    current_elite: None,
                    tier_up_requirement: None,
                });
            } else if let Some(action) =
                tier_up_action(operbox, name, priority, domain, "参考组合成员")
            {
                actions.push(action);
            }
        }

        for name in &domain.current.operators {
            if domain.baseline.operators.iter().any(|n| n == name) {
                continue;
            }
            if !seen.insert((name.clone(), format!("{}_sub", domain.id))) {
                continue;
            }
            if let Some(action) =
                tier_up_action(operbox, name, "P2", domain, "当前在用组合")
            {
                actions.push(action);
            }
        }
    }

    actions.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| a.operator.cmp(&b.operator))
    });

    // 同一干员只保留最高优先级的一条（跨域参考组合可能重复）。
    let mut deduped = Vec::new();
    let mut seen_ops = std::collections::HashSet::new();
    for action in actions {
        if seen_ops.insert(action.operator.clone()) {
            deduped.push(action);
        }
    }
    deduped
}

fn tier_up_action(
    operbox: &OperBox,
    name: &str,
    priority: &str,
    domain: &DomainMetric,
    role: &str,
) -> Option<ProfileAction> {
    let progress = operbox.progress_of(name)?;
    if PromotionTier::is_tier_up(progress) {
        return None;
    }
    let req = PromotionTier::tier_up_requirement_label(progress);
    let brief = PromotionTier::format_progress_brief(progress);
    Some(ProfileAction {
        priority: priority.to_string(),
        kind: ActionKind::PromoteTierUp,
        operator: name.to_string(),
        domain_id: domain.id.to_string(),
        message: format!(
            "将「{}」升至 tier_up（需{}）——{}{}，现{}",
            name, req, domain.label, role, brief
        ),
        current_elite: Some(progress.elite),
        tier_up_requirement: Some(req.to_string()),
    })
}

pub(super) fn build_flags(domains: &[DomainMetric]) -> Vec<String> {
    let mut flags = Vec::new();
    for d in domains {
        if d.severity == GapSeverity::Critical {
            flags.push(format!("{}_critical", d.id));
        } else if d.severity == GapSeverity::Warn {
            flags.push(format!("{}_warn", d.id));
        }
    }
    if domains
        .iter()
        .any(|d| d.id == "trade_gold" && d.severity == GapSeverity::Ok)
    {
        flags.push("trade_gold_ok".to_string());
    }
    if domains
        .iter()
        .any(|d| d.id == "manu_composite" && d.severity == GapSeverity::Critical)
    {
        flags.push("manu_bottleneck".to_string());
    }
    flags
}

pub(super) fn build_narration_hints(
    domains: &[DomainMetric],
    current: &LayoutProbe,
    baseline: &LayoutProbe,
) -> Vec<String> {
    let mut hints = Vec::new();

    if let (Some(g), Some(bg)) = (
        current.trade_report.gold_order_line.as_ref(),
        baseline.trade_report.gold_order_line.as_ref(),
    ) {
        if g.names == bg.names {
            hints.push("赤金贸易 meta 组合与公孙参考一致，差距主要在练度".to_string());
        }
    }

    if let (Some(c), Some(b)) = (
        domains.iter().find(|d| d.id == "manu_composite"),
        domains.iter().find(|d| d.id == "trade_gold"),
    ) {
        if c.severity == GapSeverity::Critical && b.severity != GapSeverity::Critical {
            hints.push("制造为当前最大短板，优先于贸易".to_string());
        }
    }

    if current.trade_pool_ready < baseline.trade_pool_ready / 2 {
        hints.push(format!(
            "贸易可建模池偏小（{} vs 参考 {}），部分干员未拥有或未录入",
            current.trade_pool_ready, baseline.trade_pool_ready
        ));
    }

    hints
}

pub fn baseline_path_or_default(path: Option<&Path>) -> Result<PathBuf> {
    match path {
        Some(p) => Ok(p.to_path_buf()),
        None => default_operbox_full_e2_path(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gap_severity_thresholds() {
        assert_eq!(severity(-0.05, 0.08, 0.20), GapSeverity::Ok);
        assert_eq!(severity(-0.10, 0.08, 0.20), GapSeverity::Warn);
        assert_eq!(severity(-0.45, 0.08, 0.20), GapSeverity::Critical);
    }

    #[test]
    fn tier_up_action_respects_star_rules() {
        use std::collections::HashMap;

        let operbox = OperBox {
            entries: vec![],
            owned: HashMap::from([
                ("清流".to_string(), OperatorProgress::new(1, 1, 4)),
                ("槐琥".to_string(), OperatorProgress::new(1, 80, 5)),
            ]),
        };
        let domain = DomainMetric {
            id: "manu_gold",
            label: "制造·赤金",
            current: ComboSnapshot {
                operators: vec![],
                score: 0.0,
                trade_pct: None,
                gold_pct: None,
            },
            baseline: ComboSnapshot {
                operators: vec!["清流".to_string(), "槐琥".to_string()],
                score: 1.0,
                trade_pct: None,
                gold_pct: None,
            },
            gap_ratio: -0.3,
            severity: GapSeverity::Critical,
        };
        let actions = build_actions(&operbox, &[domain]);
        assert!(
            !actions.iter().any(|a| a.operator == "清流"),
            "4★精1已是 tier_up，不应建议升级"
        );
        assert_eq!(actions.iter().filter(|a| a.operator == "槐琥").count(), 1);
        let h = actions.iter().find(|a| a.operator == "槐琥").unwrap();
        assert_eq!(h.kind, ActionKind::PromoteTierUp);
        assert_eq!(h.tier_up_requirement.as_deref(), Some("精2"));
    }
}
