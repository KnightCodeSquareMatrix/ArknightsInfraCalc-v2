//! 全局资源 producer / consumer / 转化边注册表（权威清单见 `docs/EFFECT_ATOM_DESIGN.md` §8.13）。

use super::key::GlobalResourceKey;

/// 资源池内的有向转化：`from` 每 `from_per` 点 → `to` 增加 `to_per` 点。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlobalResourceConversion {
    pub from: GlobalResourceKey,
    pub to: GlobalResourceKey,
    /// 分母：消耗 `from_per` 点 `from`。
    pub from_per: f64,
    /// 分子：产出 `to_per` 点 `to`。
    pub to_per: f64,
    pub skill_hint: &'static str,
}

/// 已知跨资源转化（不含同房即时 `StateConsumeToEff` 的 div 消费）。
pub const CONVERSIONS: &[GlobalResourceConversion] = &[
    GlobalResourceConversion {
        from: GlobalResourceKey::Dream,
        to: GlobalResourceKey::Perception,
        from_per: 1.0,
        to_per: 1.0,
        skill_hint: "爱丽丝·梦境呓语",
    },
    GlobalResourceConversion {
        from: GlobalResourceKey::MusicalSection,
        to: GlobalResourceKey::Perception,
        from_per: 1.0,
        to_per: 1.0,
        skill_hint: "车尔尼·琴键漫步",
    },
    GlobalResourceConversion {
        from: GlobalResourceKey::MemoryFragment,
        to: GlobalResourceKey::Perception,
        from_per: 1.0,
        to_per: 1.0,
        skill_hint: "絮雨·追忆",
    },
    // 注意：感知→无声共鸣（黑键·乐感）/ 感知→思维链环（迷迭香·超感）**不是**全局消耗边。
    // 感知是全基建共享的累加值，黑键与迷迭香各自读取全量、互不扣减（PRTS：90 感知 → 迷迭香 90% / 黑键 45%）。
    // 这两条转化在各自设施房内由 `state_convert` atom 就地完成（读 `layout.global` 快照），
    // 切勿放回全局 CONVERSIONS——否则固定点迭代会让先跑的一条吃光感知，另一条恒为 0（race）。
    GlobalResourceConversion {
        from: GlobalResourceKey::HumanFireworks,
        to: GlobalResourceKey::WitchcraftCrystal,
        from_per: 5.0,
        to_per: 1.0,
        skill_hint: "截云·古老巫术",
    },
];

/// 建模优先级（求解器落地顺序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalResourceTier {
    /// 贸易 + 制造自动化 / 公孙基准已用或即将用。
    P0,
    /// 办公室 + 中枢薄 producer。
    P1,
    /// 全图扫描类（工程机器人）。
    P2,
    /// 单设施局部状态，不进全局池。
    LocalOnly,
}

#[derive(Debug, Clone, Copy)]
pub struct GlobalResourceEntry {
    pub key: GlobalResourceKey,
    pub tier: GlobalResourceTier,
    pub primary_producers: &'static [&'static str],
    pub primary_consumers: &'static [&'static str],
    pub notes: &'static str,
}

pub const REGISTRY: &[GlobalResourceEntry] = &[
    GlobalResourceEntry {
        key: GlobalResourceKey::Matatabi,
        tier: GlobalResourceTier::P0,
        primary_producers: &["中枢·火龙S黑角", "中枢·麒麟R夜刀"],
        primary_consumers: &["贸易/制造·泰拉大陆调查团"],
        notes: "",
    },
    GlobalResourceEntry {
        key: GlobalResourceKey::Perception,
        tier: GlobalResourceTier::P0,
        primary_producers: &["中枢·令/夕", "贸易·黑键/迷迭香", "宿舍·梦境/小节转化"],
        primary_consumers: &["黑键→无声共鸣", "迷迭香→思维链环"],
        notes: "多条入边；贸易/制造房内可再产",
    },
    GlobalResourceEntry {
        key: GlobalResourceKey::VirtualPower,
        tier: GlobalResourceTier::P0,
        primary_producers: &["中枢·森蚺", "发电站·承曦格雷伊晨曦"],
        primary_consumers: &["PowerStationCount 派生"],
        notes: "物理发电站在 Blueprint",
    },
    GlobalResourceEntry {
        key: GlobalResourceKey::VirtualGoldLines,
        tier: GlobalResourceTier::P0,
        primary_producers: &["贸易·鸿雪际崖", "贸易·绮良/图耶链"],
        primary_consumers: &["贸易订单%", "gold_flow"],
        notes: "同房链可再累加",
    },
    GlobalResourceEntry {
        key: GlobalResourceKey::HumanFireworks,
        tier: GlobalResourceTier::P0,
        primary_producers: &["中枢·令/夕/重岳", "办公室·桑葚", "贸易·乌有"],
        primary_consumers: &["铎铃", "截云→巫术结晶", "黍", "训练室·余"],
        notes: "",
    },
    GlobalResourceEntry {
        key: GlobalResourceKey::SilentEcho,
        tier: GlobalResourceTier::P0,
        primary_producers: &["宿舍·塑心", "办公室·深律", "感知转化·黑键"],
        primary_consumers: &["贸易·黑键徘徊旋律/怅惘和声"],
        notes: "",
    },
    GlobalResourceEntry {
        key: GlobalResourceKey::MonsterCuisine,
        tier: GlobalResourceTier::P0,
        primary_producers: &["宿舍·森西大食堂"],
        primary_consumers: &["贸易·齐尔查克", "制造·玛露西尔", "会客室·莱欧斯"],
        notes: "层数",
    },
    GlobalResourceEntry {
        key: GlobalResourceKey::Dream,
        tier: GlobalResourceTier::P0,
        primary_producers: &["宿舍·爱丽丝睡前故事"],
        primary_consumers: &["爱丽丝·梦境呓语→感知"],
        notes: "宿舍每级 +1 层",
    },
    GlobalResourceEntry {
        key: GlobalResourceKey::MusicalSection,
        tier: GlobalResourceTier::P0,
        primary_producers: &["宿舍·车尔尼慢板行歌"],
        primary_consumers: &["车尔尼·琴键漫步→感知"],
        notes: "宿舍每级 +1",
    },
    GlobalResourceEntry {
        key: GlobalResourceKey::MemoryFragment,
        tier: GlobalResourceTier::P0,
        primary_producers: &["办公室·絮雨巡游"],
        primary_consumers: &["絮雨·追忆→感知"],
        notes: "心情耗尽清空碎片与感知",
    },
    GlobalResourceEntry {
        key: GlobalResourceKey::WitchcraftCrystal,
        tier: GlobalResourceTier::P0,
        primary_producers: &["制造·截云古老巫术"],
        primary_consumers: &["截云·逐水草/问枯荣"],
        notes: "5 人间烟火 → 1",
    },
    GlobalResourceEntry {
        key: GlobalResourceKey::ThoughtChainRing,
        tier: GlobalResourceTier::P0,
        primary_producers: &["制造·迷迭香超感"],
        primary_consumers: &["迷迭香·念力/意识实体"],
        notes: "",
    },
    GlobalResourceEntry {
        key: GlobalResourceKey::IntelligenceReserve,
        tier: GlobalResourceTier::P1,
        primary_producers: &["中枢·灰烬情报储备"],
        primary_consumers: &["办公室·闪击", "加工站·霜华", "会客室·双月"],
        notes: "",
    },
    GlobalResourceEntry {
        key: GlobalResourceKey::UsautDrink,
        tier: GlobalResourceTier::P1,
        primary_producers: &["中枢·战车乌萨斯特饮"],
        primary_consumers: &["制造·导火索", "办公室·闪击", "加工站·霜华"],
        notes: "",
    },
    GlobalResourceEntry {
        key: GlobalResourceKey::Passion,
        tier: GlobalResourceTier::P1,
        primary_producers: &["中枢·初华/海铃/若麦/睦"],
        primary_consumers: &["中枢·祥子制造%", "中枢·睦贸易%"],
        notes: "",
    },
    GlobalResourceEntry {
        key: GlobalResourceKey::EngineeringRobot,
        tier: GlobalResourceTier::P2,
        primary_producers: &["制造·至简绘图设计"],
        primary_consumers: &["至简·机械辅助"],
        notes: "全基建扫描，上限 64",
    },
];
