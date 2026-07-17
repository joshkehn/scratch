//! Command-line entry point for the Obsidian Glean indexer.
//!
//! Usage:
//!   obsidian-glean-indexer <vault-dir> [-o <out.json>] [--pretty]
//!
//! Emits the Glean JSON fact document to stdout (or `-o` file) and a short
//! summary to stderr.

use obsidian_glean::index_vault;
use std::path::PathBuf;
use std::process::ExitCode;

struct Args {
    vault: PathBuf,
    output: Option<PathBuf>,
    pretty: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut vault: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut pretty = false;

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => return Err(String::new()),
            "--pretty" => pretty = true,
            "-o" | "--output" => {
                let v = it.next().ok_or("expected a path after -o/--output")?;
                output = Some(PathBuf::from(v));
            }
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("unknown option: {other}"));
            }
            other => {
                if vault.is_some() {
                    return Err(format!("unexpected extra argument: {other}"));
                }
                vault = Some(PathBuf::from(other));
            }
        }
    }

    Ok(Args {
        vault: vault.ok_or("missing <vault-dir> argument")?,
        output,
        pretty,
    })
}

fn usage() {
    eprintln!(
        "obsidian-glean-indexer — emit Glean facts (obsidian.notes) for an Obsidian vault\n\n\
         USAGE:\n    obsidian-glean-indexer <vault-dir> [-o <out.json>] [--pretty]\n\n\
         ARGS:\n    <vault-dir>          Path to the Obsidian vault to index\n\n\
         OPTIONS:\n    -o, --output <file>  Write JSON here instead of stdout\n    \
         --pretty             Pretty-print the JSON (default: compact)\n    \
         -h, --help           Show this help"
    );
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(msg) => {
            if !msg.is_empty() {
                eprintln!("error: {msg}\n");
            }
            usage();
            return if msg.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            };
        }
    };

    if !args.vault.is_dir() {
        eprintln!("error: {} is not a directory", args.vault.display());
        return ExitCode::FAILURE;
    }

    let (facts, stats) = match index_vault(&args.vault) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: failed to index vault: {e}");
            return ExitCode::FAILURE;
        }
    };

    let json = if args.pretty {
        serde_json::to_string_pretty(&facts)
    } else {
        serde_json::to_string(&facts)
    };
    let json = match json {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: failed to serialize facts: {e}");
            return ExitCode::FAILURE;
        }
    };

    if let Some(path) = &args.output {
        if let Err(e) = std::fs::write(path, &json) {
            eprintln!("error: failed to write {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    } else {
        println!("{json}");
    }

    eprintln!(
        "indexed {} files ({} notes): {} references, {} unresolved links, {} note-tag links",
        stats.files, stats.notes, stats.references, stats.unresolved, stats.tags
    );

    ExitCode::SUCCESS
}
