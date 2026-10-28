use std::path::PathBuf;
use std::sync::Arc;

use infra_core::{
    bake_and_publish_catalogs, bake_catalogs, current_exe_generator_fingerprint,
    default_baked_out_dir, validate_baked_catalog, verify_baked_catalog_responses, BakeOptions,
    BakeProgressEvent, Error,
};

use super::verify::run_all_regressions;

pub fn bake_cmd(args: &[String]) -> Result<(), Error> {
    let mut options = BakeOptions::default();
    options.out_dir = default_baked_out_dir()?;
    let mut mode = "all";
    let mut validate_only = false;
    let mut verify_only = false;
    let mut action_seen = false;

    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "all" | "trade" | "manufacture" => {
                if action_seen {
                    return Err(Error::msg("bake accepts exactly one action"));
                }
                mode = args[i].as_str();
                action_seen = true;
                i += 1;
            }
            "validate" => {
                if action_seen {
                    return Err(Error::msg("bake accepts exactly one action"));
                }
                validate_only = true;
                action_seen = true;
                i += 1;
            }
            "verify" => {
                if action_seen {
                    return Err(Error::msg("bake accepts exactly one action"));
                }
                verify_only = true;
                action_seen = true;
                i += 1;
            }
            "--out" => {
                let Some(path) = args.get(i + 1) else {
                    return Err(Error::msg("bake --out requires a path"));
                };
                options.out_dir = PathBuf::from(path);
                i += 2;
            }
            "--limit-per-signature" => {
                let Some(raw) = args.get(i + 1) else {
                    return Err(Error::msg(
                        "bake --limit-per-signature requires a positive integer",
                    ));
                };
                let limit = raw
                    .parse()
                    .map_err(|_| Error::msg("invalid --limit-per-signature value"))?;
                if limit == 0 {
                    return Err(Error::msg(
                        "bake --limit-per-signature requires a positive integer",
                    ));
                }
                options.limit_per_signature = Some(limit);
                i += 2;
            }
            "--help" | "-h" => {
                print_bake_usage();
                return Ok(());
            }
            other => {
                return Err(Error::msg(format!("unknown bake argument {other:?}")));
            }
        }
    }

    if (validate_only || verify_only) && options.limit_per_signature.is_some() {
        return Err(Error::msg(
            "--limit-per-signature is only valid while generating a catalog",
        ));
    }

    let generator = current_exe_generator_fingerprint()?;
    if validate_only || verify_only {
        validate_baked_catalog(&options.out_dir, &generator)?;
        eprintln!(
            "baked catalog is valid for current cli hash={} -> {}",
            generator.hash64,
            options.out_dir.display()
        );
        if verify_only {
            let verified = verify_baked_catalog_responses(&options.out_dir, &generator)?;
            eprintln!(
                "baked catalog verification passed: rows_checked={} row_mismatches={} queries_checked={} query_mismatches={} skipped_signatures={}",
                verified.rows_checked,
                verified.row_mismatches,
                verified.queries_checked,
                verified.query_mismatches,
                verified.skipped_signatures
            );
            run_all_regressions()?;
            eprintln!("mechanism regressions passed");
        }
        return Ok(());
    }
    options.generator = Some(generator);
    options.progress = Some(Arc::new(print_bake_progress));

    match mode {
        "all" => {
            options.include_trade = true;
            options.include_manufacture = true;
        }
        "trade" => {
            options.include_trade = true;
            options.include_manufacture = false;
        }
        "manufacture" => {
            options.include_trade = false;
            options.include_manufacture = true;
        }
        _ => unreachable!(),
    }

    let report = if options.limit_per_signature.is_some() {
        bake_catalogs(&options)?
    } else {
        bake_and_publish_catalogs(&options)?
    };
    eprintln!(
        "baked operators={} trade_signatures={} trade_hits={} trade_mechanism_rows={} trade_mechanism_signatures={} manufacture_signatures={} manufacture_hits={} elapsed={}ms -> {}",
        report.operator_count,
        report.trade_signatures,
        report.trade_hits,
        report.trade_mechanism_rows,
        report.trade_mechanism_signatures,
        report.manufacture_signatures,
        report.manufacture_hits,
        report.elapsed_ms,
        report.out_dir.display()
    );
    validate_baked_catalog(&options.out_dir, report.generator.as_ref().unwrap())?;
    let verified =
        verify_baked_catalog_responses(&options.out_dir, report.generator.as_ref().unwrap())?;
    eprintln!(
        "baked catalog validation passed: rows_checked={} row_mismatches={} queries_checked={} query_mismatches={} skipped_signatures={}",
        verified.rows_checked,
        verified.row_mismatches,
        verified.queries_checked,
        verified.query_mismatches,
        verified.skipped_signatures
    );
    run_all_regressions()?;
    eprintln!("mechanism regressions passed");
    Ok(())
}

fn print_bake_usage() {
    eprintln!("Usage:");
    eprintln!("  infra-cli bake [all|trade|manufacture] [--out <dir>] [--limit-per-signature <n>]");
    eprintln!("  infra-cli bake validate [--out <dir>]");
    eprintln!("  infra-cli bake verify [--out <dir>]");
    eprintln!("      Generates and validates indexed 3/2/1-person combo_table.bin, then runs regressions.");
}

fn print_bake_progress(event: BakeProgressEvent) {
    match event {
        BakeProgressEvent::Started {
            out_dir,
            operator_count,
            signature_count,
            worker_count,
        } => {
            eprintln!(
                "[bake] start operators={} signatures={} rayon_workers={} -> {}",
                operator_count,
                signature_count,
                worker_count,
                out_dir.display()
            );
        }
        BakeProgressEvent::SignatureStarted {
            facility,
            signature_key,
            combo_count,
        } => {
            eprintln!("[bake] {facility} {signature_key}: enumerating {combo_count} combos");
        }
        BakeProgressEvent::SignatureFinished {
            facility,
            signature_key,
            rows,
            elapsed_ms,
        } => {
            eprintln!("[bake] {facility} {signature_key}: rows={rows} elapsed={elapsed_ms}ms");
        }
        BakeProgressEvent::Writing { path, rows } => {
            if let Some(rows) = rows {
                eprintln!("[bake] write {} rows={rows}", path.display());
            } else {
                eprintln!("[bake] write {}", path.display());
            }
        }
        BakeProgressEvent::Finished {
            combo_table_rows,
            elapsed_ms,
        } => {
            eprintln!("[bake] done combo_table_rows={combo_table_rows} elapsed={elapsed_ms}ms");
        }
    }
}
