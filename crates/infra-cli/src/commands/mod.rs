mod advice;
mod bake;
mod debug;
mod layout;
mod plan;
mod plan_compute;
mod prefix_shadow;
mod profile;
mod serve;
pub(crate) mod verify;

pub use advice::advice_cmd;
pub use bake::bake_cmd;
pub use debug::debug_cmd;
pub use layout::layout_cmd;
pub use plan::plan_cmd;
pub use profile::profile_cmd;
pub use serve::serve_cmd;
pub use verify::verify_cmd;

fn timed_rotation_profile_from_args(
    args: &[String],
) -> Result<infra_core::schedule::TimedRotationProfile, infra_core::Error> {
    let Some(index) = args.iter().position(|arg| arg == "--rotation") else {
        return Ok(infra_core::schedule::TimedRotationProfile::default());
    };
    args.get(index + 1)
        .ok_or_else(|| infra_core::Error::msg("missing --rotation <profile>"))?
        .parse()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_cli_defaults_and_rejects_unnamed_four_shift() {
        assert_eq!(
            timed_rotation_profile_from_args(&[]).unwrap(),
            infra_core::schedule::TimedRotationProfile::Abc12_6_6
        );
        assert!(timed_rotation_profile_from_args(&["--rotation".into(), "4".into()]).is_err());
        assert!(timed_rotation_profile_from_args(&["--rotation".into()]).is_err());
    }
}
