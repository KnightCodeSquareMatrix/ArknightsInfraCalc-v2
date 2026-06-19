use crate::roster::OperatorProgress;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PromotionTier {
    Tier0,
    TierUp,
}

impl PromotionTier {
    /// Resolve tier from operbox progress (elite + rarity + level).
    pub fn from_progress(progress: OperatorProgress) -> Self {
        Self::from_elite_rarity_level(progress.elite, progress.rarity, progress.level)
    }

    /// Game rules: 1–2★ lv30, 3–4★ E1, 5–6★ E2; rarity 0 = unknown → 5–6★ rule.
    pub fn from_elite_rarity_level(elite: u8, rarity: u8, level: u32) -> Self {
        let use_tier_up = match rarity {
            1 | 2 => level >= 30,
            3 | 4 => elite >= 1,
            0 => elite >= 2,
            _ => elite >= 2,
        };
        if use_tier_up {
            Self::TierUp
        } else {
            Self::Tier0
        }
    }

    /// Legacy: elite only, unknown rarity → conservative 5–6★ (E2) threshold.
    pub fn from_elite(elite: u8) -> Self {
        Self::from_elite_rarity_level(elite, 0, 1)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tier0 => "tier_0",
            Self::TierUp => "tier_up",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "tier_0" => Some(Self::Tier0),
            "tier_up" => Some(Self::TierUp),
            _ => None,
        }
    }

    /// 当前练度是否已启用 tier_up 技能（与 pool / search 一致）。
    pub fn is_tier_up(progress: OperatorProgress) -> bool {
        Self::from_progress(progress) == Self::TierUp
    }

    /// 距 tier_up 还差什么（按星级规则）。
    pub fn tier_up_requirement_label(progress: OperatorProgress) -> &'static str {
        match progress.rarity {
            1 | 2 => "30级",
            3 | 4 => "精1",
            _ => "精2",
        }
    }

    /// 练度简短描述（用于建议文案）。
    pub fn format_progress_brief(progress: OperatorProgress) -> String {
        if progress.rarity >= 1 {
            format!(
                "{}★精{} {}级",
                progress.rarity, progress.elite, progress.level
            )
        } else {
            format!("精{} {}级", progress.elite, progress.level)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_4star_e1_uses_tier_up() {
        assert_eq!(
            PromotionTier::from_elite_rarity_level(1, 4, 1),
            PromotionTier::TierUp
        );
    }

    #[test]
    fn tier_5star_e0_stays_tier_0() {
        assert_eq!(
            PromotionTier::from_elite_rarity_level(0, 5, 1),
            PromotionTier::Tier0
        );
    }

    #[test]
    fn tier_6star_e1_stays_tier_0() {
        assert_eq!(
            PromotionTier::from_elite_rarity_level(1, 6, 80),
            PromotionTier::Tier0
        );
    }

    #[test]
    fn tier_2star_lv30_uses_tier_up() {
        assert_eq!(
            PromotionTier::from_elite_rarity_level(0, 2, 30),
            PromotionTier::TierUp
        );
    }

    #[test]
    fn tier_2star_below_lv30_stays_tier_0() {
        assert_eq!(
            PromotionTier::from_elite_rarity_level(0, 2, 29),
            PromotionTier::Tier0
        );
    }

    #[test]
    fn tier_unknown_rarity_matches_legacy_elite_e2() {
        assert_eq!(PromotionTier::from_elite(2), PromotionTier::TierUp);
        assert_eq!(PromotionTier::from_elite(1), PromotionTier::Tier0);
    }
}
