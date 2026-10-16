//! Pure CLI proposals: code is proposal data; a script from stdin is read after authorization.
use crate::{COMMAND_WORD, Eval, EvalInput, PythonProvider, SqliteEval};
use clap::{Parser, Subcommand};
use dekopon_provider_sdk::provider::{Proposal, Usage};

#[cfg(feature = "http")]
const AFTER_HELP: &str = "Proposes python.eval; python sql proposes python.sqlite-eval with sqlite3.connect() (pathless). Pure scripts need no HTTP grant.
Imports json, re, yaml, time (wall/monotonic), math, collections,
itertools, functools, decimal, fractions, csv, io (memory only), base64,
dekopon_numeric,
dekopon_requests (GET/HEAD only), and dekopon_subshell.
Every HTTP call is constrained by the host invocation grant; no credentials,
redirects, retries, sockets, filesystem, or input().
dekopon_subshell.run(script, stdin=None|INHERIT) runs a child Dekopon shell
script under this invocation's grants and returns CompletedRun(returncode,
stdout, stderr, truncated); stdout is capped at 65,536 bytes. INHERIT hands the
child the rest of this invocation's stdin; do not read parent stdin afterwards.
Non-empty child stdout ends in a newline; child stderr is empty in core 0.33;
a child panic returns status 70, and nonzero status is returned, not raised.
No input= bytes or OS subprocess is available.
Script: at most 65,536 UTF-8 bytes. print() is bounded to 65,536 bytes.
Assign a safe JSON-shaped value to result. Runtime output is bounded JSON with
ok/stdout/stdoutTruncated/result (or error), not an OS exit status.";
#[cfg(not(feature = "http"))]
const AFTER_HELP: &str = "Proposes python.eval; python sql proposes python.sqlite-eval with sqlite3.connect() (pathless). Imports json, re, yaml, time (wall/monotonic), math, collections, itertools, functools, decimal, fractions, csv, io (memory only), base64, and dekopon_numeric; no filesystem or network authority.";

#[cfg(feature = "tables")]
const TABLES_HELP: &str = "dekopon_tables is available for bounded in-memory SQL.";
#[cfg(not(feature = "tables"))]
const TABLES_HELP: &str = "dekopon_tables is not built into this component.";

#[cfg(all(feature = "http", feature = "tables"))]
pub(crate) const CAPABILITY_DESCRIPTION: &str = "Evaluate bounded Python with in-memory dekopon_tables SQL, dekopon_numeric, invocation time, broker-granted dekopon_requests GET/HEAD and dekopon_subshell child scripts; assign output to result";
#[cfg(all(feature = "http", not(feature = "tables")))]
pub(crate) const CAPABILITY_DESCRIPTION: &str = "Evaluate bounded Python with dekopon_numeric, invocation time, broker-granted dekopon_requests GET/HEAD and dekopon_subshell child scripts; assign output to result";
#[cfg(all(not(feature = "http"), feature = "tables"))]
pub(crate) const CAPABILITY_DESCRIPTION: &str = "Evaluate bounded Python with in-memory dekopon_tables SQL, dekopon_numeric and invocation time; assign output to result";
#[cfg(all(not(feature = "http"), not(feature = "tables")))]
pub(crate) const CAPABILITY_DESCRIPTION: &str =
    "Evaluate bounded Python with dekopon_numeric and invocation time; assign output to result";

#[cfg(not(feature = "http"))]
pub(crate) const ABOUT: &str =
    "Run one Python 3 script in a fresh, import-free RustPython 0.5.0 VM";
#[cfg(feature = "http")]
pub(crate) const ABOUT: &str =
    "Run one Python 3 script with broker-granted HTTP in a fresh RustPython 0.5.0 VM";

#[derive(Parser)]
#[command(
    name = COMMAND_WORD,
    version,
    about = ABOUT,
    override_usage = "python -c <CODE>\n       python - <<'EOF'\n       python <<'EOF'\n       python sql -c <CODE>",
    after_help = format!("{AFTER_HELP}\n{TABLES_HELP}"),
    args_conflicts_with_subcommands = true,
    group = clap::ArgGroup::new("script").args(["code", "piped"]),
)]
pub struct Python {
    /// Run CODE as the script
    #[arg(short = 'c', value_name = "CODE", allow_hyphen_values = true)]
    code: Option<String>,
    /// Read the script from the value piped into the word
    #[arg(value_name = "-", value_parser = ["-"], hide_possible_values = true)]
    piped: Option<String>,
    /// Propose a separately authorized persistent-SQL script
    #[command(subcommand)]
    command: Option<PythonCommand>,
}

#[derive(Subcommand)]
enum PythonCommand {
    /// Persistent SQL (requires python.sqlite-eval)
    #[command(
        after_help = "sqlite3 subset: connect() is pathless; execute(sql, tuple) runs one statement; fetchone()/fetchall() return at most 256 rows / 128 KiB per statement; commit(), rollback(), close() manage pending writes. dekopon_requests and dekopon_subshell import when built but calls raise 'http not granted' / 'spawn not granted' under this capability."
    )]
    Sql {
        /// Run CODE as the SQL-capable Python script
        #[arg(short = 'c', value_name = "CODE", allow_hyphen_values = true)]
        code: Option<String>,
        /// Read the SQL-capable script from stdin
        #[arg(value_name = "-", value_parser = ["-"], hide_possible_values = true)]
        piped: Option<String>,
    },
}

pub(crate) fn propose(args: Python, stdin_piped: bool) -> Result<Proposal<PythonProvider>, Usage> {
    let sql = args.command.is_some();
    let (code, piped) = match args.command {
        Some(PythonCommand::Sql { code, piped }) => (code, piped),
        None => (args.code, args.piped),
    };
    let (script, stdin_script) = match (code, piped) {
        (Some(code), None) => (code, false),
        (None, Some(_)) if stdin_piped => (String::new(), true),
        (None, Some(_)) => {
            return Err(Usage::new(if sql {
                "python sql -: nothing was piped in"
            } else {
                "python -: nothing was piped in"
            }));
        }
        (None, None) if stdin_piped => (String::new(), true),
        _ => {
            return Err(Usage::new(if sql {
                "python sql takes `-c <CODE>` or `-`"
            } else {
                "python takes `-c <CODE>` or `-`"
            }));
        }
    };
    let input = EvalInput {
        script,
        stdin_script,
    };
    Ok(if sql {
        Proposal::to::<SqliteEval>(input)
    } else {
        Proposal::to::<Eval>(input)
    })
}

#[cfg(test)]
mod tests {
    use super::TABLES_HELP;
    use crate::PythonProvider;
    use dekopon_provider_sdk::{CommandRunOutcome, provider};
    use serde_json::json;
    fn command(args: &[&str], piped: bool) -> CommandRunOutcome {
        provider::command::<PythonProvider>(
            &args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>(),
            piped,
        )
    }
    fn proposed(args: &[&str], piped: bool) -> serde_json::Value {
        let CommandRunOutcome::Proposed {
            capability,
            input,
            secret_use,
        } = command(args, piped)
        else {
            panic!("expected proposal")
        };
        assert_eq!(capability.as_str(), "python.eval");
        assert!(secret_use.is_none());
        input
    }
    #[test]
    fn pure_proposal_does_not_consume_script_and_preserves_piped_data_for_dash_c() {
        assert_eq!(
            proposed(&["-"], true),
            json!({"script":"", "stdin_script":true})
        );
        assert_eq!(proposed(&[], true), proposed(&["-"], true));
        assert_eq!(
            proposed(&["-c", "result = 42"], true),
            json!({"script":"result = 42"})
        );
        assert_eq!(proposed(&["-c", "-1"], true), json!({"script":"-1"}));
        let CommandRunOutcome::Proposed {
            capability, input, ..
        } = command(&["sql", "-c", "import sqlite3"], false)
        else {
            panic!("sql must propose")
        };
        assert_eq!(capability.as_str(), "python.sqlite-eval");
        assert_eq!(input, json!({"script":"import sqlite3"}));
        let CommandRunOutcome::Proposed {
            capability, input, ..
        } = command(&["sql", "-"], true)
        else {
            panic!("sql stdin must propose")
        };
        assert_eq!(capability.as_str(), "python.sqlite-eval");
        assert_eq!(input, json!({"script":"", "stdin_script":true}));
    }
    #[test]
    fn declined_inputs_and_help_are_safe() {
        for args in [
            &["-c"][..],
            &["script.py"],
            &["-c", "x", "-"],
            &["sql", "file.py"],
            &["-c", "x", "sql", "-c", "result=1"],
        ] {
            assert!(
                matches!(
                    command(args, false),
                    CommandRunOutcome::Rendered { status: 2, .. }
                ),
                "{args:?}: {:?}",
                command(args, false)
            );
        }
        assert!(matches!(
            command(&["sql", "-c", "x", "-"], false),
            CommandRunOutcome::Failed { .. }
        ));
        assert!(matches!(
            command(&["-"], false),
            CommandRunOutcome::Failed { .. }
        ));
        let CommandRunOutcome::Rendered {
            status: 0, stdout, ..
        } = command(&["--help"], false)
        else {
            panic!("expected safe help")
        };
        assert!(
            stdout.starts_with(
                "Run one Python 3 script with broker-granted HTTP in a fresh RustPython 0.5.0 VM"
            ) || stdout
                .starts_with("Run one Python 3 script in a fresh, import-free RustPython 0.5.0 VM"),
            "{stdout}"
        );
        let CommandRunOutcome::Rendered {
            status: 0,
            stdout: sql_help,
            ..
        } = command(&["sql", "--help"], false)
        else {
            panic!("expected safe SQL help")
        };
        for phrase in [
            "connect()",
            "execute(sql, tuple)",
            "fetchone()",
            "fetchall()",
            "commit()",
            "rollback()",
            "close()",
            "256 rows / 128 KiB",
            "http not granted",
            "spawn not granted",
        ] {
            assert!(sql_help.contains(phrase), "{phrase}: {sql_help}");
        }
        for phrase in [
            "sql",
            "RustPython 0.5.0",
            "dekopon_numeric",
            "time (wall/monotonic)",
            TABLES_HELP,
            "math",
            "collections",
            "itertools",
            "functools",
            "decimal",
            "fractions",
            "csv",
            "io (memory only)",
            "base64",
        ] {
            assert!(stdout.contains(phrase), "{phrase}");
        }
        let description = <crate::Eval as provider::Capability>::DESCRIPTION;
        for phrase in ["dekopon_numeric", "invocation time"] {
            assert!(description.contains(phrase), "{phrase}: {description}");
        }
        assert_eq!(
            description.contains("dekopon_tables"),
            cfg!(feature = "tables")
        );
        for phrase in ["dekopon_requests", "dekopon_subshell"] {
            assert_eq!(
                description.contains(phrase),
                cfg!(feature = "http"),
                "{phrase}: {description}"
            );
        }
        #[cfg(feature = "http")]
        for phrase in [
            "dekopon_subshell.run",
            "INHERIT",
            "status 70",
            "No input= bytes",
        ] {
            assert!(stdout.contains(phrase), "{phrase}");
        }
    }
}
