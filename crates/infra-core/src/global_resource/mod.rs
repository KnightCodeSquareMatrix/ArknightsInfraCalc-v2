//! 全基建全局资源层：木天蓼、感知信息、虚拟发电站、虚拟赤金产线等。
//!
//! **布局（Blueprint）** 只描述物理设施；本模块的 [`GlobalResourcePool`] 统一管理
//! 跨设施 producer → 池 → consumer 链路。贸易/制造同房求解从池中取快照，
//! `StateWrite` phase 在快照上叠加，再通过 Selector 派生（如有效发电站数）。

mod inject;
mod key;
mod pool;
mod registry;

pub use inject::{
    GlobalInjectManifest, KarlanPrecision, INJECT_FAMILY_MANU_GLOBAL_ALL,
    INJECT_FAMILY_TRADE_GLOBAL_FLAT,
};
pub use key::GlobalResourceKey;
pub use pool::GlobalResourcePool;
pub use registry::{
    GlobalResourceConversion, GlobalResourceEntry, GlobalResourceTier, CONVERSIONS, REGISTRY,
};
