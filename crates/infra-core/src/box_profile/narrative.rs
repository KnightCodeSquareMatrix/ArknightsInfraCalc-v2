use super::build::{ActionKind, BoxProfile, DomainMetric, GapSeverity};

/// 将 BoxProfile 渲染为面向用户的中文评价与建议（Bot / RAG 可直接消费）。
pub fn render_box_profile_narrative(profile: &BoxProfile) -> String {
    let mut out = String::new();

    out.push_str("══════════════════════════════════════\n");
    out.push_str("  基建账号画像 · 评价与建议\n");
    out.push_str("══════════════════════════════════════\n\n");

    out.push_str(&format!("布局：{}\n", profile.layout_label));
    out.push_str(&format!("练度盒：{}\n", profile.operbox_label));
    out.push_str(&format!("参考基准：{}\n\n", profile.baseline_label));

    out.push_str("【账号概览】\n");
    out.push_str(&format!(
        "  已拥有 {} 名干员，其中 tier_up {} 名\n",
        profile.summary.owned, profile.summary.tier_up_owned
    ));
    out.push_str(&format!(
        "  可建模池：贸易 {} 人 · 制造 {} 人\n\n",
        profile.summary.trade_pool_ready, profile.summary.manu_pool_ready
    ));

    out.push_str("【总体评价】\n");
    out.push_str(&format!("  {}\n\n", overall_assessment(profile)));

    out.push_str("【分域对比（当前 vs 参考）】\n");
    for d in &profile.domains {
        out.push_str(&format_domain_line(d));
    }
    out.push('\n');

    out.push_str("【24h 三队轮休加权】\n");
    out.push_str(&format!(
        "  贸易 {:.2}（参考 {:.2}）· 制造 {:.1}（参考 {:.1}）· 发电 {:.1}（参考 {:.1}）\n\n",
        profile.rotation.daily_trade,
        profile.baseline_rotation.daily_trade,
        profile.rotation.daily_manu,
        profile.baseline_rotation.daily_manu,
        profile.rotation.daily_power,
        profile.baseline_rotation.daily_power,
    ));

    if !profile.actions.is_empty() {
        out.push_str("【提升建议】\n");
        for action in &profile.actions {
            let tag = match action.kind {
                ActionKind::PromoteTierUp => action
                    .tier_up_requirement
                    .as_deref()
                    .unwrap_or("tier_up"),
                ActionKind::Acquire => "获取",
                ActionKind::Note => "提示",
            };
            out.push_str(&format!(
                "  [{}] [{}] {}\n",
                action.priority, tag, action.message
            ));
        }
        out.push('\n');
    } else {
        out.push_str("【提升建议】\n  各域与参考接近，暂无显著缺口。\n\n");
    }

    if !profile.narration_hints.is_empty() {
        out.push_str("【补充说明】\n");
        for hint in &profile.narration_hints {
            out.push_str(&format!("  · {}\n", hint));
        }
    }

    out
}

fn overall_assessment(profile: &BoxProfile) -> String {
    let critical: Vec<_> = profile
        .domains
        .iter()
        .filter(|d| d.severity == GapSeverity::Critical)
        .map(|d| d.label)
        .collect();
    let warn: Vec<_> = profile
        .domains
        .iter()
        .filter(|d| d.severity == GapSeverity::Warn)
        .map(|d| d.label)
        .collect();

    if critical.is_empty() && warn.is_empty() {
        return "与参考基准整体接近，基建效率处于良好水平。".to_string();
    }

    let mut parts = Vec::new();
    if !critical.is_empty() {
        parts.push(format!("{}明显偏低", critical.join("、")));
    }
    if !warn.is_empty() {
        parts.push(format!("{}略有差距", warn.join("、")));
    }

    let mut s = parts.join("；") + "。";

    if profile.flags.iter().any(|f| f == "trade_gold_ok") {
        s.push_str("赤金贸易架构基本正确。");
    }
    if profile.flags.iter().any(|f| f == "manu_bottleneck") {
        s.push_str("建议优先投入制造相关练度。");
    }

    s
}

fn format_domain_line(d: &DomainMetric) -> String {
    let pct = d.gap_ratio * 100.0;
    let mark = match d.severity {
        GapSeverity::Ok => "✓",
        GapSeverity::Warn => "△",
        GapSeverity::Critical => "⚠",
    };
    let combo = if d.current.operators.is_empty() {
        String::new()
    } else {
        format!(" [{}]", d.current.operators.join("+"))
    };
    let extra = match (d.current.trade_pct, d.current.gold_pct) {
        (Some(t), Some(g)) => format!(" trade={:.0}% gold={:.0}%", t, g),
        (Some(t), None) => format!(" trade={:.0}%", t),
        _ => String::new(),
    };
    format!(
        "  {} {}  {:.2} → 参考 {:.2}（{:+.0}%）{combo}{extra}\n",
        mark, d.label, d.current.score, d.baseline.score, pct,
    )
}
