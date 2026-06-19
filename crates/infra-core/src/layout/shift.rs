#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssignShiftMode {
    /// 高峰班：贸易 meta + 制造/发电贪心。
    Peak,
    /// 恢复班：贸易孑/余量 + 制造/发电次优；`seed` 钉死中枢/宿舍。
    Recovery,
}
