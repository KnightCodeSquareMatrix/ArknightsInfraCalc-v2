use std::path::PathBuf;

use infra_core::instances::{default_instances_path, OperatorInstances};
use infra_core::layout::BaseBlueprint;
use infra_core::operbox::OperBox;
use infra_core::skill_table::workspace_root;
use infra_core::training_advice::{
    build_player_training_advice, build_training_advice, build_training_advice_rag_input,
    default_training_advice_knowledge_path, default_training_recommendations_path,
    load_training_advice_knowledge, load_training_recommendation_rules,
    render_training_advice_answer, PlayerAdviceContext, TrainingAdviceBundle,
    TrainingAdviceOptions,
};
use infra_core::Error;

pub fn advice_cmd(args: &[String]) -> Result<(), Error> {
    let options = parse_args(args)?;
    let operbox_path = options.operbox_path;

    if options.player {
        let operbox = OperBox::load(&operbox_path)?;
        let knowledge = load_training_advice_knowledge(&default_training_advice_knowledge_path()?)?;
        let context = options
            .layout_path
            .as_deref()
            .map(BaseBlueprint::load)
            .transpose()?
            .as_ref()
            .map(PlayerAdviceContext::from_blueprint)
            .unwrap_or_default();
        let report = build_player_training_advice(&operbox, &knowledge, &context);
        let json = if options.pretty {
            serde_json::to_string_pretty(&report)?
        } else {
            serde_json::to_string(&report)?
        };
        println!("{json}");
        return Ok(());
    }

    let rules_path = options
        .rules_path
        .map(Ok)
        .unwrap_or_else(default_training_recommendations_path)?;

    let operbox = OperBox::load(&operbox_path)?;
    let instances = OperatorInstances::load(&default_instances_path()?)?;
    let rules = load_training_recommendation_rules(&rules_path)?;
    let report = build_training_advice(
        &operbox,
        &instances,
        &rules,
        &TrainingAdviceOptions {
            operbox_label: Some(operbox_path.to_string_lossy().into_owned()),
        },
    )?;

    if options.answer {
        print!("{}", render_training_advice_answer(&report));
        return Ok(());
    }

    let json = if options.explain {
        let rag_input = build_training_advice_rag_input(&report, &workspace_root()?);
        let bundle = TrainingAdviceBundle { report, rag_input };
        if options.pretty {
            serde_json::to_string_pretty(&bundle)?
        } else {
            serde_json::to_string(&bundle)?
        }
    } else if options.pretty {
        serde_json::to_string_pretty(&report)?
    } else {
        serde_json::to_string(&report)?
    };
    println!("{json}");
    Ok(())
}

struct AdviceCommandOptions {
    operbox_path: PathBuf,
    rules_path: Option<PathBuf>,
    pretty: bool,
    explain: bool,
    answer: bool,
    player: bool,
    layout_path: Option<PathBuf>,
}

fn parse_args(args: &[String]) -> Result<AdviceCommandOptions, Error> {
    let mut operbox_path = None;
    let mut rules_path = None;
    let mut pretty = false;
    let mut explain = false;
    let mut answer = false;
    let mut player = false;
    let mut layout_path = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--operbox" | "--rules" | "--layout" => {
                let flag = &args[index];
                let value = args.get(index + 1).filter(|value| !value.starts_with("--"));
                let Some(value) = value else {
                    return Err(Error::msg(format!("missing {flag} <path>")));
                };
                let path = PathBuf::from(value);
                match flag.as_str() {
                    "--operbox" => {
                        if operbox_path.replace(path).is_some() {
                            return Err(Error::msg("duplicate --operbox"));
                        }
                    }
                    "--rules" => {
                        if rules_path.replace(path).is_some() {
                            return Err(Error::msg("duplicate --rules"));
                        }
                    }
                    "--layout" => {
                        if layout_path.replace(path).is_some() {
                            return Err(Error::msg("duplicate --layout"));
                        }
                    }
                    _ => unreachable!(),
                }
                index += 2;
            }
            "--pretty" => {
                pretty = true;
                index += 1;
            }
            "--explain" => {
                explain = true;
                index += 1;
            }
            "--answer" => {
                answer = true;
                index += 1;
            }
            "--player" => {
                player = true;
                index += 1;
            }
            other => return Err(Error::msg(format!("unknown advice option {other:?}"))),
        }
    }
    if answer && explain {
        return Err(Error::msg("--answer conflicts with --explain"));
    }
    if answer && pretty {
        return Err(Error::msg("--answer conflicts with --pretty"));
    }
    if player && (answer || explain || rules_path.is_some()) {
        return Err(Error::msg(
            "--player conflicts with --answer, --explain, and --rules",
        ));
    }
    if layout_path.is_some() && !player {
        return Err(Error::msg("--layout requires --player"));
    }
    Ok(AdviceCommandOptions {
        operbox_path: operbox_path
            .ok_or_else(|| Error::msg("missing required --operbox <path>"))?,
        rules_path,
        pretty,
        explain,
        answer,
        player,
        layout_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_supported_advice_options() {
        let options = parse_args(&args(&[
            "--operbox",
            "box.json",
            "--rules",
            "rules.json",
            "--pretty",
            "--explain",
        ]))
        .unwrap();
        assert_eq!(options.operbox_path, PathBuf::from("box.json"));
        assert_eq!(options.rules_path, Some(PathBuf::from("rules.json")));
        assert!(options.pretty);
        assert!(options.explain);
        assert!(!options.answer);
        assert!(!options.player);
        assert!(options.layout_path.is_none());
    }

    #[test]
    fn rejects_missing_duplicate_and_unknown_options() {
        assert!(parse_args(&args(&["--pretty"])).is_err());
        assert!(parse_args(&args(&["--operbox", "--pretty"])).is_err());
        assert!(parse_args(&args(&[
            "--operbox",
            "first.json",
            "--operbox",
            "second.json",
        ]))
        .is_err());
        assert!(parse_args(&args(&["--operbox", "box.json", "--unknown"])).is_err());
        assert!(parse_args(&args(&["--operbox", "box.json", "--answer", "--explain"])).is_err());
        assert!(parse_args(&args(&["--operbox", "box.json", "--answer", "--pretty"])).is_err());
        assert!(parse_args(&args(&[
            "--operbox",
            "box.json",
            "--player",
            "--rules",
            "rules.json"
        ]))
        .is_err());
        assert!(parse_args(&args(&["--operbox", "box.json", "--layout", "layout.json"])).is_err());
    }

    #[test]
    fn parses_player_options_with_layout() {
        let options = parse_args(&args(&[
            "--operbox",
            "box.json",
            "--player",
            "--layout",
            "layout.json",
            "--pretty",
        ]))
        .unwrap();
        assert!(options.player);
        assert_eq!(options.layout_path, Some(PathBuf::from("layout.json")));
        assert!(options.pretty);
    }
}
