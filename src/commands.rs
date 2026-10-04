//! Pure CLI proposals: code is proposal data; a script from stdin is read after authorization.
use crate::{COMMAND_WORD, Eval, EvalInput, PythonProvider};
use clap::Parser;
use dekopon_provider_sdk::provider::{Proposal, Usage};

#[cfg(feature = "http")]
const AFTER_HELP: &str = "Proposes python.eval. Pure scripts need no HTTP grant.
Imports json, re, yaml, dekopon_requests (GET/HEAD only), and dekopon_subshell.
Every HTTP call is constrained by the host invocation grant; no credentials,
redirects, retries, sockets, filesystem, clock, or input().
dekopon_subshell.run(script, stdin=None|INHERIT) runs a child Dekopon shell
script under this invocation's grants and returns CompletedRun(returncode,
stdout, stderr, truncated); stdout is capped at 65,536 bytes. INHERIT hands the
child the rest of this invocation's stdin.
Script: at most 65,536 UTF-8 bytes. print() is bounded to 65,536 bytes.
Assign a safe JSON-shaped value to result. Runtime output is bounded JSON with
ok/stdout/stdoutTruncated/result (or error), not an OS exit status.";
#[cfg(not(feature = "http"))]
const AFTER_HELP: &str = "Proposes python.eval. Imports json, re, and yaml; no host authority.";

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
    override_usage = "python -c <CODE>\n       python - <<'EOF'\n       python <<'EOF'",
    after_help = AFTER_HELP,
    group = clap::ArgGroup::new("script").args(["code", "piped"]),
)]
pub struct Python {
    /// Run CODE as the script
    #[arg(short = 'c', value_name = "CODE", allow_hyphen_values = true)]
    code: Option<String>,
    /// Read the script from the value piped into the word
    #[arg(value_name = "-", value_parser = ["-"], hide_possible_values = true)]
    piped: Option<String>,
}

pub(crate) fn propose(args: Python, stdin_piped: bool) -> Result<Proposal<PythonProvider>, Usage> {
    let (script, stdin_script) = match (args.code, args.piped) {
        (Some(code), None) => (code, false),
        (None, Some(_)) if stdin_piped => (String::new(), true),
        (None, Some(_)) => return Err(Usage::new("python -: nothing was piped in")),
        (None, None) if stdin_piped => (String::new(), true),
        _ => return Err(Usage::new("python takes `-c <CODE>` or `-`")),
    };
    Ok(Proposal::to::<Eval>(EvalInput {
        script,
        stdin_script,
    }))
}

#[cfg(test)]
mod tests {
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
    }
    #[test]
    fn declined_inputs_and_help_are_safe() {
        for args in [&["-c"][..], &["script.py"], &["-c", "x", "-"]] {
            assert!(matches!(
                command(args, false),
                CommandRunOutcome::Rendered { status: 2, .. }
            ));
        }
        assert!(matches!(
            command(&["-"], false),
            CommandRunOutcome::Failed { .. }
        ));
        assert!(matches!(
            command(&["--help"], false),
            CommandRunOutcome::Rendered { status: 0, .. }
        ));
    }
}
