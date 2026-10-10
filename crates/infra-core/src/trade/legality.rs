//! 贸易组合合法性中按名字、站级口径判定的部分。
//!
//! 同房机制互斥（但书/可露希尔/巫恋侧/佩佩）按 buff 判定，仍在
//! `shortcut::trade_station_exclusive_violation`；这里只放按名字和站级口径的规则，
//! 搜索、Bake 查表、布局回填、轮换资格都从这里取，不再各写一份。

use crate::layout::{RoomBlueprint, RoomProduct};
use crate::trade::input::TradeOrderKind;

pub const WITCH_NAME: &str = "巫恋";
pub const TEQUILA_NAME: &str = "龙舌兰";
pub const BLACKKEY_NAME: &str = "黑键";

/// 全布局的贸易同房禁配（与 Plan 约束无关）。
pub const GLOBAL_FORBIDDEN_TRADE_PAIRS: [(&str, &str); 1] = [(BLACKKEY_NAME, WITCH_NAME)];

/// 兜底散件搜索不得单独凑出的组合：巫恋+龙舌兰只能以完整龙巫组出现，
/// 完整组由 witch 角色搜索产出，且仅限 [`witch_core_station_ok`] 的站。
pub const PLAIN_FORBIDDEN_TRADE_PAIRS: [(&str, &str); 1] = [(WITCH_NAME, TEQUILA_NAME)];

pub fn is_global_trade_forbidden_pair(a: &str, b: &str) -> bool {
    GLOBAL_FORBIDDEN_TRADE_PAIRS
        .iter()
        .any(|&(x, y)| (a == x && b == y) || (a == y && b == x))
}

/// 同房名单是否包含全布局禁配。
pub fn names_have_global_forbidden_pair<'a>(
    names: impl IntoIterator<Item = &'a str> + Clone,
) -> bool {
    GLOBAL_FORBIDDEN_TRADE_PAIRS.iter().any(|&(x, y)| {
        names.clone().into_iter().any(|name| name == x)
            && names.clone().into_iter().any(|name| name == y)
    })
}

/// 巫恋+龙舌兰完整组的站级口径：三级金单贸易站。
pub fn witch_core_station_ok(room: &RoomBlueprint) -> bool {
    room.level == 3
        && matches!(
            room.product,
            Some(RoomProduct::Trade {
                order: TradeOrderKind::Gold
            })
        )
}
