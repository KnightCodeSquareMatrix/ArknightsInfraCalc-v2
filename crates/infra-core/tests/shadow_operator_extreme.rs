use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use infra_core::bake::BakeMode;
use infra_core::layout::{BaseBlueprint, LayoutContext};
use infra_core::manufacture::ManuSearchRecipeMode;
use infra_core::operbox::OperBox;
use infra_core::pool::{build_manufacture_pool, build_trade_pool, ManuPool, TradePool};
use infra_core::search::{
    search_manufacture_triples, search_trade_triples, ManuSearchOptions, TradeSearchOptions,
};
use infra_core::skill_table::SkillTable;
use infra_core::tier::PromotionTier;
use infra_core::trade::input::{TradeOrderKind, TradeSearchOrderMode};
use infra_core::types::RecipeKind;
use infra_core::OperatorInstances;

fn fixture_path(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/fixtures/shadow_operator_extreme")
        .join(file)
}

fn select_trade(pool: &TradePool, names: &[&str]) -> TradePool {
    let wanted: HashSet<_> = names.iter().copied().collect();
    TradePool {
        entries: pool
            .entries
            .iter()
            .filter(|entry| wanted.contains(entry.name.as_str()))
            .cloned()
            .collect(),
        skipped: vec![],
    }
}

fn select_manu(pool: &ManuPool, names: &[&str]) -> ManuPool {
    let wanted: HashSet<_> = names.iter().copied().collect();
    ManuPool {
        entries: pool
            .entries
            .iter()
            .filter(|entry| wanted.contains(entry.name.as_str()))
            .cloned()
            .collect(),
        skipped: vec![],
    }
}

fn fixture_pools() -> (TradePool, ManuPool, SkillTable) {
    let blueprint = BaseBlueprint::load(&fixture_path("layout.json")).unwrap();
    let operbox = OperBox::load(&fixture_path("operbox.json")).unwrap();
    let instances = OperatorInstances::load(&fixture_path("operator_instances.json")).unwrap();
    let table = SkillTable::load(&fixture_path("skill_table.json")).unwrap();
    assert!(operbox.owns("shadow_trade"));
    assert!(instances
        .get("shadow_trade", PromotionTier::TierUp)
        .is_some());
    assert!(instances
        .resolve_trade_buff_ids("shadow_trade", PromotionTier::TierUp)
        .iter()
        .any(|id| id == "test_trade_shadow_trade"));
    let trade_pool =
        build_trade_pool(&operbox.trade_roster(&instances), &instances, &table).unwrap();
    let manu_pool =
        build_manufacture_pool(&operbox.manufacture_roster(&instances), &instances, &table)
            .unwrap();
    assert_eq!(blueprint.rooms.len(), 2);
    assert_eq!(
        trade_pool.entries.len(),
        13,
        "trade skipped: {:?}",
        trade_pool.skipped
    );
    assert_eq!(
        manu_pool.entries.len(),
        13,
        "manu skipped: {:?}",
        manu_pool.skipped
    );
    (trade_pool, manu_pool, table)
}

fn trade_options_for(level: u8, capacity: usize) -> TradeSearchOptions {
    TradeSearchOptions {
        trade_level: level,
        operator_capacity: capacity,
        mood: 24.0,
        top_k: 10,
        gold_production_lines: 1,
        layout: Arc::new(LayoutContext::search_baseline()),
        shift_hours: 24.0,
        order_mode: TradeSearchOrderMode::Single(TradeOrderKind::Gold),
        bake_mode: BakeMode::Disabled,
        full_pool: true,
    }
}

fn trade_options() -> TradeSearchOptions {
    trade_options_for(1, 1)
}

fn manu_options_for(level: u8, capacity: usize) -> ManuSearchOptions {
    ManuSearchOptions {
        level,
        operator_capacity: capacity,
        recipe_mode: ManuSearchRecipeMode::Single(RecipeKind::Gold),
        mood: 24.0,
        top_k: 10,
        layout: Arc::new(LayoutContext::search_baseline()),
        shift_hours: Some(24.0),
        must_include_name: None,
        bake_mode: BakeMode::Disabled,
        full_pool: true,
    }
}

fn manu_options() -> ManuSearchOptions {
    manu_options_for(1, 1)
}

fn assert_opportunity_cost_scenario(
    all_trade: &TradePool,
    all_manu: &ManuPool,
    table: &SkillTable,
    names: &[&str],
    shared: &str,
    expected_backup_trade: &str,
    expected_backup_manu: &str,
    expect_trade: bool,
) {
    let trade_pool = select_trade(all_trade, names);
    let manu_pool = select_manu(all_manu, names);
    assert_eq!(
        trade_pool.entries.len(),
        3,
        "selected trade names: {names:?}"
    );
    assert_eq!(manu_pool.entries.len(), 3, "selected manu names: {names:?}");
    let trade = search_trade_triples(&trade_pool, table, &trade_options()).unwrap();
    let manu = search_manufacture_triples(&manu_pool, table, &manu_options()).unwrap();
    assert_eq!(trade.combinations, 3);
    assert_eq!(manu.combinations, 3);
    assert_eq!(trade.best.names, [shared]);
    assert_eq!(manu.best.names, [shared]);

    let without_shared: HashSet<_> = [shared].into_iter().collect();
    let trade_without_pool = TradePool {
        entries: trade_pool
            .entries
            .iter()
            .filter(|entry| !without_shared.contains(entry.name.as_str()))
            .cloned()
            .collect(),
        skipped: vec![],
    };
    let manu_without_pool = ManuPool {
        entries: manu_pool
            .entries
            .iter()
            .filter(|entry| !without_shared.contains(entry.name.as_str()))
            .cloned()
            .collect(),
        skipped: vec![],
    };
    let trade_without = search_trade_triples(&trade_without_pool, table, &trade_options()).unwrap();
    let manu_without =
        search_manufacture_triples(&manu_without_pool, table, &manu_options()).unwrap();
    assert_eq!(trade_without.best.names, [expected_backup_trade]);
    assert_eq!(manu_without.best.names, [expected_backup_manu]);

    let trade_score = trade.best.final_efficiency + manu_without.best.final_efficiency;
    let manu_score = trade_without.best.final_efficiency + manu.best.final_efficiency;
    assert_eq!(
        expect_trade,
        trade_score > manu_score || trade_score == manu_score,
        "trade_score={} manufacture_score={}",
        trade_score.millis(),
        manu_score.millis()
    );
    if expect_trade {
        assert!(trade_score >= manu_score);
    } else {
        assert!(manu_score > trade_score);
    }
}

#[test]
fn extreme_trade_winner_prefers_shared_operator_for_trade() {
    let (trade_pool, manu_pool, table) = fixture_pools();
    assert_opportunity_cost_scenario(
        &trade_pool,
        &manu_pool,
        &table,
        &["shadow_trade", "trade_backup", "manu_backup_for_trade"],
        "shadow_trade",
        "trade_backup",
        "manu_backup_for_trade",
        true,
    );
}

#[test]
fn extreme_manufacture_winner_prefers_shared_operator_for_manufacture() {
    let (trade_pool, manu_pool, table) = fixture_pools();
    assert_opportunity_cost_scenario(
        &trade_pool,
        &manu_pool,
        &table,
        &["shadow_manu", "trade_backup_for_manu", "manu_backup"],
        "shadow_manu",
        "trade_backup_for_manu",
        "manu_backup",
        false,
    );
}

#[test]
fn extreme_tie_is_stable_for_trade() {
    let (trade_pool, manu_pool, table) = fixture_pools();
    assert_opportunity_cost_scenario(
        &trade_pool,
        &manu_pool,
        &table,
        &["shadow_tie", "tie_trade_backup", "tie_manu_backup"],
        "shadow_tie",
        "tie_trade_backup",
        "tie_manu_backup",
        true,
    );
}

#[test]
fn level_two_fixture_exposes_two_shared_picks_on_both_sides() {
    let (trade_pool, manu_pool, table) = fixture_pools();
    let blueprint = BaseBlueprint::load(&fixture_path("layout_level2.json")).unwrap();
    let trade_room = blueprint
        .room(&infra_core::layout::RoomId::from("trade_1"))
        .unwrap();
    let manu_room = blueprint
        .room(&infra_core::layout::RoomId::from("manu_1"))
        .unwrap();
    assert_eq!(trade_room.operator_capacity(), 2);
    assert_eq!(manu_room.operator_capacity(), 2);

    let names = [
        "shadow_multi_alpha",
        "shadow_multi_beta",
        "multi_trade_backup",
        "multi_manu_backup",
    ];
    let trade = search_trade_triples(
        &select_trade(&trade_pool, &names),
        &table,
        &trade_options_for(2, 2),
    )
    .unwrap();
    let manu = search_manufacture_triples(
        &select_manu(&manu_pool, &names),
        &table,
        &manu_options_for(2, 2),
    )
    .unwrap();
    assert_eq!(trade.combinations, 6);
    assert_eq!(manu.combinations, 6);

    let expected: HashSet<_> = ["shadow_multi_alpha", "shadow_multi_beta"]
        .into_iter()
        .collect();
    let trade_names: HashSet<_> = trade.best.names.iter().map(String::as_str).collect();
    let manu_names: HashSet<_> = manu.best.names.iter().map(String::as_str).collect();
    assert_eq!(trade_names, expected);
    assert_eq!(manu_names, expected);
}
