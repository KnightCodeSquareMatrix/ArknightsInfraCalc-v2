use serde::{Deserialize, Serialize};

/// 跨设施产出、全局池累积、由 consumer 读取的基建资源种类。
///
/// 与 **布局（Blueprint）** 中的物理设施数（真实发电站 3 座等）区分。
/// `skill_table` 中 `StateProduce` / `StateConvert` / `StateConsumeToEff` 的 `key`
/// 字段通过 [`Self::parse`] 映射到本枚举。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlobalResourceKey {
    /// 木天蓼 — 中枢火龙S黑角 / 麒麟R夜刀等 producer。
    Matatabi,
    /// 感知信息 — 令、夕、黑键、迷迭香、梦境/小节/记忆碎片转化等。
    Perception,
    /// 虚拟发电站 — 森蚺我寻思能行、承曦晨曦；与物理发电站叠加 → `PowerStationCount`。
    VirtualPower,
    /// 虚拟赤金产线 — 鸿雪际崖、绮良/图耶链；贸易 `%` 与 `gold_flow` 初值。
    VirtualGoldLines,
    /// 人间烟火 — 令、夕、重岳、桑葚、乌有等。
    HumanFireworks,
    /// 无声共鸣 — 塑心、深律；黑键乐感由感知 1:1 转化。
    SilentEcho,
    /// 魔物料理 — 森西宿舍层数；齐尔查克 / 玛露西尔 / 莱欧斯会客室等。
    MonsterCuisine,
    /// 梦境 — 爱丽丝睡前故事（宿舍每级 +1 层）→ 梦境呓语 → 感知信息。
    Dream,
    /// 小节 — 车尔尼慢板行歌（宿舍每级 +1）→ 琴键漫步 → 感知信息。
    MusicalSection,
    /// 记忆碎片 — 絮雨巡游（招募位 +10）→ 追忆 → 感知信息；心情耗尽清空。
    MemoryFragment,
    /// 巫术结晶 — 截云古老巫术（5 人间烟火 → 1）→ 逐水草 / 问枯荣。
    WitchcraftCrystal,
    /// 思维链环 — 迷迭香超感（感知 1:1）→ 念力 / 意识实体。
    ThoughtChainRing,
    /// 情报储备 — 灰烬情报储备（中枢彩虹小队）；闪击 / 霜华 / 双月倍率。
    IntelligenceReserve,
    /// 乌萨斯特饮 — 战车（中枢乌萨斯学生团）；导火索仓库、闪击 / 霜华办公室。
    UsautDrink,
    /// 热情值 — 初华偶像光环等；祥子制造 %、睦贸易 % 等。
    Passion,
    /// 工程机器人 — 至简绘图设计（全基建设施每级 +1，上限 64）→ 机械辅助。
    EngineeringRobot,
}

impl GlobalResourceKey {
    pub const ALL: &[Self] = &[
        Self::Matatabi,
        Self::Perception,
        Self::VirtualPower,
        Self::VirtualGoldLines,
        Self::HumanFireworks,
        Self::SilentEcho,
        Self::MonsterCuisine,
        Self::Dream,
        Self::MusicalSection,
        Self::MemoryFragment,
        Self::WitchcraftCrystal,
        Self::ThoughtChainRing,
        Self::IntelligenceReserve,
        Self::UsautDrink,
        Self::Passion,
        Self::EngineeringRobot,
    ];

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "Matatabi" | "木天蓼" => Some(Self::Matatabi),
            "Perception" | "感知信息" => Some(Self::Perception),
            "VirtualPower" | "虚拟发电站" => Some(Self::VirtualPower),
            "VirtualGoldLines" | "虚拟赤金产线" => Some(Self::VirtualGoldLines),
            "HumanFireworks" | "人间烟火" => Some(Self::HumanFireworks),
            "SilentEcho" | "无声共鸣" => Some(Self::SilentEcho),
            "MonsterCuisine" | "魔物料理" => Some(Self::MonsterCuisine),
            "Dream" | "梦境" => Some(Self::Dream),
            "MusicalSection" | "小节" => Some(Self::MusicalSection),
            "MemoryFragment" | "记忆碎片" => Some(Self::MemoryFragment),
            "WitchcraftCrystal" | "巫术结晶" => Some(Self::WitchcraftCrystal),
            "ThoughtChainRing" | "思维链环" => Some(Self::ThoughtChainRing),
            "IntelligenceReserve" | "情报储备" => Some(Self::IntelligenceReserve),
            "UsautDrink" | "乌萨斯特饮" => Some(Self::UsautDrink),
            "Passion" | "热情值" => Some(Self::Passion),
            "EngineeringRobot" | "工程机器人" => Some(Self::EngineeringRobot),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Matatabi => "matatabi",
            Self::Perception => "perception",
            Self::VirtualPower => "virtual_power",
            Self::VirtualGoldLines => "virtual_gold_lines",
            Self::HumanFireworks => "human_fireworks",
            Self::SilentEcho => "silent_echo",
            Self::MonsterCuisine => "monster_cuisine",
            Self::Dream => "dream",
            Self::MusicalSection => "musical_section",
            Self::MemoryFragment => "memory_fragment",
            Self::WitchcraftCrystal => "witchcraft_crystal",
            Self::ThoughtChainRing => "thought_chain_ring",
            Self::IntelligenceReserve => "intelligence_reserve",
            Self::UsautDrink => "usaut_drink",
            Self::Passion => "passion",
            Self::EngineeringRobot => "engineering_robot",
        }
    }

    /// 游戏内中文名（注册表 / CLI 展示）。
    pub fn name_zh(self) -> &'static str {
        match self {
            Self::Matatabi => "木天蓼",
            Self::Perception => "感知信息",
            Self::VirtualPower => "虚拟发电站",
            Self::VirtualGoldLines => "虚拟赤金产线",
            Self::HumanFireworks => "人间烟火",
            Self::SilentEcho => "无声共鸣",
            Self::MonsterCuisine => "魔物料理",
            Self::Dream => "梦境",
            Self::MusicalSection => "小节",
            Self::MemoryFragment => "记忆碎片",
            Self::WitchcraftCrystal => "巫术结晶",
            Self::ThoughtChainRing => "思维链环",
            Self::IntelligenceReserve => "情报储备",
            Self::UsautDrink => "乌萨斯特饮",
            Self::Passion => "热情值",
            Self::EngineeringRobot => "工程机器人",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_zh_and_en_aliases() {
        assert_eq!(GlobalResourceKey::parse("思维链环"), Some(GlobalResourceKey::ThoughtChainRing));
        assert_eq!(GlobalResourceKey::parse("ThoughtChainRing"), Some(GlobalResourceKey::ThoughtChainRing));
        assert_eq!(GlobalResourceKey::parse("乌萨斯特饮"), Some(GlobalResourceKey::UsautDrink));
        assert_eq!(GlobalResourceKey::parse("工程机器人"), Some(GlobalResourceKey::EngineeringRobot));
    }

    #[test]
    fn all_keys_have_unique_ids() {
        use std::collections::HashSet;
        let ids: HashSet<_> = GlobalResourceKey::ALL.iter().map(|k| k.id()).collect();
        assert_eq!(ids.len(), GlobalResourceKey::ALL.len());
    }
}
