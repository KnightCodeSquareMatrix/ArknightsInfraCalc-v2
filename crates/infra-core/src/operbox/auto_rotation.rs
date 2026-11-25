//! 自动轮换的体系岗位 policy；只生成现有 roster/Plan/补位共用的禁用集合。
use super::OperBox;
use crate::error::Result;
use crate::instances::OperatorInstances;
use crate::layout::FacilityKind;
use crate::skill_table::SkillTable;
use crate::tier::PromotionTier;

/// 按体系列关键来源岗位，不把 Rule 的可选成员或普通效率成员整体禁用。
const SYSTEM_ROLE_BANS: &[(&str, FacilityKind, &[&str])] = &[
    (
        "感知信息",
        FacilityKind::ControlCenter,
        &["夕", "焰狐龙梓兰"],
    ),
    ("感知信息", FacilityKind::Office, &["絮雨"]),
    ("感知信息", FacilityKind::Factory, &["迷迭香"]),
    ("感知信息", FacilityKind::TradePost, &["黑键"]),
    (
        "感知信息/无声共鸣",
        FacilityKind::Dormitory,
        &["爱丽丝", "车尔尼", "塑心"],
    ),
    ("人间烟火", FacilityKind::ControlCenter, &["重岳", "令"]),
    ("人间烟火", FacilityKind::Office, &["桑葚"]),
    ("人间烟火", FacilityKind::TradePost, &["乌有"]),
    ("红松林", FacilityKind::ControlCenter, &["焰尾", "薇薇安娜"]),
    // 只禁虚拟赤金来源；图耶仍可消费实际赤金线，杜林成员保留普通岗位。
    ("四赤金", FacilityKind::TradePost, &["鸿雪", "绮良"]),
    ("莱茵生命", FacilityKind::Factory, &["娜斯提"]),
    ("莱茵生命", FacilityKind::PowerPlant, &["缪尔赛思"]),
    ("S.E.E.S.", FacilityKind::Factory, &["结城理"]),
    ("精英干员挂件", FacilityKind::Office, &["凯尔希·思衡托"]),
    ("精英干员挂件", FacilityKind::TradePost, &["真言"]),
    ("魔物料理", FacilityKind::Dormitory, &["森西"]),
    (
        "宿舍人数挂件",
        FacilityKind::Dormitory,
        &["乌尔比安", "伺夜"],
    ),
    // 发电＋制造分支仍可用；仅禁依赖中枢森蚺的分支。
    ("中枢自动化", FacilityKind::ControlCenter, &["森蚺"]),
];

impl OperBox {
    pub(crate) fn with_auto_rotation_bans(
        &self,
        instances: &OperatorInstances,
        table: &SkillTable,
    ) -> Result<Self> {
        let mut result = self.clone();
        for (_, facility, names) in SYSTEM_ROLE_BANS {
            for name in *names {
                let roles = result
                    .facility_policy_exclusions
                    .entry((*name).into())
                    .or_default();
                if !roles.contains(facility) {
                    roles.push(*facility);
                }
            }
        }
        // 沿用 producer 目录的中枢禁用来源，覆盖所有练度和随机补位。
        // 喀兰、叙拉古、格拉斯哥、黑钢等来源由目录维护，不另抄姓名清单。
        let catalog = crate::response_dependency::producer_rule_catalog(table)?;
        for (_, instance) in instances.iter() {
            let name = &instance.name;
            if [PromotionTier::Tier0, PromotionTier::TierUp]
                .into_iter()
                .any(|tier| {
                    instances
                        .resolve_control_buff_ids(name, tier)
                        .iter()
                        .any(|buff_id| {
                            catalog
                                .rules
                                .iter()
                                .any(|rule| rule.source_buff_id == *buff_id)
                        })
                })
            {
                let roles = result
                    .facility_policy_exclusions
                    .entry(name.clone())
                    .or_default();
                if !roles.contains(&FacilityKind::ControlCenter) {
                    roles.push(FacilityKind::ControlCenter);
                }
            }
        }
        Ok(result)
    }
}
