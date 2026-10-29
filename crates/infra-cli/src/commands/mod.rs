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

fn timed_rotation_request_from_args(
    args: &[String],
) -> Result<infra_core::schedule::TimedRotationRequest, infra_core::Error> {
    match rotation_request_from_args(args)? {
        infra_core::schedule::RotationRequest::Timed(request) => Ok(request),
        infra_core::schedule::RotationRequest::Auto(_) => Err(infra_core::Error::msg(
            "自动轮换建议使用 plan --rotation auto@H1/H2",
        )),
    }
}

fn rotation_request_from_args(
    args: &[String],
) -> Result<infra_core::schedule::RotationRequest, infra_core::Error> {
    let Some(index) = args.iter().position(|arg| arg == "--rotation") else {
        return Ok(infra_core::schedule::RotationRequest::default());
    };
    if args.iter().skip(index + 1).any(|arg| arg == "--rotation") {
        return Err(infra_core::Error::msg(
            "--rotation must be supplied only once",
        ));
    }
    args.get(index + 1)
        .ok_or_else(|| infra_core::Error::msg("missing --rotation <profile-or-topology@hours>"))?
        .parse()
}
