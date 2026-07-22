//! CLI for the generic Markdown Glean indexer.
//!
//! Usage: markdown-glean-indexer <dir> [-o <out.json>] [--pretty]

use markdown_glean::{index_corpus, NoDialect};
use std::path::PathBuf;
use std::process::ExitCode;

struct Args {
    root: PathBuf,
    output: Option<PathBuf>,
    pretty: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut root = None;
    let mut output = None;
    let mut pretty = false;
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => return Err(String::new()),
            "--pretty" => pretty = true,
            "-o" | "--output" => {
                output = Some(PathBuf::from(
                    it.next().ok_or("expected a path after -o/--output")?,
                ));
            }
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("unknown option: {other}"));
            }
            other => {
                if root.is_some() {
                    return Err(format!("unexpected extra argument: {other}"));
                }
                root = Some(PathBuf::from(other));
            }
        }
    }
    Ok(Args {
        root: root.ok_or("missing <dir> argument")?,
        output,
        pretty,
    })
}

fn usage() {
    eprintln!(
        "markdown-glean-indexer — emit Glean facts (markdown.*) for a Markdown corpus\n\n\
         USAGE:\n    markdown-glean-indexer <dir> [-o <out.json>] [--pretty]\n\n\
         OPTIONS:\n    -o, --output <file>  Write JSON here instead of stdout\n    \
         --pretty             Pretty-print the JSON\n    -h, --help           Show this help"
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

    if !args.root.is_dir() {
        eprintln!("error: {} is not a directory", args.root.display());
        return ExitCode::FAILURE;
    }

    let (facts, stats) = match index_corpus(&args.root, NoDialect::default()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: failed to index: {e}");
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
            eprintln!("error: failed to serialize: {e}");
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
        "indexed {} files ({} documents): {} headings, {} links, {} code blocks, {} tables, {} tasks",
        stats.files, stats.documents, stats.headings, stats.links, stats.code_blocks, stats.tables, stats.tasks
    );
    ExitCode::SUCCESS
}
