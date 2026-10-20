use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use csv::Writer;
use infra_core::box_profile::{render_box_profile_narrative, BoxProfile};
use infra_core::layout::{BaseAssignment, BaseBlueprint};
use infra_core::pool::PoolSkip;
use infra_core::schedule::{TeamLabel, TeamRotationReport};
use infra_core::search::{ManuSearchHit, ManuSearchReport, TradeSearchHit, TradeSearchReport};
use infra_core::Efficiency;
use infra_core::Error;
use serde::Serialize;

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
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}

pub fn join_ops(names: &[String]) -> String {
    names.join("+")
}

pub fn emit_debug_json(value: &impl Serialize, pretty: bool) -> Result<(), Error> {
    if pretty {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        println!("{}", serde_json::to_string(value)?);
    }
    Ok(())
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

fn skip_reason_label(reason: &PoolSkip) -> String {
    match reason {
        PoolSkip::NoTradeBinding => "无技能绑定".to_string(),
        PoolSkip::UnmodeledBuff(id) => format!("未建模:{id}"),
        PoolSkip::ExcludedMechanic(id) => format!("排除机制:{id}"),
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
        "manufacture_gold_line" => "制造赤金线",
        "manufacture_battle_record_line" => "制造经验线",
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

#[derive(serde::Serialize)]
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

fn write_pool_text(
    summary: &PoolSummary<'_>,
    skipped: &[(String, u8, PoolSkip)],
) -> Result<(), Error> {
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
            PoolSkip::ExcludedMechanic(id) => format!("excluded mechanic {id}"),
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
        "最终效率",
        "机制等效效率",
        "单位产出倍率",
        "日贸易量",
        "日赤金消耗",
        "日固源岩",
        "规则ID",
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
        &meta.combinations.to_string(),
        &meta.evaluated.to_string(),
        &elapsed_secs(meta.elapsed),
        meta.order_mode_label,
        &meta
            .operbox_owned
            .map(|n| n.to_string())
            .unwrap_or_default(),
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
        &hit.final_efficiency.to_string(),
        &hit.mechanic_equivalent_efficiency.to_string(),
        &hit.breakdown
            .as_ref()
            .map(|b| b.unit_output_multiplier.to_string())
            .unwrap_or_default(),
        &format!("{:.0}", hit.unit_trade_per_day),
        &format!("{:.1}", hit.unit_gold_per_day),
        &format!("{:.1}", hit.unit_originium_per_day),
        hit.rule_id.as_deref().unwrap_or(""),
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
                "  #{:<2} total_efficiency={} mechanic_efficiency={}",
                i + 1,
                hit.final_efficiency,
                hit.mechanic_equivalent_efficiency,
            );
            eprintln!("         gold_ops={:?}", hit.gold_names);
            eprintln!("         ori_ops={:?}", hit.originium_names);
        } else {
            eprintln!(
        "  #{:<2} total_efficiency={} mechanic_efficiency={} unit_trade={:.0} unit_multiplier={} rule_id={:?} ops={:?}",
                i + 1,
                hit.final_efficiency,
                hit.mechanic_equivalent_efficiency,
                hit.unit_trade_per_day,
                hit.breakdown.as_ref().map(|b| b.unit_output_multiplier).unwrap_or_default(),
                hit.rule_id,
                hit.names
            );
        }
    }
    if let (Some(gold), Some(ori)) = (&report.gold_order_line, &report.originium_order_line) {
        eprintln!(
            "  (split) gold_line={} mechanic={} {:?}  ori_line={} {:?} ori/day={:.1}",
            gold.final_efficiency,
            gold.mechanic_equivalent_efficiency,
            gold.names,
            ori.final_efficiency,
            ori.names,
            ori.unit_originium_per_day,
        );
    }
    Ok(())
}

// ── bench ───────────────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
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
        OutputFormat::Text => {
            write_bench_text(meta, &trade_pool, trade_report, &manu_pool, manu_report)
        }
        OutputFormat::Json => {
            let value = serde_json::json!({
                "meta": meta,
                "trade": {
                    "pool": trade_pool,
                    "report": trade_report,
                },
                "manufacture": {
                    "pool": manu_pool,
                    "report": manu_report,
                },
            });
            println!("{}", serde_json::to_string_pretty(&value)?);
            Ok(())
        }
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
        "最终效率",
        "基础效率",
        "人头效率",
        "技能效率",
        "全局/中枢效率",
        "机制等效效率",
        "单位产出倍率",
        "赤金线效率",
        "赤金仓储",
        "经验线效率",
        "经验仓储",
        "干员组合",
        "赤金线干员",
        "源石线干员",
        "经验线干员",
        "规则ID",
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
    let mut meta_row = vec![String::new(); 28];
    meta_row[0] = section_label("meta").to_string();
    meta_row[1] = section_label("bench").to_string();
    meta_row[22] = meta.operbox.to_string();
    meta_row[23] = meta.owned.to_string();
    meta_row[24] = format!(
        "layout:{};trade:{};manufacture:{}",
        meta.layout.unwrap_or("search_baseline"),
        meta.trade_order_mode,
        meta.manufacture_scenario
    );
    wtr.write_record(meta_row)?;
    write_bench_pool_row(&mut wtr, "trade", trade_pool)?;
    for (i, hit) in trade_report.top.iter().enumerate() {
        write_bench_trade_row(&mut wtr, section_label("trade"), i + 1, hit, trade_report)?;
    }
    if let Some(hit) = &trade_report.gold_order_line {
        write_bench_trade_row(
            &mut wtr,
            section_label("trade_gold_line"),
            1,
            hit,
            trade_report,
        )?;
    }
    if let Some(hit) = &trade_report.originium_order_line {
        write_bench_trade_row(
            &mut wtr,
            section_label("trade_originium_line"),
            1,
            hit,
            trade_report,
        )?;
    }
    write_bench_pool_row(&mut wtr, "manufacture", manu_pool)?;
    for (i, hit) in manu_report.top.iter().enumerate() {
        write_bench_manu_row(
            &mut wtr,
            section_label("manufacture"),
            i + 1,
            hit,
            manu_report,
        )?;
    }
    if let Some(hit) = &manu_report.gold_line {
        write_bench_manu_row(
            &mut wtr,
            section_label("manufacture_gold_line"),
            1,
            hit,
            manu_report,
        )?;
    }
    if let Some(hit) = &manu_report.battle_record_line {
        write_bench_manu_row(
            &mut wtr,
            section_label("manufacture_battle_record_line"),
            1,
            hit,
            manu_report,
        )?;
    }
    flush_csv(wtr)
}

fn write_bench_pool_row(
    wtr: &mut Writer<Box<dyn Write>>,
    domain: &str,
    pool: &PoolSummary<'_>,
) -> Result<(), Error> {
    let mut row = vec![String::new(); 28];
    row[0] = section_label("pool").to_string();
    row[1] = domain_label(domain).to_string();
    row[25] = pool.ready.to_string();
    row[26] = pool.skipped.to_string();
    row[27] = pool.combinations_3.to_string();
    wtr.write_record(row)?;
    Ok(())
}

fn write_bench_trade_row(
    wtr: &mut Writer<Box<dyn Write>>,
    section: &str,
    rank: usize,
    hit: &TradeSearchHit,
    report: &TradeSearchReport,
) -> Result<(), Error> {
    let mut row = vec![String::new(); 28];
    row[0] = section.to_string();
    row[1] = domain_label("trade").to_string();
    row[2] = rank.to_string();
    row[3] = hit.final_efficiency.to_string();
    if let Some(breakdown) = hit.breakdown.as_ref() {
        row[4] = breakdown.base_efficiency.to_string();
        row[5] = breakdown.occupancy_efficiency.to_string();
        row[6] = breakdown.skill_efficiency.to_string();
        row[7] = breakdown.control_efficiency.to_string();
        row[9] = breakdown.unit_output_multiplier.to_string();
    }
    row[8] = hit.mechanic_equivalent_efficiency.to_string();
    row[14] = join_ops(&hit.names);
    row[15] = join_ops(&hit.gold_names);
    row[16] = join_ops(&hit.originium_names);
    row[18] = hit.rule_id.clone().unwrap_or_default();
    row[19] = report.combinations.to_string();
    row[20] = report.evaluated.to_string();
    row[21] = elapsed_secs(report.elapsed);
    wtr.write_record(row)?;
    Ok(())
}

fn write_bench_manu_row(
    wtr: &mut Writer<Box<dyn Write>>,
    section: &str,
    rank: usize,
    hit: &ManuSearchHit,
    report: &ManuSearchReport,
) -> Result<(), Error> {
    let mut row = vec![String::new(); 28];
    row[0] = section.to_string();
    row[1] = domain_label("manufacture").to_string();
    row[2] = rank.to_string();
    row[3] = hit.final_efficiency.to_string();
    row[4] = hit.breakdown.base_efficiency.to_string();
    row[5] = hit.breakdown.occupancy_efficiency.to_string();
    row[6] = hit.breakdown.skill_efficiency.to_string();
    row[7] = hit.breakdown.global_efficiency.to_string();
    row[10] = hit.per_station.gold.to_string();
    row[11] = hit.storage.gold.to_string();
    row[12] = hit.per_station.battle_record.to_string();
    row[13] = hit.storage.battle_record.to_string();
    row[14] = join_ops(&hit.names);
    row[15] = join_ops(&hit.gold_names);
    row[17] = join_ops(&hit.battle_record_names);
    row[19] = report.combinations.to_string();
    row[20] = report.evaluated.to_string();
    row[21] = elapsed_secs(report.elapsed);
    wtr.write_record(row)?;
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
                "  manufacture #{:<2} total_efficiency={} gold_efficiency={} storage={} battle_record_efficiency={} storage={}",
                i + 1,
                hit.final_efficiency,
                hit.per_station.gold,
                hit.storage.gold,
                hit.per_station.battle_record,
                hit.storage.battle_record,
            );
            eprintln!("         gold_ops={:?}", hit.gold_names);
            eprintln!("         exp_ops={:?}", hit.battle_record_names);
        } else {
            eprintln!(
                "  manufacture #{:<2} total_efficiency={} gold_efficiency={} storage={} battle_record_efficiency={} storage={} ops={:?}",
                i + 1,
                hit.final_efficiency,
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
            "  (split) gold_line={} {:?}  battle_record_line={} {:?}",
            gold.final_efficiency, gold.names, br.final_efficiency, br.names
        );
    }
    Ok(())
}

// ── trade yield ─────────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
pub struct TradeYieldRow<'a> {
    pub fixture: &'a str,
    pub level: u8,
    pub shift_hours: f64,
    pub paper_efficiency: Efficiency,
    pub total_efficiency: Efficiency,
    pub mechanic_equivalent_efficiency: Efficiency,
    pub trade_equivalent_efficiency: Efficiency,
    pub gold_equivalent_efficiency: Efficiency,
    pub rule_id: Option<&'a str>,
    pub unit_trade: f64,
    pub unit_gsl_gold: f64,
    pub unit_output_multiplier: Efficiency,
    pub trade_output_per_day: f64,
    #[serde(skip_serializing)]
    pub gold_consumption_per_day: f64,
    pub daily_trade: f64,
    pub daily_gold: f64,
    pub drone_trade: f64,
}

pub fn emit_trade_yield(opts: &OutputOptions, row: &TradeYieldRow<'_>) -> Result<(), Error> {
    match opts.format {
        OutputFormat::Csv => write_trade_yield_csv(opts.path.as_deref(), row),
        OutputFormat::Text => write_trade_yield_text(row),
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(row)?);
            Ok(())
        }
    }
}

fn write_trade_yield_csv(path: Option<&Path>, row: &TradeYieldRow<'_>) -> Result<(), Error> {
    let mut wtr = csv_writer(path)?;
    wtr.write_record([
        "夹具",
        "贸易站等级",
        "上班时长",
        "纸面效率",
        "总效率",
        "机制等效效率",
        "贸易等效效率",
        "赤金等效效率",
        "规则ID",
        "日贸易量",
        "日GSL赤金",
        "单位产出倍率",
        "日产贸易",
        "日产赤金",
        "贸易单位时间产出",
        "赤金单位时间产出",
        "无人机贸易",
    ])?;
    wtr.write_record([
        row.fixture,
        &row.level.to_string(),
        &format!("{:.1}", row.shift_hours),
        &row.paper_efficiency.to_string(),
        &row.total_efficiency.to_string(),
        &row.mechanic_equivalent_efficiency.to_string(),
        &row.trade_equivalent_efficiency.to_string(),
        &row.gold_equivalent_efficiency.to_string(),
        row.rule_id.unwrap_or(""),
        &format!("{:.1}", row.unit_trade),
        &format!("{:.1}", row.unit_gsl_gold),
        &row.unit_output_multiplier.to_string(),
        &format!("{:.0}", row.daily_trade),
        &format!("{:.1}", row.daily_gold),
        &format!("{:.1}", row.trade_output_per_day),
        &format!("{:.1}", row.gold_consumption_per_day),
        &format!("{:.0}", row.drone_trade),
    ])?;
    flush_csv(wtr)
}

fn write_trade_yield_text(row: &TradeYieldRow<'_>) -> Result<(), Error> {
    eprintln!(
        "fixture={} level={} shift={}h paper_efficiency={} total_efficiency={} mechanic_efficiency={} trade_equivalent={} gold_equivalent={} rule_id={:?}",
        row.fixture,
        row.level,
        row.shift_hours,
        row.paper_efficiency,
        row.total_efficiency,
        row.mechanic_equivalent_efficiency,
        row.trade_equivalent_efficiency,
        row.gold_equivalent_efficiency,
        row.rule_id
    );
    eprintln!(
        "  unit_trade={:.1} unit_gsl_gold={:.1} trade_output_per_day={:.1} gold_consumption_per_day={:.1} unit_output_multiplier={}",
        row.unit_trade,
        row.unit_gsl_gold,
        row.trade_output_per_day,
        row.gold_consumption_per_day,
        row.unit_output_multiplier
    );
    eprintln!(
        "  daily_trade={:.0} daily_gold={:.1} drone_trade={:.0}",
        row.daily_trade, row.daily_gold, row.drone_trade
    );
    Ok(())
}

fn team_label(
    profile: infra_core::schedule::TimedRotationProfile,
    label: TeamLabel,
) -> &'static str {
    if profile.is_two_team() {
        return match label {
            TeamLabel::Alpha => "主力",
            TeamLabel::Beta => "替补",
            TeamLabel::Gamma => "γ",
        };
    }
    match label {
        TeamLabel::Alpha => "α",
        TeamLabel::Beta => "β",
        TeamLabel::Gamma => "γ",
    }
}

fn team_by_label(
    teams: &[infra_core::schedule::TeamAssignment],
    label: TeamLabel,
) -> &infra_core::schedule::TeamAssignment {
    teams
        .iter()
        .find(|t| t.label == label)
        .expect("team label in report")
}

fn assignment_room_ops<'a>(assignment: &'a BaseAssignment, room_id: &str) -> Option<Vec<&'a str>> {
    assignment
        .iter_rooms()
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
    profile: infra_core::schedule::TimedRotationProfile,
    shift: &infra_core::schedule::TeamShiftResult,
    fiammetta_targets: &std::collections::HashSet<String>,
) -> Option<TeamLabel> {
    if profile == infra_core::schedule::TimedRotationProfile::MainBackup24_24
        && ops.iter().any(|name| fiammetta_targets.contains(*name))
    {
        return shift.active_teams.first().copied();
    }
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

fn shift_station_order(report: &TeamRotationReport) -> Vec<String> {
    let mut room_ids: std::collections::BTreeSet<String> = report
        .shifts
        .iter()
        .flat_map(|shift| shift.assignment.iter_rooms())
        .map(|room| room.room_id.0.clone())
        .collect();
    room_ids.extend(
        report
            .shifts
            .iter()
            .flat_map(|shift| shift.efficiencies.room_lines.iter())
            .map(|line| line.room_id.clone()),
    );
    let mut room_ids: Vec<_> = room_ids.into_iter().collect();
    room_ids.sort_by_key(|room_id| {
        let (group, index) = if let Some(index) = room_id.strip_prefix("trade_") {
            (0, index.parse::<u32>().unwrap_or(u32::MAX))
        } else if let Some(index) = room_id.strip_prefix("manu_") {
            (1, index.parse::<u32>().unwrap_or(u32::MAX))
        } else if let Some(index) = room_id.strip_prefix("power_") {
            (2, index.parse::<u32>().unwrap_or(u32::MAX))
        } else if room_id == "control" {
            (3, 0)
        } else if room_id == "office" {
            (4, 0)
        } else if let Some(index) = room_id.strip_prefix("office_") {
            (4, index.parse::<u32>().unwrap_or(u32::MAX))
        } else if room_id == "meeting" {
            (5, 0)
        } else if let Some(index) = room_id.strip_prefix("dorm_") {
            (6, index.parse::<u32>().unwrap_or(u32::MAX))
        } else {
            (7, u32::MAX)
        };
        (group, index, room_id.clone())
    });
    room_ids
}

fn format_shift_station_line(
    shift: &infra_core::schedule::TeamShiftResult,
    op_team: &std::collections::HashMap<String, TeamLabel>,
    profile: infra_core::schedule::TimedRotationProfile,
    room_id: &str,
    fiammetta_targets: &std::collections::HashSet<String>,
) -> String {
    let label = room_display_name(room_id);
    // 查找该房间的评分
    let score_hint = shift
        .efficiencies
        .room_lines
        .iter()
        .find(|r| r.room_id == room_id)
        .map(|r| {
            if !r.trade_efficiency.is_zero() {
                format!(
                    " [总效率{} / 基础效率{} / 干员技能效率{} / 全局加成{} / 等效效率{}]",
                    r.total_efficiency,
                    r.base_efficiency,
                    r.operator_efficiency,
                    r.global_efficiency,
                    r.equivalent_efficiency
                )
            } else if !r.manufacture_efficiency.is_zero() {
                format!(
                    " [总效率{} / 基础效率{} / 干员技能效率{} / 全局加成{}]",
                    r.total_efficiency,
                    r.base_efficiency,
                    r.operator_efficiency,
                    r.global_efficiency
                )
            } else if !r.power_efficiency.is_zero() {
                format!(
                    " [发电效率{} / 干员技能效率{}]",
                    r.final_efficiency, r.operator_efficiency
                )
            } else {
                String::new()
            }
        })
        .unwrap_or_default();

    if let Some(ops) = shift_room_ops(shift, room_id) {
        let names = ops.join(", ");
        if let Some(team) = room_active_team(op_team, &ops, profile, shift, fiammetta_targets) {
            format!(
                "  {label}: {names}（{t}队）{score_hint}",
                t = team_label(profile, team)
            )
        } else if matches!(
            room_id,
            "control" | "meeting" | "office" | "office_1" | "office_2"
        ) || room_id.starts_with("office_")
            || room_id.starts_with("dorm_")
        {
            format!("  {label}: {names}（共享）{score_hint}")
        } else {
            format!("  {label}: {names}{score_hint}")
        }
    } else {
        format!("  {label}: —{score_hint}")
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
        OutputFormat::Csv => {
            write_team_rotation_csv(opts.path.as_deref(), layout, operbox, owned, report)
        }
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
        "rotation_profile",
        "班次",
        "时长h",
        "上岗队",
        "休息队",
        "贸易效率",
        "制造效率",
        "发电效率",
        "贸易加权",
        "制造加权",
        "发电加权",
        "peak最长工作h",
        "peak瓶颈",
        "耗时秒",
    ])?;
    for shift in &report.shifts {
        let active: Vec<&str> = shift
            .active_teams
            .iter()
            .map(|team| team_label(report.profile, *team))
            .collect();
        let peak_eta = report
            .peak_mood_eta
            .as_ref()
            .and_then(|eta| eta.eta_hours)
            .map(|hours| format!("{hours:.2}"))
            .unwrap_or_default();
        let peak_bottleneck = report
            .peak_mood_eta
            .as_ref()
            .and_then(|eta| eta.bottleneck.as_deref())
            .unwrap_or("");
        wtr.write_record([
            layout,
            operbox,
            &owned.to_string(),
            report.profile.cli_name(),
            &(shift.index + 1).to_string(),
            &format!("{:.0}", shift.duration_hours),
            &active.join("+"),
            team_label(report.profile, shift.resting_team),
            &shift.efficiencies.trade_efficiency.to_string(),
            &shift.efficiencies.manufacture_efficiency.to_string(),
            &shift.efficiencies.power_efficiency.to_string(),
            &shift.weighted_trade.to_string(),
            &shift.weighted_manufacture.to_string(),
            &shift.weighted_power.to_string(),
            &peak_eta,
            peak_bottleneck,
            &elapsed_secs(report.elapsed),
        ])?;
    }
    flush_csv(wtr)
}

// ── box profile（用户报告 → stdout）────────────────────────────────────────

pub fn print_box_profile_report(profile: &BoxProfile) {
    print!("{}", render_box_profile_narrative(profile));
}

fn report_line(line: &str) {
    println!("{line}");
}

fn write_team_rotation_text(
    layout: &str,
    operbox: &str,
    owned: usize,
    report: &TeamRotationReport,
) -> Result<(), Error> {
    let op_team = infra_core::schedule::operator_team_map(report);
    let fiammetta_targets: std::collections::HashSet<String> =
        if report.profile == infra_core::schedule::TimedRotationProfile::MainBackup24_24 {
            report
                .shifts
                .iter()
                .filter_map(|shift| shift.fiammetta.as_ref())
                .map(|action| action.target.clone())
                .collect()
        } else {
            std::collections::HashSet::new()
        };
    let fiammetta_action_teams: std::collections::HashMap<String, TeamLabel> =
        if report.profile == infra_core::schedule::TimedRotationProfile::MainBackup24_24 {
            report
                .shifts
                .iter()
                .filter_map(|shift| {
                    let action = shift.fiammetta.as_ref()?;
                    let team = *shift.active_teams.first()?;
                    Some((action.target.clone(), team))
                })
                .collect()
        } else {
            std::collections::HashMap::new()
        };
    let fiammetta_blueprint = if fiammetta_targets.is_empty() {
        None
    } else {
        Some(BaseBlueprint::load(Path::new(layout))?)
    };

    report_line(&format!(
        "{}: layout={layout} operbox={operbox} owned={owned} elapsed={:.2?}",
        report.profile.display_name(),
        report.elapsed
    ));
    if let Some(eta) = &report.peak_mood_eta {
        match (eta.eta_hours, eta.bottleneck.as_deref()) {
            (Some(hours), Some(bottleneck)) => report_line(&format!(
                "  peak 主力最长工作时间: {hours:.2}h（首个瓶颈: {bottleneck}）"
            )),
            _ => report_line("  peak 主力最长工作时间: 无有限瓶颈"),
        }
    }
    if !report.peak_plan.activated.is_empty() {
        let systems: Vec<&str> = report
            .peak_plan
            .activated
            .iter()
            .map(|system| system.system_id.as_str())
            .collect();
        report_line(&format!("  peak 编排体系: {}", systems.join(", ")));
    }
    if !report.peak_plan.registry_claims.is_empty() {
        let trade_rooms: Vec<String> = report
            .peak_plan
            .registry_trade_room_ids()
            .into_iter()
            .map(|r| r.0)
            .collect();
        if !trade_rooms.is_empty() {
            report_line(&format!(
                "  peak 贸易 meta 房间: {}",
                trade_rooms.join(", ")
            ));
        }
    }
    report_line(&format!(
        "  每日加权效率（三类分开）: 贸易={}  制造={}  发电={}",
        report.daily.trade, report.daily.manufacture, report.daily.power
    ));
    if !fiammetta_targets.is_empty() {
        let action_targets: Vec<_> = report
            .shifts
            .iter()
            .filter_map(|shift| {
                let action = shift.fiammetta.as_ref()?;
                Some(format!(
                    "S{} {}（{}）",
                    shift.index + 1,
                    action.target,
                    room_display_name(&action.room_id.0)
                ))
            })
            .collect();
        report_line(&format!(
            "  菲亚目标（各自动作班最终在岗）: {}",
            action_targets.join("；")
        ));
    }

    let team_roster_note = if fiammetta_targets.is_empty() {
        ""
    } else {
        "；回池菲亚目标只列在其动作班所属队伍，逐班工位表显示实际在岗"
    };
    report_line(&format!(
        "\n--- 轮换队伍花名册（办公室/会客室尽量满编，缺员见 warnings{team_roster_note}）---"
    ));
    for team in &report.teams {
        let operators: Vec<_> = team
            .operators
            .iter()
            .filter(|name| {
                fiammetta_action_teams
                    .get(name.as_str())
                    .map_or(true, |owner| owner == &team.label)
            })
            .collect();
        report_line(&format!(
            "  {} 队 ({} 人): {}",
            team_label(report.profile, team.label),
            operators.len(),
            operators
                .iter()
                .map(|name| name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !report.warnings.is_empty() {
        report_line("\n--- 缺员提示 ---");
        for warning in &report.warnings {
            report_line(&format!("  {warning}"));
        }
    }

    report_line("\n--- 轮换一览 ---");
    report_line("  班次   时长   上班队        休息队");
    for shift in &report.shifts {
        let active: Vec<&str> = shift
            .active_teams
            .iter()
            .map(|team| team_label(report.profile, *team))
            .collect();
        report_line(&format!(
            "  {:>5}  {:>3.0}h   {:<12}  {}",
            format!("shift{}", shift.index + 1),
            shift.duration_hours,
            active.join("+"),
            team_label(report.profile, shift.resting_team),
        ));
    }

    for shift in &report.shifts {
        let active_names: Vec<&str> = shift
            .active_teams
            .iter()
            .map(|team| team_label(report.profile, *team))
            .collect();
        let resting = team_by_label(&report.teams, shift.resting_team);
        let working_fiammetta_targets: std::collections::HashSet<_> = fiammetta_blueprint
            .as_ref()
            .map_or_else(std::collections::HashSet::new, |blueprint| {
                let working_names = shift.assignment.work_position_operator_names(blueprint);
                fiammetta_targets
                    .intersection(&working_names)
                    .cloned()
                    .collect()
            });
        let resting_operators: Vec<_> = resting
            .operators
            .iter()
            .filter(|name| !working_fiammetta_targets.contains(*name))
            .collect();
        report_line(&format!(
            "\n======== Shift {} · {:.0}h · 上班 {} · 休息 {} ========",
            shift.index + 1,
            shift.duration_hours,
            active_names.join("+"),
            team_label(report.profile, shift.resting_team),
        ));
        if report.profile == infra_core::schedule::TimedRotationProfile::MainBackup24_24 {
            if let Some(action) = &shift.fiammetta {
                report_line(&format!(
                    "  本班菲亚目标: {}（{}）",
                    action.target,
                    room_display_name(&action.room_id.0)
                ));
            }
        }
        report_line(&format!(
            "  休息干员（{}队）: {}",
            team_label(report.profile, resting.label),
            resting_operators
                .iter()
                .map(|name| name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        if let Some(rest) = &shift.dorm_rest {
            report_line(&format!(
                "  指定宿舍恢复（{}）: 单回 {} + 群回 {} + 目标 {}",
                room_display_name(&rest.room_id.0),
                rest.single_manager,
                rest.group_manager,
                rest.target,
            ));
        }
        report_line(&format!(
            "  效率(原值): trade={}  manufacture={}  power={}",
            shift.efficiencies.trade_efficiency,
            shift.efficiencies.manufacture_efficiency,
            shift.efficiencies.power_efficiency,
        ));
        report_line(&format!(
            "  效率(按{:.0}h加权): trade={}  manufacture={}  power={}",
            shift.duration_hours,
            shift.weighted_trade,
            shift.weighted_manufacture,
            shift.weighted_power,
        ));

        report_line("\n  【各设施上岗情况】");
        for room_id in shift_station_order(report) {
            report_line(&format_shift_station_line(
                shift,
                &op_team,
                report.profile,
                &room_id,
                &fiammetta_targets,
            ));
        }
    }

    let formula = report
        .shifts
        .iter()
        .map(|shift| {
            let active = shift
                .active_teams
                .iter()
                .map(|team| team_label(report.profile, *team))
                .collect::<Vec<_>>()
                .join("+");
            format!("{:.0}h×{active}", shift.duration_hours)
        })
        .collect::<Vec<_>>()
        .join(" + ");
    report_line(&format!(
        "\n每日加权效率（三类分开，{formula}）: 贸易={}  制造={}  发电={}",
        report.daily.trade, report.daily.manufacture, report.daily.power
    ));
    Ok(())
}
