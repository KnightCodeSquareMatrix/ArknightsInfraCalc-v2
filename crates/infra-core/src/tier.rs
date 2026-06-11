#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PromotionTier {
    Tier0,
    TierUp,
}

impl PromotionTier {
    pub fn from_elite(elite: u8) -> Self {
        if elite >= 2 {
            Self::TierUp
        } else {
            Self::Tier0
        }
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
}
