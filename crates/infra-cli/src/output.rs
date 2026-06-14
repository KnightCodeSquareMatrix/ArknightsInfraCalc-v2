use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use csv::Writer;
use infra_core::layout::BaseAssignment;
use infra_core::pool::PoolSkip;
use infra_core::schedule::{
    BaseRotationReport, BaseShiftRole, TeamLabel, TeamRotationReport, TradeRotationReport,
    TradeStationRole,
};
use infra_core::search::{ManuSearchHit, ManuSearchReport, TradeSearchHit, TradeSearchReport};
use infra_core::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Csv,
    Text,
    Json,
}

#[derive(Debug, Clone)]
pub struct OutputOptions {
    pub format: OutputFormat,
    pub path: Option<PathBuf>,
}

impl OutputOptions {
    pub fn from_args(args: &[String]) -> Self {
        let format = if args.iter().any(|a| a == "--json") {
            OutputFormat::Json
        } else if args.iter().any(|a| a == "--text")
            || arg_value(args, "--format").as_deref() == Some("text")
        {
            OutputFormat::Text
        } else {
            OutputFormat::Csv
        };
        Self {
            format,
            path: arg_value(args, "--output")
                .or_else(|| arg_value(args, "-o"))
                .map(PathBuf::from),
        }
    }
}

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].clone())
}

pub fn join_ops(names: &[String]) -> String {
    names.join("+")
}

fn csv_writer(path: Option<&Path>) -> Result<Writer<Box<dyn Write>>, Error> {
    if let Some(path) = path {
        let mut file = File::create(path)?;
        file.write_all(&[0xEF, 0xBB, 0xBF])?;
        Ok(Writer::from_writer(Box::new(file) as Box<dyn Write>))
    } else {
        Ok(Writer::from_writer(Box::new(io::stdout()) as Box<dyn Write>))
    }
}

fn flush_csv(mut wtr: Writer<Box<dyn Write>>) -> Result<(), Error> {
    wtr.flush()?;
    Ok(())
}

fn role_label(role: TradeStationRole) -> &'static str {
    match role {
        TradeStationRole::Witch => "巫恋核",
        TradeStationRole::Closure => "可露希尔",
        TradeStationRole::Docus => "但书",
        TradeStationRole::JieE0Lead => "孑带队",
        TradeStationRole::Plain => "常规",
    }
}

fn skip_reason_label(reason: &PoolSkip) -> String {
    match reason {
        PoolSkip::NoTradeBinding => "无技能绑定".to_string(),
        PoolSkip::UnmodeledBuff(id) => format!("未建模:{id}"),
    }
}

fn section_label(section: &str) -> &str {
    match section {
        "summary" => "汇总",
        "skipped" => "跳过",
        "meta" => "元信息",
        "result" => "结果",
        "gold_line" => "赤金线",
        "originium_line" => "源石线",
        "pool" => "干员池",
        "trade" => "贸易",
        "trade_gold_line" => "贸易赤金线",
        "trade_originium_line" => "贸易源石线",
        "manufacture" => "制造",
        "manu_gold_line" => "制造赤金线",
        "manu_exp_line" => "制造经验线",
        "station" => "站点",
        "bench" => "基准测试",
        other => other,
    }
}

fn domain_label(domain: &str) -> &str {
    match domain {
        "trade" => "贸易站",
        "manufacture" => "制造站",
        other => other,
    }
}

fn facility_label(facility: &str) -> &str {
    match facility {
        "trade" => "贸易站",
        "manufacture" => "制造站",
        other => other,
    }
}

fn elapsed_secs(d: Duration) -> String {
    format!("{:.3}", d.as_secs_f64())
}

// ── pool ────────────────────────────────────────────────────────────────────

pub struct PoolSummary<'a> {
    pub facility: &'a str,
    pub operbox: Option<&'a str>,
    pub owned: Option<usize>,
    pub roster_size: usize,
    pub ready: usize,
    pub skipped: usize,
    pub combinations_3: u64,
}

pub fn emit_pool(
    opts: &OutputOptions,
    summary: &PoolSummary<'_>,
    skipped: &[(String, u8, PoolSkip)],
) -> Result<(), Error> {
    match opts.format {
        OutputFormat::Csv => write_pool_csv(opts.path.as_deref(), summary, skipped),
        OutputFormat::Text => write_pool_text(summary, skipped),
        OutputFormat::Json => Err(Error::msg("pool does not support --json; use --format csv")),
    }
}

fn write_pool_csv(
    path: Option<&Path>,
    summary: &PoolSummary<'_>,
    skipped: &[(String, u8, PoolSkip)],
) -> Result<(), Error> {
    let mut wtr = csv_writer(path)?;
    wtr.write_record([
        "区块",
        "设施",
        "练度盒",
        "拥有数",
        "名单人数",
        "可求解",
        "跳过数",
        "三人组合数",
        "干员",
        "精英化",
        "跳过原因",
    ])?;
    wtr.write_record([
        section_label("summary"),
        facility_label(summary.facility),
        summary.operbox.unwrap_or(""),
        &summary.owned.map(|n| n.to_string()).unwrap_or_default(),
        &summary.roster_size.to_string(),
        &summary.ready.to_string(),
        &summary.skipped.to_string(),
        &summary.combinations_3.to_string(),
        "",
        "",
        "",
    ])?;
    for (name, elite, reason) in skipped {
        wtr.write_record([
            section_label("skipped"),
            facility_label(summary.facility),
            "",
            "",
            "",
            "",
            "",
            "",
            name,
            &elite.to_string(),
            &skip_reason_label(reason),
        ])?;
    }
    flush_csv(wtr)
}

fn write_pool_text(summary: &PoolSummary<'_>, skipped: &[(String, u8, PoolSkip)]) -> Result<(), Error> {
    if let (Some(owned), Some(ob)) = (summary.owned, summary.operbox) {
        eprintln!(
            "operbox: {} owned={} {}_roster={}",
            ob, owned, summary.facility, summary.roster_size
        );
    }
    eprintln!(
        "{} pool: ready={} skipped={} C(ready,3)={}",
        summary.facility, summary.ready, summary.skipped, summary.combinations_3
    );
    for (name, elite, reason) in skipped {
        let detail = match reason {
            PoolSkip::NoTradeBinding => "no binding".to_string(),
            PoolSkip::UnmodeledBuff(id) => format!("unmodeled {id}"),
        };
        eprintln!("  skip   {name} e{elite}: {detail}");
    }
    Ok(())
}

// ── trade search ────────────────────────────────────────────────────────────

pub struct SearchMeta<'a> {
    pub operbox_owned: Option<usize>,
    pub trade_roster: Option<usize>,
    pub combinations: u64,
    pub evaluated: u64,
    pub elapsed: Duration,
    pub order_mode_label: &'a str,
}

pub fn emit_trade_search(
    opts: &OutputOptions,
    meta: &SearchMeta<'_>,
    report: &TradeSearchReport,
) -> Result<(), Error> {
    match opts.format {
        OutputFormat::Csv => write_trade_search_csv(opts.path.as_deref(), meta, report),
        OutputFormat::Text => write_trade_search_text(meta, report),
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(report)?);
            Ok(())
        }
    }
}

fn write_trade_search_csv(
    path: Option<&Path>,
    meta: &SearchMeta<'_>,
    report: &TradeSearchReport,
) -> Result<(), Error> {
    let mut wtr = csv_writer(path)?;
    wtr.write_record([
        "区块",
        "排名",
        "评分",
        "贸易效率%",
        "赤金效率%",
        "日贸易量",
        "日赤金消耗",
        "日固源岩",
        "产出倍率",
        "短路ID",
        "干员组合",
        "赤金线干员",
        "源石线干员",
        "组合总数",
        "评估数",
        "耗时秒",
        "订单模式",
        "拥有数",
        "贸易名单",
    ])?;
    wtr.write_record([
        section_label("meta"),
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        &meta.combinations.to_string(),
        &meta.evaluated.to_string(),
        &elapsed_secs(meta.elapsed),
        meta.order_mode_label,
        &meta.operbox_owned.map(|n| n.to_string()).unwrap_or_default(),
        &meta.trade_roster.map(|n| n.to_string()).unwrap_or_default(),
    ])?;
    for (i, hit) in report.top.iter().enumerate() {
        write_trade_hit_row(&mut wtr, section_label("result"), i + 1, hit)?;
    }
    if let Some(hit) = &report.gold_order_line {
        write_trade_hit_row(&mut wtr, section_label("gold_line"), 1, hit)?;
    }
    if let Some(hit) = &report.originium_order_line {
        write_trade_hit_row(&mut wtr, section_label("originium_line"), 1, hit)?;
    }
    flush_csv(wtr)
}

fn write_trade_hit_row(
    wtr: &mut Writer<Box<dyn Write>>,
    section: &str,
    rank: usize,
    hit: &TradeSearchHit,
) -> Result<(), Error> {
    wtr.write_record([
        section,
        &rank.to_string(),
        &format!("{:.3}", hit.score),
        &format!("{:.1}", hit.trade_pct),
        &format!("{:.1}", hit.gold_pct),
        &format!("{:.0}", hit.unit_trade_per_day),
        &format!("{:.1}", hit.unit_gold_per_day),
        &format!("{:.1}", hit.unit_originium_per_day),
        &format!("{:.3}", hit.output_multiplier),
        hit.shortcut.as_deref().unwrap_or(""),
        &join_ops(&hit.names),
        &join_ops(&hit.gold_names),
        &join_ops(&hit.originium_names),
        "",
        "",
        "",
        "",
        "",
        "",
    ])?;
    Ok(())
}

fn write_trade_search_text(meta: &SearchMeta<'_>, report: &TradeSearchReport) -> Result<(), Error> {
    if let (Some(owned), Some(roster)) = (meta.operbox_owned, meta.trade_roster) {
        eprintln!(
            "operbox: owned={} trade_roster={} ({})",
            owned, roster, meta.order_mode_label
        );
    }
    let rate = if meta.elapsed.as_secs_f64() > 0.0 {
        meta.evaluated as f64 / meta.elapsed.as_secs_f64()
    } else {
        0.0
    };
    eprintln!(
        "combinations={} evaluated={} elapsed={:.2?} ({:.0} eval/s)",
        meta.combinations, meta.evaluated, meta.elapsed, rate
    );
    for (i, hit) in report.top.iter().enumerate() {
        if !hit.gold_names.is_empty() || !hit.originium_names.is_empty() {
            eprintln!(
                "  #{:<2} composite={:.3} trade={:.1} gold={:.1}",
                i + 1,
                hit.score,
                hit.trade_pct,
                hit.gold_pct,
            );
            eprintln!("         gold_ops={:?}", hit.gold_names);
            eprintln!("         ori_ops={:?}", hit.originium_names);
        } else {
            eprintln!(
                "  #{:<2} score={:.3} trade={:.1} gold={:.1} unit={:.0} mult={:.3} shortcut={:?} ops={:?}",
                i + 1,
                hit.score,
                hit.trade_pct,
                hit.gold_pct,
                hit.unit_trade_per_day,
                hit.output_multiplier,
                hit.shortcut,
                hit.names
            );
        }
    }
    if let (Some(gold), Some(ori)) = (
        &report.gold_order_line,
        &report.originium_order_line,
    ) {
        eprintln!(
            "  (split) gold_line score={:.3} {:?}  ori_line score={:.3} {:?} ori/day={:.1}",
            gold.score,
            gold.names,
            ori.score,
            ori.names,
            ori.unit_originium_per_day,
        );
    }
    Ok(())
}

// ── bench ───────────────────────────────────────────────────────────────────

pub struct BenchMeta<'a> {
    pub operbox: &'a str,
    pub owned: usize,
    pub layout: Option<&'a str>,
    pub manufacture_scenario: &'a str,
    pub trade_order_mode: &'a str,
}

pub fn emit_bench(
    opts: &OutputOptions,
    meta: &BenchMeta<'_>,
    trade_pool: PoolSummary<'_>,
    trade_report: &TradeSearchReport,
    manu_pool: PoolSummary<'_>,
    manu_report: &ManuSearchReport,
) -> Result<(), Error> {
    match opts.format {
        OutputFormat::Csv => write_bench_csv(
            opts.path.as_deref(),
            meta,
            &trade_pool,
            trade_report,
            &manu_pool,
            manu_report,
        ),
        OutputFormat::Text => write_bench_text(meta, &trade_pool, trade_report, &manu_pool, manu_report),
        OutputFormat::Json => Err(Error::msg("bench does not support --json; use --format csv")),
    }
}

fn write_bench_csv(
    path: Option<&Path>,
    meta: &BenchMeta<'_>,
    trade_pool: &PoolSummary<'_>,
    trade_report: &TradeSearchReport,
    manu_pool: &PoolSummary<'_>,
    manu_report: &ManuSearchReport,
) -> Result<(), Error> {
    let mut wtr = csv_writer(path)?;
    wtr.write_record([
        "区块",
        "域",
        "排名",
        "评分",
        "贸易效率%",
        "赤金效率%",
        "日固源岩",
        "综合分",
        "赤金产出%",
        "赤金仓储",
        "经验产出%",
        "经验仓储",
        "干员组合",
        "赤金线干员",
        "源石线干员",
        "经验线干员",
        "短路ID",
        "组合总数",
        "评估数",
        "耗时秒",
        "练度盒",
        "拥有数",
        "场景",
        "池可求解",
        "池跳过数",
        "池三人组合数",
    ])?;
    wtr.write_record([
        section_label("meta"),
        section_label("bench"),
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        meta.operbox,
        &meta.owned.to_string(),
        &format!(
            "layout:{};trade:{};manu:{}",
            meta.layout.unwrap_or("search_baseline"),
            meta.trade_order_mode,
            meta.manufacture_scenario
        ),
        "",
        "",
        "",
    ])?;
    wtr.write_record([
        section_label("pool"),
        domain_label("trade"),
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        &trade_pool.ready.to_string(),
        &trade_pool.skipped.to_string(),
        &trade_pool.combinations_3.to_string(),
    ])?;
    for (i, hit) in trade_report.top.iter().enumerate() {
        write_bench_trade_row(&mut wtr, section_label("trade"), i + 1, hit, trade_report)?;
    }
    if let Some(hit) = &trade_report.gold_order_line {
        write_bench_trade_row(&mut wtr, section_label("trade_gold_line"), 1, hit, trade_report)?;
    }
    if let Some(hit) = &trade_report.originium_order_line {
        write_bench_trade_row(&mut wtr, section_label("trade_originium_line"), 1, hit, trade_report)?;
    }
    wtr.write_record([
        section_label("pool"),
        domain_label("manufacture"),
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        &manu_pool.ready.to_string(),
        &manu_pool.skipped.to_string(),
        &manu_pool.combinations_3.to_string(),
    ])?;
    for (i, hit) in manu_report.top.iter().enumerate() {
        write_bench_manu_row(&mut wtr, section_label("manufacture"), i + 1, hit, manu_report)?;
    }
    if let Some(hit) = &manu_report.gold_line {
        write_bench_manu_row(&mut wtr, section_label("manu_gold_line"), 1, hit, manu_report)?;
    }
    if let Some(hit) = &manu_report.battle_record_line {
        write_bench_manu_row(&mut wtr, section_label("manu_exp_line"), 1, hit, manu_report)?;
    }
    flush_csv(wtr)
}

fn write_bench_trade_row(
    wtr: &mut Writer<Box<dyn Write>>,
    section: &str,
    rank: usize,
    hit: &TradeSearchHit,
    report: &TradeSearchReport,
) -> Result<(), Error> {
    wtr.write_record([
        section,
        domain_label("trade"),
        &rank.to_string(),
        &format!("{:.3}", hit.score),
        &format!("{:.1}", hit.trade_pct),
        &format!("{:.1}", hit.gold_pct),
        &format!("{:.1}", hit.unit_originium_per_day),
        "",
        "",
        "",
        "",
        "",
        &join_ops(&hit.names),
        &join_ops(&hit.gold_names),
        &join_ops(&hit.originium_names),
        "",
        hit.shortcut.as_deref().unwrap_or(""),
        &report.combinations.to_string(),
        &report.evaluated.to_string(),
        &elapsed_secs(report.elapsed),
        "",
        "",
        "",
        "",
        "",
        "",
    ])?;
    Ok(())
}

fn write_bench_manu_row(
    wtr: &mut Writer<Box<dyn Write>>,
    section: &str,
    rank: usize,
    hit: &ManuSearchHit,
    report: &ManuSearchReport,
) -> Result<(), Error> {
    wtr.write_record([
        section,
        domain_label("manufacture"),
        &rank.to_string(),
        "",
        "",
        "",
        "",
        &format!("{:.1}", hit.composite_score),
        &format!("{:.1}", hit.per_station.gold),
        &hit.storage.gold.to_string(),
        &format!("{:.1}", hit.per_station.battle_record),
        &hit.storage.battle_record.to_string(),
        &join_ops(&hit.names),
        &join_ops(&hit.gold_names),
        "",
        &join_ops(&hit.battle_record_names),
        "",
        &report.combinations.to_string(),
        &report.evaluated.to_string(),
        &elapsed_secs(report.elapsed),
        "",
        "",
        "",
        "",
        "",
        "",
    ])?;
    Ok(())
}

fn write_bench_text(
    meta: &BenchMeta<'_>,
    trade_pool: &PoolSummary<'_>,
    trade_report: &TradeSearchReport,
    manu_pool: &PoolSummary<'_>,
    manu_report: &ManuSearchReport,
) -> Result<(), Error> {
    eprintln!(
        "bench operbox={} owned={} layout={} manufacture={}",
        meta.operbox,
        meta.owned,
        meta.layout.unwrap_or("search_baseline"),
        meta.manufacture_scenario
    );
    eprintln!(
        "trade pool: ready={} skipped={} C(ready,3)={}",
        trade_pool.ready, trade_pool.skipped, trade_pool.combinations_3
    );
    let trade_meta = SearchMeta {
        operbox_owned: None,
        trade_roster: None,
        combinations: trade_report.combinations,
        evaluated: trade_report.evaluated,
        elapsed: trade_report.elapsed,
        order_mode_label: meta.trade_order_mode,
    };
    write_trade_search_text(&trade_meta, trade_report)?;
    eprintln!(
        "manufacture pool: ready={} skipped={} C(ready,3)={}",
        manu_pool.ready, manu_pool.skipped, manu_pool.combinations_3
    );
    eprintln!(
        "manufacture search ({}): combos={} evaluated={} elapsed={:.2?}",
        meta.manufacture_scenario,
        manu_report.combinations,
        manu_report.evaluated,
        manu_report.elapsed
    );
    for (i, hit) in manu_report.top.iter().enumerate() {
        if !hit.gold_names.is_empty() || !hit.battle_record_names.is_empty() {
            eprintln!(
                "  manu #{:<2} composite={:.1} gold={:.1}% stor={} exp={:.1}% stor={}",
                i + 1,
                hit.composite_score,
                hit.per_station.gold,
                hit.storage.gold,
                hit.per_station.battle_record,
                hit.storage.battle_record,
            );
            eprintln!("         gold_ops={:?}", hit.gold_names);
            eprintln!("         exp_ops={:?}", hit.battle_record_names);
        } else {
            eprintln!(
                "  manu #{:<2} composite={:.1} gold={:.1}% stor={} exp={:.1}% stor={} ops={:?}",
                i + 1,
                hit.composite_score,
                hit.per_station.gold,
                hit.storage.gold,
                hit.per_station.battle_record,
                hit.storage.battle_record,
                hit.names
            );
        }
    }
    if let (Some(gold), Some(br)) = (&manu_report.gold_line, &manu_report.battle_record_line) {
        eprintln!(
            "  (split) gold_line={:.1}% {:?}  exp_line={:.1}% {:?}",
            gold.composite_score,
            gold.names,
            br.composite_score,
            br.names
        );
    }
    Ok(())
}

// ── schedule ────────────────────────────────────────────────────────────────

pub fn emit_schedule(
    opts: &OutputOptions,
    owned: usize,
    report: &TradeRotationReport,
) -> Result<(), Error> {
    match opts.format {
        OutputFormat::Csv => write_schedule_csv(opts.path.as_deref(), owned, report),
        OutputFormat::Text => write_schedule_text(owned, report),
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(report)?);
            Ok(())
        }
    }
}

fn write_schedule_csv(
    path: Option<&Path>,
    owned: usize,
    report: &TradeRotationReport,
) -> Result<(), Error> {
    let mut wtr = csv_writer(path)?;
    wtr.write_record([
        "区块",
        "班次",
        "班次评分",
        "复用班次",
        "本班干员",
        "贸易站",
        "角色",
        "评分",
        "贸易效率%",
        "赤金效率%",
        "日贸易量",
        "产出倍率",
        "短路ID",
        "干员组合",
        "拥有数",
        "耗时秒",
    ])?;
    wtr.write_record([
        section_label("meta"),
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        &owned.to_string(),
        &elapsed_secs(report.elapsed),
    ])?;
    for shift in &report.shifts {
        let workers = join_ops(&shift.workers);
        let reused = shift
            .reused_from_shift
            .map(|i| (i + 1).to_string())
            .unwrap_or_default();
        for station in &shift.stations {
            let hit = &station.hit;
            wtr.write_record([
                section_label("station"),
                &(shift.index + 1).to_string(),
                &format!("{:.3}", shift.total_score),
                &reused,
                &workers,
                &(station.station_index + 1).to_string(),
                role_label(station.role),
                &format!("{:.3}", hit.score),
                &format!("{:.1}", hit.trade_pct),
                &format!("{:.1}", hit.gold_pct),
                &format!("{:.0}", hit.unit_trade_per_day),
                &format!("{:.3}", hit.output_multiplier),
                hit.shortcut.as_deref().unwrap_or(""),
                &join_ops(&hit.names),
                "",
                "",
            ])?;
        }
    }
    flush_csv(wtr)
}

// ── trade yield ─────────────────────────────────────────────────────────────

pub struct TradeYieldRow<'a> {
    pub fixture: &'a str,
    pub level: u8,
    pub shift_hours: f64,
    pub paper_eff_pct: f64,
    pub shortcut: Option<&'a str>,
    pub unit_trade: f64,
    pub unit_gsl_gold: f64,
    pub multiplier: f64,
    pub daily_trade: f64,
    pub daily_gold: f64,
    pub drone_trade: f64,
    pub pre_shortcut: f64,
}

pub fn emit_trade_yield(opts: &OutputOptions, row: &TradeYieldRow<'_>) -> Result<(), Error> {
    match opts.format {
        OutputFormat::Csv => write_trade_yield_csv(opts.path.as_deref(), row),
        OutputFormat::Text => write_trade_yield_text(row),
        OutputFormat::Json => Err(Error::msg("trade yield does not support --json; use --format csv")),
    }
}

fn write_trade_yield_csv(path: Option<&Path>, row: &TradeYieldRow<'_>) -> Result<(), Error> {
    let mut wtr = csv_writer(path)?;
    wtr.write_record([
        "夹具",
        "贸易站等级",
        "上班时长",
        "纸面效率%",
        "短路ID",
        "日贸易量",
        "日GSL赤金",
        "产出倍率",
        "日产贸易",
        "日产赤金",
        "无人机贸易",
        "短路前效率%",
    ])?;
    wtr.write_record([
        row.fixture,
        &row.level.to_string(),
        &format!("{:.1}", row.shift_hours),
        &format!("{:.1}", row.paper_eff_pct),
        row.shortcut.unwrap_or(""),
        &format!("{:.1}", row.unit_trade),
        &format!("{:.1}", row.unit_gsl_gold),
        &format!("{:.3}", row.multiplier),
        &format!("{:.0}", row.daily_trade),
        &format!("{:.1}", row.daily_gold),
        &format!("{:.0}", row.drone_trade),
        &format!("{:.1}", row.pre_shortcut),
    ])?;
    flush_csv(wtr)
}

fn write_trade_yield_text(row: &TradeYieldRow<'_>) -> Result<(), Error> {
    eprintln!(
        "fixture={} level={} shift={}h paper_eff={:.1}% shortcut={:?}",
        row.fixture, row.level, row.shift_hours, row.paper_eff_pct, row.shortcut
    );
    eprintln!(
        "  unit_trade={:.1} unit_gsl_gold={:.1} mult={:.3}",
        row.unit_trade, row.unit_gsl_gold, row.multiplier
    );
    eprintln!(
        "  daily_trade={:.0} daily_gold={:.1} drone_trade={:.0} pre_shortcut={:.1}",
        row.daily_trade, row.daily_gold, row.drone_trade, row.pre_shortcut
    );
    Ok(())
}

// ── base rotation (full base A-B-A) ─────────────────────────────────────────

pub fn emit_base_rotation(
    opts: &OutputOptions,
    layout: &str,
    operbox: &str,
    owned: usize,
    report: &BaseRotationReport,
) -> Result<(), Error> {
    match opts.format {
        OutputFormat::Csv => write_base_rotation_csv(opts.path.as_deref(), layout, operbox, owned, report),
        OutputFormat::Text => write_base_rotation_text(layout, operbox, owned, report),
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(report)?);
            Ok(())
        }
    }
}

pub fn print_base_rotation_text(
    layout: &str,
    operbox: &str,
    owned: usize,
    report: &BaseRotationReport,
) -> Result<(), Error> {
    write_base_rotation_text(layout, operbox, owned, report)
}

fn write_base_rotation_csv(
    path: Option<&Path>,
    layout: &str,
    operbox: &str,
    owned: usize,
    report: &BaseRotationReport,
) -> Result<(), Error> {
    let mut wtr = csv_writer(path)?;
    wtr.write_record([
        "layout",
        "operbox",
        "owned",
        "班次",
        "班型",
        "复用班次",
        "贸易分",
        "制造prod合计",
        "轮换人数",
        "轮换干员",
        "耗时秒",
    ])?;
    for shift in &report.shifts {
        let reused = shift
            .reused_from_shift
            .map(|i| (i + 1).to_string())
            .unwrap_or_default();
        wtr.write_record([
            layout,
            operbox,
            &owned.to_string(),
            &(shift.index + 1).to_string(),
            shift_role_label(shift.role),
            &reused,
            &format!("{:.3}", shift.scores.trade_score),
            &format!("{:.1}", shift.scores.manu_prod_sum),
            &shift.rotating_workers.len().to_string(),
            &shift.rotating_workers.join("|"),
            &elapsed_secs(report.elapsed),
        ])?;
    }
    flush_csv(wtr)
}

fn shift_role_label(role: BaseShiftRole) -> &'static str {
    match role {
        BaseShiftRole::Peak => "peak",
        BaseShiftRole::Recovery => "recovery",
    }
}

fn team_label(label: TeamLabel) -> &'static str {
    match label {
        TeamLabel::Alpha => "α",
        TeamLabel::Beta => "β",
        TeamLabel::Gamma => "γ",
    }
}

fn team_by_label<'a>(teams: &'a [infra_core::schedule::TeamAssignment], label: TeamLabel) -> &'a infra_core::schedule::TeamAssignment {
    teams
        .iter()
        .find(|t| t.label == label)
        .expect("team label in report")
}

fn assignment_room_ops<'a>(
    assignment: &'a BaseAssignment,
    room_id: &str,
) -> Option<Vec<&'a str>> {
    assignment
        .rooms
        .iter()
        .find(|r| r.room_id.0 == room_id)
        .filter(|r| !r.operators.is_empty())
        .map(|r| r.operators.iter().map(|o| o.name.as_str()).collect())
}

fn shift_room_ops<'a>(
    shift: &'a infra_core::schedule::TeamShiftResult,
    room_id: &str,
) -> Option<Vec<&'a str>> {
    assignment_room_ops(&shift.assignment, room_id)
}

/// 某设施本班由哪支队伍在岗：取其在岗干员所属队伍（同设施同队）。
fn room_active_team(
    op_team: &std::collections::HashMap<String, TeamLabel>,
    ops: &[&str],
) -> Option<TeamLabel> {
    ops.iter().find_map(|name| op_team.get(*name).copied())
}

fn room_display_name(room_id: &str) -> String {
    if let Some(n) = room_id.strip_prefix("trade_") {
        return format!("贸易站{n}");
    }
    if let Some(n) = room_id.strip_prefix("manu_") {
        return format!("制造站{n}");
    }
    if let Some(n) = room_id.strip_prefix("power_") {
        return format!("发电站{n}");
    }
    if let Some(n) = room_id.strip_prefix("dorm_") {
        return format!("宿舍{n}");
    }
    if let Some(n) = room_id.strip_prefix("office_") {
        return format!("办公室{n}");
    }
    if room_id == "office" {
        return "办公室".to_string();
    }
    match room_id {
        "control" => "中枢".to_string(),
        "meeting" => "会客室".to_string(),
        other => other.to_string(),
    }
}

/// 排班表按设施编号顺序列出（与蓝图 trade_1→…→control→办公室→会客室→宿舍 一致）。
const SHIFT_STATION_ORDER: &[&str] = &[
    "trade_1",
    "trade_2",
    "manu_1",
    "manu_2",
    "manu_3",
    "manu_4",
    "power_1",
    "power_2",
    "power_3",
    "control",
    "office_1",
    "meeting",
    "dorm_1",
    "dorm_2",
    "dorm_3",
    "dorm_4",
];

fn format_shift_station_line(
    shift: &infra_core::schedule::TeamShiftResult,
    op_team: &std::collections::HashMap<String, TeamLabel>,
    room_id: &str,
) -> String {
    let label = room_display_name(room_id);
    if let Some(ops) = shift_room_ops(shift, room_id) {
        let names = ops.join(", ");
        if let Some(team) = room_active_team(op_team, &ops) {
            format!("  {label}: {names}（{t}队）", t = team_label(team))
        } else if matches!(room_id, "control" | "meeting" | "office" | "office_1" | "office_2")
            || room_id.starts_with("office_")
            || room_id.starts_with("dorm_")
        {
            format!("  {label}: {names}（共享）")
        } else {
            format!("  {label}: {names}")
        }
    } else {
        format!("  {label}: —")
    }
}

pub fn emit_team_rotation(
    opts: &OutputOptions,
    layout: &str,
    operbox: &str,
    owned: usize,
    report: &TeamRotationReport,
) -> Result<(), Error> {
    match opts.format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(report)?);
            Ok(())
        }
        OutputFormat::Csv => write_team_rotation_csv(opts.path.as_deref(), layout, operbox, owned, report),
        OutputFormat::Text => write_team_rotation_text(layout, operbox, owned, report),
    }
}

pub fn print_team_rotation_text(
    layout: &str,
    operbox: &str,
    owned: usize,
    report: &TeamRotationReport,
) -> Result<(), Error> {
    write_team_rotation_text(layout, operbox, owned, report)
}

fn write_team_rotation_csv(
    path: Option<&Path>,
    layout: &str,
    operbox: &str,
    owned: usize,
    report: &TeamRotationReport,
) -> Result<(), Error> {
    let mut wtr = csv_writer(path)?;
    wtr.write_record([
        "layout",
        "operbox",
        "owned",
        "班次",
        "时长h",
        "上岗队",
        "休息队",
        "贸易分",
        "制造prod合计",
        "发电充能%合计",
        "贸易加权",
        "制造加权",
        "发电加权",
        "耗时秒",
    ])?;
    for shift in &report.shifts {
        let active: Vec<&str> = shift.active_teams.iter().map(|t| team_label(*t)).collect();
        wtr.write_record([
            layout,
            operbox,
            &owned.to_string(),
            &(shift.index + 1).to_string(),
            &format!("{:.0}", shift.duration_hours),
            &active.join("+"),
            team_label(shift.resting_team),
            &format!("{:.3}", shift.scores.trade_score),
            &format!("{:.1}", shift.scores.manu_prod_sum),
            &format!("{:.1}", shift.scores.power_charge_sum),
            &format!("{:.3}", shift.weighted_trade),
            &format!("{:.1}", shift.weighted_manu),
            &format!("{:.1}", shift.weighted_power),
            &elapsed_secs(report.elapsed),
        ])?;
    }
    flush_csv(wtr)
}

fn write_team_rotation_text(
    layout: &str,
    operbox: &str,
    owned: usize,
    report: &TeamRotationReport,
) -> Result<(), Error> {
    let op_team = infra_core::schedule::operator_team_map(report);

    eprintln!(
        "αβγ team rotation: layout={} operbox={} owned={} elapsed={:.2?}",
        layout, operbox, owned, report.elapsed
    );
    eprintln!(
        "  每日加权产出（三类分开评分）: 贸易={:.3}  制造={:.1}  发电={:.1}",
        report.daily.trade, report.daily.manu, report.daily.power
    );

    eprintln!("\n--- 三队花名册（每班两队上岗、一队休息；设施始终满编不空转）---");
    for team in &report.teams {
        eprintln!(
            "  {} 队 ({} 人): {}",
            team_label(team.label),
            team.operators.len(),
            team.operators.join(", ")
        );
    }

    eprintln!("\n--- 轮换一览 ---");
    eprintln!("  班次   时长   上班队        休息队");
    for shift in &report.shifts {
        let active: Vec<&str> = shift.active_teams.iter().map(|t| team_label(*t)).collect();
        eprintln!(
            "  {:>5}  {:>3.0}h   {:<12}  {}",
            format!("shift{}", shift.index + 1),
            shift.duration_hours,
            active.join("+"),
            team_label(shift.resting_team),
        );
    }

    for shift in &report.shifts {
        let active_names: Vec<&str> = shift.active_teams.iter().map(|t| team_label(*t)).collect();
        let resting = team_by_label(&report.teams, shift.resting_team);
        eprintln!(
            "\n======== Shift {} · {:.0}h · 上班 {} · 休息 {} ========",
            shift.index + 1,
            shift.duration_hours,
            active_names.join("+"),
            team_label(shift.resting_team),
        );
        eprintln!(
            "  休息干员（{}队）: {}",
            team_label(resting.label),
            resting.operators.join(", ")
        );
        eprintln!(
            "  分数(原值): trade={:.3}  manu={:.1}  power={:.1}",
            shift.scores.trade_score, shift.scores.manu_prod_sum, shift.scores.power_charge_sum,
        );
        eprintln!(
            "  分数(按{:.0}h加权): trade={:.3}  manu={:.1}  power={:.1}",
            shift.duration_hours, shift.weighted_trade, shift.weighted_manu, shift.weighted_power,
        );

        eprintln!("\n  【各设施上岗情况】");
        for room_id in SHIFT_STATION_ORDER {
            eprintln!("{}", format_shift_station_line(shift, &op_team, room_id));
        }
    }

    eprintln!(
        "\n每日加权产出（三类分开，12h×αβ + 6h×βγ + 6h×γα）: 贸易={:.3}  制造={:.1}  发电={:.1}",
        report.daily.trade, report.daily.manu, report.daily.power
    );
    Ok(())
}

fn write_base_rotation_text(
    layout: &str,
    operbox: &str,
    owned: usize,
    report: &BaseRotationReport,
) -> Result<(), Error> {
    eprintln!(
        "base rotation A-B-A: layout={} operbox={} owned={} elapsed={:.2?}",
        layout, operbox, owned, report.elapsed
    );
    for shift in &report.shifts {
        let reuse = shift
            .reused_from_shift
            .map(|i| format!(" (reuse shift {})", i + 1))
            .unwrap_or_default();
        eprintln!(
            "\n=== Shift {} {:?}{} trade={:.3} manu={:.1} rotating={} ===",
            shift.index + 1,
            shift.role,
            reuse,
            shift.scores.trade_score,
            shift.scores.manu_prod_sum,
            shift.rotating_workers.len()
        );
        eprintln!("  rotating_workers: {:?}", shift.rotating_workers);
        eprintln!("  【各设施上岗情况】");
        for room_id in SHIFT_STATION_ORDER {
            if let Some(ops) = assignment_room_ops(&shift.assignment, room_id) {
                let names = ops.join(", ");
                eprintln!("  {}: {}", room_display_name(room_id), names);
            }
        }
        for room in &shift.assignment.rooms {
            if room.operators.is_empty() {
                continue;
            }
            if SHIFT_STATION_ORDER.contains(&room.room_id.0.as_str()) {
                continue;
            }
            let names: Vec<_> = room.operators.iter().map(|o| o.name.as_str()).collect();
            eprintln!("  {}: {}", room_display_name(&room.room_id.0), names.join(", "));
        }
    }
    let avg_trade: f64 = report.shifts.iter().map(|s| s.scores.trade_score).sum::<f64>()
        / report.shifts.len() as f64;
    let avg_manu: f64 = report.shifts.iter().map(|s| s.scores.manu_prod_sum).sum::<f64>()
        / report.shifts.len() as f64;
    eprintln!(
        "\n3-shift avg: trade={:.3} manu={:.1}",
        avg_trade, avg_manu
    );
    Ok(())
}

fn write_schedule_text(owned: usize, report: &TradeRotationReport) -> Result<(), Error> {
    eprintln!(
        "trade rotation A-B-A: owned={} elapsed={:.2?}",
        owned, report.elapsed
    );
    for shift in &report.shifts {
        let reuse = shift
            .reused_from_shift
            .map(|i| format!(" (reuse shift {})", i + 1))
            .unwrap_or_default();
        eprintln!(
            "\n=== Shift {} score={:.3}{} ===",
            shift.index + 1,
            shift.total_score,
            reuse
        );
        eprintln!("  workers: {:?}", shift.workers);
        for station in &shift.stations {
            let hit = &station.hit;
            eprintln!(
                "  trade_{} role={:?} score={:.3} trade={:.1} gold={:.1} unit_trade={:.0} mult={:.3} shortcut={:?} ops={:?}",
                station.station_index + 1,
                station.role,
                hit.score,
                hit.trade_pct,
                hit.gold_pct,
                hit.unit_trade_per_day,
                hit.output_multiplier,
                hit.shortcut,
                hit.names
            );
        }
    }
    Ok(())
}
