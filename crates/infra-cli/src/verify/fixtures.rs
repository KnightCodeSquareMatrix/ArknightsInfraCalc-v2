use infra_core::layout::LayoutContext;
use infra_core::trade::{TradeOperator, TradeRoomInput};
use std::sync::Arc;

pub fn unit_fixture(name: &str, level: u8) -> TradeRoomInput {
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
    let exusiai = op("能天使", 2, vec!["trade_ord_spd[020]"]);
    let texas_e2 = op("德克萨斯", 2, vec!["trade_ord_spd&cost_P[000]"]);
    let texas_e0 = op("德克萨斯", 0, vec!["trade_ord_spd&cost_P[000]"]);
    let lappland = op("拉普兰德", 2, vec!["trade_ord_limit&cost_P[001]"]);

    let operators = match case_id {
        "reg_gsl_closure_tier90" => vec![closure, exusiai, texas_e2, lappland],
        "reg_gsl_closure_tier80" => vec![closure, texas_e2, lappland],
        "reg_gsl_closure_tier60" => vec![closure, exusiai, texas_e0],
        _ => vec![closure, exusiai, texas_e2, lappland],
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
    let op = |name: &str, elite: u8, buff_ids: Vec<&str>, tags: Vec<&str>| {
        TradeOperator {
            name: name.into(),
            elite,
            buff_ids: buff_ids.into_iter().map(str::to_string).collect(),
            tags: tags.into_iter().map(str::to_string).collect(),
            ..Default::default()
        }
    };
    let mut layout = LayoutContext::default();
    layout
        .global_inject
        .record_karlan_precision(-15.0, 6);
    let mut input = TradeRoomInput::with_operators(
        level,
        vec![
            op("孑", 2, vec!["trade_ord_limit_count[000]"], vec![]),
            op(
                "银灰",
                2,
                vec!["trade_ord_spd&limit[022]"],
                vec!["cc.g.karlan"],
            ),
            op(
                "崖心",
                2,
                vec!["trade_ord_spd&limit[021]"],
                vec!["cc.g.karlan"],
            ),
        ],
    );
    input.layout = Arc::new(layout);
    input
}
