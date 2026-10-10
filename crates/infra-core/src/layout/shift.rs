#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssignShiftMode {
    /// 高峰班：贸易 meta + 制造/发电贪心。
    Peak,
}
