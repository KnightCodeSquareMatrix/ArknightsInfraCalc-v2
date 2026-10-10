pub mod efficiency;
pub mod gold_flow;
pub mod input;
pub mod interpreter;
pub mod legality;
pub mod order_mechanic;
pub mod segment;
pub mod shortcut;
pub mod solver;
pub mod unit_output;

pub use efficiency::{
    PaperTradeEfficiency, TradeEfficiency, TradeProductionBasis,
    GOLD_TRADE_REFERENCE_OUTPUT_PER_DAY,
};
pub use input::{
    LayoutContext, TradeLayoutContext, TradeOperator, TradeOrderKind, TradeRoomInput,
    TradeSearchOrderMode, TradeStationScenario,
};
pub use segment::{
    default_segments_path, load_trade_segments, match_registered_trade_segment,
    segment_producer_active, SegmentProducerDef, TradeRoleDef, TradeSegmentDef,
};
pub use solver::{solve_trade, solve_trade_with_shift, TradeProductionReport, TradeResult};
pub use unit_output::{daily_yield, TradeDailyYield, TradeUnitOutput, DRONE_TRADE_FACTOR};
