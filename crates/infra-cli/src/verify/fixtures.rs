use infra_core::layout::LayoutContext;
use infra_core::trade::{TradeOperator, TradeRoomInput};
use std::sync::Arc;

pub fn unit_fixture(name: &str, level: u8) -> TradeRoomInput {
    if name.starts_with("gsl_witch_") {
        return witch_fixture(name, level);
    }
    if name == "gsl_blackkey_closure" {
        return blackkey_closure_fixture(level);
    }
    if name.starts_with("gsl_closure_") {
        let case_id = format!("reg_{name}");
        return closure_fixture(&case_id, level);
    }
    let op = |n: &str, elite: u8, buff_ids: Vec<&str>| {
        TradeOperator::new(n, elite, buff_ids.into_iter().map(str::to_string).collect())
    };
    let operators = match name {
        "closure_solo" => vec![op("可露希尔", 2, vec!["trade_ord_closure[000]"])],
        "docus_solo" => vec![op(
            "但书",
            2,
            vec!["trade_ord_law[000]", "trade_ord_against[010]"],
        )],
        "witch_long_beta" => vec![
            op(
                "巫恋",
                2,
                vec!["trade_ord_vodfox[000]", "trade_ord_wt&cost[000]"],
            ),
            op("龙舌兰", 2, vec!["trade_ord_long[010]"]),
            op("柏喙", 2, vec!["trade_ord_wt&cost[011]"]),
        ],
        _ => vec![op("古米", 0, vec!["trade_ord_spd&cost[000]"])],
    };
    TradeRoomInput::with_operators(level, operators)
}

pub fn witch_fixture(shortcut_id: &str, level: u8) -> TradeRoomInput {
    let op = |name: &str, elite: u8, buff_ids: Vec<&str>| {
        TradeOperator::new(
            name,
            elite,
            buff_ids.into_iter().map(str::to_string).collect(),
        )
    };
    let shamare = op(
        "巫恋",
        2,
        vec!["trade_ord_vodfox[000]", "trade_ord_wt&cost[000]"],
    );
    let long_e2 = op("龙舌兰", 2, vec!["trade_ord_long[010]"]);
    let long_e0 = op("龙舌兰", 0, vec!["trade_ord_long[000]"]);
    let kafka_beta = op("卡夫卡", 2, vec!["trade_ord_wt&cost[011]"]);
    let zheguang_alpha = op("折光", 0, vec!["trade_ord_wt&cost[002]"]);
    let blank = op("古米", 0, vec!["trade_ord_spd&cost[000]"]);

    let operators = match shortcut_id {
        "gsl_witch_long_beta" => vec![shamare, long_e2, kafka_beta],
        "gsl_witch_long_alpha" => vec![shamare, long_e2, zheguang_alpha],
        "gsl_witch_long_blank" => vec![shamare, long_e2, blank],
        "gsl_witch_long0_alpha" => vec![shamare, long_e0, zheguang_alpha],
        "gsl_witch_long0_beta" => vec![shamare, long_e0, kafka_beta],
        "gsl_witch_long0_blank" => vec![shamare, long_e0, blank],
        "gsl_witch_beta_blank" => vec![shamare, kafka_beta, blank],
        _ => vec![shamare, long_e2, blank],
    };

    TradeRoomInput::with_operators(level, operators)
}

pub fn docus_fixture(_case_id: &str, level: u8) -> TradeRoomInput {
    let op = |name: &str, elite: u8, buff_ids: Vec<&str>| {
        TradeOperator::new(
            name,
            elite,
            buff_ids.into_iter().map(str::to_string).collect(),
        )
    };
    let docus = op(
        "但书",
        2,
        vec!["trade_ord_law[000]", "trade_ord_against[010]"],
    );
    let texas_e2 = op("德克萨斯", 2, vec!["trade_ord_spd&cost_P[000]"]);
    let exusiai_e0 = op("能天使", 0, vec!["trade_ord_spd[010]"]);

    let operators = vec![docus, exusiai_e0, texas_e2];

    TradeRoomInput::with_operators(level, operators)
}

pub fn closure_fixture(case_id: &str, level: u8) -> TradeRoomInput {
    let op = |name: &str, elite: u8, buff_ids: Vec<&str>| {
        TradeOperator::new(
            name,
            elite,
            buff_ids.into_iter().map(str::to_string).collect(),
        )
    };
    let closure = op("可露希尔", 2, vec!["trade_ord_closure[000]"]);
    let high = op(
        "高效档测试干员",
        2,
        vec!["trade_ord_spd[020]", "trade_ord_spd[021]"],
    );
    let normal = op("普通高效测试干员", 2, vec!["trade_ord_spd[020]"]);
    let medium = op("中效档测试干员", 2, vec!["trade_ord_spd[001]"]);
    let low = op("低效档测试干员", 0, vec!["trade_ord_spd[000]"]);
    let operators = match case_id {
        // 三人容量内构造 113% / 78% / 53% 加成，分别最接近社区 114 / 80 / 60 档。
        "reg_gsl_closure_tier90" => vec![closure, high, medium],
        "reg_gsl_closure_tier80" => vec![closure, normal, medium],
        "reg_gsl_closure_tier60" => vec![closure, low.clone(), low],
        _ => vec![closure, high, medium],
    };
    TradeRoomInput::with_operators(level, operators)
}

pub fn blackkey_closure_fixture(level: u8) -> TradeRoomInput {
    let op = |name: &str, elite: u8, buff_ids: Vec<&str>| {
        TradeOperator::new(
            name,
            elite,
            buff_ids.into_iter().map(str::to_string).collect(),
        )
    };
    let blackkey = op(
        "黑键",
        2,
        vec!["trade_ord_spd_bd_n1[000]", "trade_ord_spd_bd[010]"],
    );
    let closure = op("可露希尔", 2, vec!["trade_ord_closure[000]"]);
    let jixing = op("吉星", 2, vec!["trade_ord_spd&share[002]"]);

    TradeRoomInput::with_operators(level, vec![blackkey, closure, jixing])
}

pub fn ling_jie_fixture(level: u8) -> TradeRoomInput {
    let op = |name: &str, elite: u8, buff_ids: Vec<&str>, tags: Vec<&str>| TradeOperator {
        name: name.into(),
        elite,
        buff_ids: buff_ids.into_iter().map(str::to_string).collect(),
        tags: tags.into_iter().map(str::to_string).collect(),
        ..Default::default()
    };
    let mut layout = LayoutContext::default();
    layout.global_inject.record_karlan_precision(-15.0, 6);
    let mut input = TradeRoomInput::with_operators(
        level,
        vec![
            op(
                "孑",
                2,
                vec!["trade_ord_limit_diff[000]", "trade_ord_limit_count[000]"],
                vec![],
            ),
            op(
                "银灰",
                2,
                vec!["trade_ord_spd&limit[022]"],
                vec!["cc.g.karlan"],
            ),
            op(
                "琳琅诗怀雅",
                2,
                vec!["trade_ord_spd[000]", "trade_ord_spd_variable[000]"],
                vec![],
            ),
        ],
    );
    input.layout = Arc::new(layout);
    input
}
