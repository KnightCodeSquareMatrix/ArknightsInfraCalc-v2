use std::collections::HashMap;
use std::sync::Arc;

use crate::global_resource::{GlobalInjectManifest, GlobalResourceKey, GlobalResourcePool};

/// 全基建布局快照；搜索热路径用 `Arc` 共享只读。
pub type SharedLayout = Arc<LayoutContext>;

/// 简化假设：全基建宿舍进驻人数恒为满员（243c 基准 20 人）。
pub const DEFAULT_DORM_OCCUPANT_COUNT: u8 = 20;

/// 全基建场景快照（贸易/制造/发电/中控共享的布局假设与全局资源池）。
#[derive(Debug, Clone)]
pub struct LayoutContext {
    pub meeting_max_level: u8,
    pub dorm_level_sum: u16,
    pub manu_recipe_kinds: u8,
    /// 进驻游戏术语「精英干员」的设施间数（迷迭香/煌/逻各斯/烛煌/电弧/真言；真言「精英小队」）。
    pub elite_facility_count: u8,
    pub sui_facility_count: u8,
    /// 全基建宿舍进驻人数合计（黑键/乌有状态链）.
    pub dorm_occupant_count: u8,
    /// 基建内贸易站数量（制造站清流/引星棘刺等跨设施加成）.
    pub trade_station_count: u8,
    /// 基建内发电站数量（物理设施；制造站森蚺/温蒂·自动化）.
    pub power_station_count: u8,
    /// 基建内制造站数量（望·权变「实地」）.
    pub manufacture_station_count: u8,
    /// 全基建全局资源池（虚拟发电站、感知信息、木天蓼、魔物料理等）.
    pub global: GlobalResourcePool,
    /// 中枢 `GlobalInject` 汇总（贸易/制造全局 %）。
    pub global_inject: GlobalInjectManifest,
    /// 发电站进驻名单（森蚺·我寻思能行等）。
    pub power_workforce: Vec<String>,
    /// 控制中枢进驻名单（红松林等体系专属房内结算读取）。
    pub control_workforce: Vec<String>,
    /// 控制中枢运行时 buff，格式为 `(operator_name, buff_id)`。
    pub control_buffs: Vec<(String, String)>,
    /// 无人机上限（承曦·巡线框架；默认 135）。
    pub drone_cap: u32,
    /// 基建内发电站作业平台数量（阿兰娜/布丁中枢·超频）。
    pub platform_count_in_power: u8,
    /// 除某发电站干员外，基建内莱茵生命干员数（缪尔赛思）。
    pub rhine_life_in_base: u8,
    /// 基建内杜林族干员数（鸿雪·际崖居民；不含副手，至多 4）。
    pub durin_in_base: u8,
    /// 制造站赤金真实生产线数（由布局蓝图派生）。
    pub gold_manu_line_count: u32,
    /// 全基建设施等级合计（不含会客室；至简·绘图设计）。
    pub facility_level_sum_excl_meeting: u16,
    /// 训练室协助位干员（PhonoR-0·咒文共鸣）。
    pub training_assist: Vec<String>,
    /// 除本房外，其他发电站是否存在作业平台（承曦·晨曦）。
    pub other_power_has_platform: bool,
    /// 除自身外，其他作业平台在发电站（鸡励机制）。
    pub other_platform_in_power: bool,
    /// 除自身外，其他拉特兰干员在发电站（维护中）。
    pub other_laterano_in_power: bool,
    /// Operators working anywhere in base (赫德雷·白手起家).
    pub base_workforce: Vec<String>,
    /// 贸易站进驻干员（烈夏·患难拍档等跨设施条件）。
    pub trade_workforce: Vec<String>,
    /// 制造站进驻干员（涤火杰西卡/薇薇安娜等跨设施条件）。
    pub manu_workforce: Vec<String>,
    /// 各贸易站同房带 `tag` 干员数之和（中枢戴菲恩/八幡海铃等）。
    pub trade_tagged_count_sum: HashMap<String, u8>,
    /// 至少 `min` 名 `tag` 干员进驻的贸易站数量；键 `"tag@min"`（凛御银灰等）。
    pub trade_stations_tagged_gte: HashMap<String, u8>,
    /// 各制造站同房带 `tag` 干员数之和。
    pub manu_tagged_count_sum: HashMap<String, u8>,
    /// 训练室最高等级（维伊·手艺人）。
    pub training_room_level: u8,
    /// 人力办公室联络速度加成 %（闪击·语言学等；不影响贸易/制造算分）。
    pub office_hire_spd_pct: f64,
}

impl Default for LayoutContext {
    fn default() -> Self {
        Self {
            meeting_max_level: 0,
            dorm_level_sum: 0,
            manu_recipe_kinds: 0,
            elite_facility_count: 0,
            sui_facility_count: 0,
            dorm_occupant_count: DEFAULT_DORM_OCCUPANT_COUNT,
            trade_station_count: 0,
            power_station_count: 0,
            manufacture_station_count: 0,
            global: GlobalResourcePool::default(),
            global_inject: GlobalInjectManifest::default(),
            power_workforce: Vec::new(),
            control_workforce: Vec::new(),
            control_buffs: Vec::new(),
            drone_cap: 135,
            platform_count_in_power: 0,
            rhine_life_in_base: 0,
            durin_in_base: 0,
            gold_manu_line_count: 0,
            facility_level_sum_excl_meeting: 0,
            training_assist: Vec::new(),
            other_power_has_platform: false,
            other_platform_in_power: false,
            other_laterano_in_power: false,
            base_workforce: Vec::new(),
            trade_workforce: Vec::new(),
            manu_workforce: Vec::new(),
            trade_tagged_count_sum: HashMap::new(),
            trade_stations_tagged_gte: HashMap::new(),
            manu_tagged_count_sum: HashMap::new(),
            training_room_level: 0,
            office_hire_spd_pct: 0.0,
        }
    }
}

pub fn trade_station_tagged_gte_key(tag: &str, min: u8) -> String {
    format!("{tag}@{min}")
}

impl LayoutContext {
    /// 望·权变「外势」：贸易站 + 发电站间数。
    pub fn external_momentum(&self) -> u8 {
        self.trade_station_count
            .saturating_add(self.power_station_count)
    }

    /// 望·权变「实地」：制造站间数。
    pub fn field_momentum(&self) -> u8 {
        self.manufacture_station_count
    }

    /// 鸿雪精2「际崖居民」：杜林族虚拟赤金线（cap 4）。
    pub fn durin_virtual_lines(&self) -> u32 {
        (self.durin_in_base as u32).min(4)
    }

    /// operbox 规划：宿舍杜林上界与编制内计数取较大值（至多 4）。
    pub fn apply_durin_dorm_planning(&mut self, owned_durin_in_box: u8) {
        self.durin_in_base = self.durin_in_base.max(owned_durin_in_box.min(4));
    }

    /// 物理 + 全局虚拟发电站资源（自动化·α/β、仿生海龙等按此结算）。
    pub fn effective_power_station_count(&self) -> u8 {
        self.global
            .effective_power_station_count(self.power_station_count)
    }

    pub fn control_buff_active(&self, name: &str, buff_id: &str) -> bool {
        self.control_buffs
            .iter()
            .any(|(op, buff)| op == name && buff == buff_id)
    }

    /// 公孙 243 事实基准：由 `data/layout/243_use_this_.json` 派生（2 金贸）。
    pub fn search_baseline() -> Self {
        super::resolve_search_baseline_layout().unwrap_or_else(|_| Self::search_baseline_legacy())
    }

    /// 怪猎联动基准：243c 物理布局 + 中枢火龙S黑角 / 麒麟R夜刀 → 木天蓼 12。
    pub fn snhunt_baseline() -> Self {
        super::resolve_snhunt_baseline_layout().unwrap_or_else(|_| Self::snhunt_baseline_legacy())
    }

    /// 怪猎精2 双人中枢：木天蓼 12 + 全贸易 +7% + 全制造 +2%。
    pub fn snhunt_elite2_baseline() -> Self {
        super::resolve_snhunt_elite2_baseline_layout().unwrap_or_else(|_| {
            let mut layout = Self::snhunt_baseline_legacy();
            layout
                .global_inject
                .record_trade(crate::global_resource::INJECT_FAMILY_TRADE_GLOBAL_FLAT, 7.0);
            layout.global_inject.record_manu(
                crate::global_resource::INJECT_FAMILY_MANU_GLOBAL_ALL,
                None,
                2.0,
            );
            layout
        })
    }

    #[doc(hidden)]
    pub fn snhunt_baseline_legacy() -> Self {
        let mut layout = Self::search_baseline_legacy();
        layout.global.set(GlobalResourceKey::Matatabi, 12.0);
        layout
    }

    /// 252 自动组1：2 贸易 + 3 物理发电 + 中枢森蚺 + 发电站 Lancet-2。
    pub fn automation_group_1() -> Self {
        let instances = crate::instances::OperatorInstances::load(
            &crate::instances::default_instances_path().expect("instances path"),
        )
        .expect("operator_instances.json");
        let table = crate::skill_table::SkillTable::load(
            &crate::skill_table::default_skill_table_path().expect("skill_table path"),
        )
        .expect("skill_table.json");
        super::resolve_automation_group_1_layout(&instances, &table)
            .unwrap_or_else(|_| Self::automation_group_1_legacy())
    }

    #[doc(hidden)]
    pub fn search_baseline_legacy() -> Self {
        Self {
            meeting_max_level: 3,
            dorm_level_sum: 20,
            manu_recipe_kinds: 2,
            sui_facility_count: 2,
            dorm_occupant_count: DEFAULT_DORM_OCCUPANT_COUNT,
            trade_station_count: 2,
            power_station_count: 3,
            drone_cap: 135,
            // NB: MonsterCuisine no longer hardcoded here; it's handled by cross_facility orchestration layer
            gold_manu_line_count: 2,
            ..Default::default()
        }
    }

    #[doc(hidden)]
    pub fn automation_group_1_legacy() -> Self {
        Self {
            trade_station_count: 2,
            power_station_count: 3,
            power_workforce: vec!["Lancet-2".into()],
            ..Self::search_baseline_legacy()
        }
    }
}
