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
        let mut context = options
            .player_context_path
            .as_deref()
            .map(load_player_context)
            .transpose()?
            .unwrap_or_default();
        if let Some(blueprint) = options
            .layout_path
            .as_deref()
            .map(BaseBlueprint::load)
            .transpose()?
        {
            context.apply_blueprint(&blueprint);
        }
        context.validate()?;
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
    player_context_path: Option<PathBuf>,
}

fn parse_args(args: &[String]) -> Result<AdviceCommandOptions, Error> {
    let mut operbox_path = None;
    let mut rules_path = None;
    let mut pretty = false;
    let mut explain = false;
    let mut answer = false;
    let mut player = false;
    let mut layout_path = None;
    let mut player_context_path = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--operbox" | "--rules" | "--layout" | "--player-context" => {
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
                    "--player-context" => {
                        if player_context_path.replace(path).is_some() {
                            return Err(Error::msg("duplicate --player-context"));
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
    if player_context_path.is_some() && !player {
        return Err(Error::msg("--player-context requires --player"));
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
        player_context_path,
    })
}

fn load_player_context(path: &std::path::Path) -> Result<PlayerAdviceContext, Error> {
    let raw = std::fs::read_to_string(path).map_err(|error| {
        Error::msg(format!(
            "player advice context read {}: {error}",
            path.display()
        ))
    })?;
    let context: PlayerAdviceContext = serde_json::from_str(&raw).map_err(|error| {
        Error::msg(format!(
            "player advice context parse {}: {error}",
            path.display()
        ))
    })?;
    context.validate()?;
    Ok(context)
}
