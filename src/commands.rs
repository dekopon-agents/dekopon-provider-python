//! The `python` command word: the interpreter's own argv, parsed in the guest.
//!
//! `python -c CODE` runs CODE, and `python -` runs the value piped into the word, so a script
//! travels as a here-document: `python - <<'EOF' … EOF`. Both become a proposal for `python.eval`
//! carrying exactly the input a direct call sends, `{"script": …}`, which is then authorized like
//! any other call. `python --help`, `python --version`, and every usage error are rendered here and
//! authorize nothing. No VM exists until the proposal is invoked, so running the word costs no
//! interpreter startup.
//!
//! There is no `python FILE` and no trailing `sys.argv`: the component has no filesystem and the
//! capability takes a script and nothing else, so clap refuses both.

use dekopon_provider_sdk::clap::{self, CommandFactory, FromArgMatches, Parser};
use dekopon_provider_sdk::{CommandInvocation, CommandRun, ProviderError, cli};
use serde_json::json;

use crate::{COMMAND_WORD, EVAL};

/// The argument that means "read the script from the value piped into the word".
const PIPED: &str = "-";

/// What `--help` prints below the options: the contract a model needs before writing a script.
const AFTER_HELP: &str = "\
The script is Python 3 source, at most 65,536 UTF-8 bytes. Only json, re, and yaml
(safe_load, safe_dump) import; there is no filesystem, network, clock, or input().
print() output is captured. Assign the value to return to `result`: null, bool, number,
string, list, or dict with string keys.

Output is JSON:
  {\"ok\":true,\"stdout\":\"...\",\"stdoutTruncated\":false,\"result\":...}
  {\"ok\":false,\"stdout\":\"...\",\"stdoutTruncated\":false,\"error\":{\"kind\":\"runtime\",\"type\":\"ValueError\",\"message\":\"...\"}}
Error kinds are syntax, runtime, yaml, and result.

Examples:
  python -c 'result = sum(i * i for i in range(5))'
  python - <<'EOF' | jq .result
  import yaml
  result = yaml.safe_load(\"retries: 2\")
  EOF";

// The `python` tree, declared once and rendered by clap. Plain comments, not doc comments: clap
// renders a doc comment as the `about` line above `Usage:`.
#[derive(Parser)]
#[command(
    name = COMMAND_WORD,
    version,
    about = "Run one Python 3 script in a fresh, import-free RustPython 0.5.0 VM",
    override_usage = "python -c <CODE>\n       python - <<'EOF'",
    after_help = AFTER_HELP,
    group = clap::ArgGroup::new("script").args(["code", "piped"]).required(true),
)]
struct Python {
    /// Run CODE as the script
    #[arg(short = 'c', value_name = "CODE", allow_hyphen_values = true)]
    code: Option<String>,
    /// Read the script from the value piped into the word
    #[arg(value_name = PIPED, value_parser = [PIPED], hide_possible_values = true)]
    piped: Option<String>,
}

/// Runs one `python` argv.
pub(crate) fn run(argv: &[String], stdin: Option<&str>) -> Result<CommandRun, ProviderError> {
    cli::run_command(Python::command(), argv, stdin, dispatch)
}

/// Turns clap's matches into the `python.eval` proposal.
///
/// Runs only after clap accepted exactly one of `-c` and `-`, so what is left to decide is what
/// clap cannot know: whether anything was piped. The script's size bound is checked once, in
/// `invoke`, against the input a direct call would send too.
fn dispatch(
    matches: clap::ArgMatches,
    stdin: Option<&str>,
) -> Result<CommandInvocation, ProviderError> {
    let python = Python::from_arg_matches(&matches)
        .map_err(|error| ProviderError::new("usage", error.to_string()))?;
    let script = match (python.code, python.piped) {
        (Some(code), None) => code,
        (None, Some(_)) => stdin
            .map(str::to_owned)
            .ok_or_else(|| ProviderError::new("usage", "python -: nothing was piped in"))?,
        _ => {
            return Err(ProviderError::new(
                "usage",
                "python takes `-c <CODE>` or `-`",
            ));
        }
    };
    Ok(CommandInvocation {
        capability: EVAL.parse().expect("static capability ID"),
        input: json!({ "script": script }),
    })
}

#[cfg(test)]
mod tests {
    use dekopon_provider_sdk::{CommandInvocation, CommandRun, Provider};
    use serde_json::json;

    use super::run;
    use crate::{EVAL, PythonProvider};

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    fn rendered(words: &[&str], stdin: Option<&str>) -> (String, String, u8) {
        let run = run(&argv(words), stdin).expect("clap answers are rendered, not declined");
        let CommandRun::Rendered {
            stdout,
            stderr,
            status,
        } = run
        else {
            panic!("expected rendered text for {words:?}, got {run:?}");
        };
        (stdout, stderr, status)
    }

    fn proposal(words: &[&str], stdin: Option<&str>) -> CommandInvocation {
        match run(&argv(words), stdin).expect("a well-formed argv proposes") {
            CommandRun::Proposal(invocation) => invocation,
            other => panic!("expected a proposal for {words:?}, got {other:?}"),
        }
    }

    /// The help page, byte for byte. It is the only documentation a model reads before writing a
    /// script, so a change to it is a diff a reviewer sees.
    #[test]
    fn help_is_pinned_byte_for_byte() {
        const HELP: &str = "\
Run one Python 3 script in a fresh, import-free RustPython 0.5.0 VM

Usage: python -c <CODE>
       python - <<'EOF'

Arguments:
  [-]  Read the script from the value piped into the word

Options:
  -c <CODE>      Run CODE as the script
  -h, --help     Print help
  -V, --version  Print version

The script is Python 3 source, at most 65,536 UTF-8 bytes. Only json, re, and yaml
(safe_load, safe_dump) import; there is no filesystem, network, clock, or input().
print() output is captured. Assign the value to return to `result`: null, bool, number,
string, list, or dict with string keys.

Output is JSON:
  {\"ok\":true,\"stdout\":\"...\",\"stdoutTruncated\":false,\"result\":...}
  {\"ok\":false,\"stdout\":\"...\",\"stdoutTruncated\":false,\"error\":{\"kind\":\"runtime\",\"type\":\"ValueError\",\"message\":\"...\"}}
Error kinds are syntax, runtime, yaml, and result.

Examples:
  python -c 'result = sum(i * i for i in range(5))'
  python - <<'EOF' | jq .result
  import yaml
  result = yaml.safe_load(\"retries: 2\")
  EOF
";
        for words in [&["--help"][..], &["-h"][..], &["-c", "x", "--help"][..]] {
            let (stdout, stderr, status) = rendered(words, None);
            assert_eq!(status, 0, "{words:?}");
            assert_eq!(stdout, HELP, "{words:?}");
            assert!(stderr.is_empty(), "{stderr}");
        }

        let (stdout, _, status) = rendered(&["--version"], None);
        assert_eq!(status, 0);
        assert_eq!(stdout, format!("python {}\n", env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn usage_errors_render_on_stderr_at_status_two() {
        for words in [
            &[][..],
            &["-c"][..],
            &["-c", "result = 1", "-"][..],
            &["script.py"][..],
            &["-c", "result = 1", "extra"][..],
            &["-", "-"][..],
            &["-i"][..],
        ] {
            for stdin in [None, Some("result = 1")] {
                let (stdout, stderr, status) = rendered(words, stdin);
                assert_eq!(status, 2, "{words:?}");
                assert!(stdout.is_empty(), "{words:?}: {stdout}");
                assert!(stderr.starts_with("error: "), "{words:?}: {stderr}");
            }
        }

        // No script is the usage error a model meets when it pipes a script without `-`, so the
        // usage lines it prints are the two forms that work. Piped or not, nothing is proposed.
        const NO_SCRIPT: &str = "\
error: the following required arguments were not provided:
  <-c <CODE>|->

Usage: python -c <CODE>
       python - <<'EOF'

For more information, try '--help'.
";
        for stdin in [None, Some("result = 1")] {
            let (_, stderr, _) = rendered(&[], stdin);
            assert_eq!(stderr, NO_SCRIPT, "{stdin:?}");
        }

        // There is no filesystem, so a file argument names the one positional value that exists.
        let (_, stderr, _) = rendered(&["script.py"], None);
        assert!(
            stderr.contains("invalid value 'script.py' for '[-]'"),
            "{stderr}"
        );
    }

    #[test]
    fn a_dash_without_a_piped_value_is_declined_naming_the_cause() {
        let error = run(&argv(&["-"]), None)
            .expect_err("a decline, reported to the model as a usage error at status 2");
        assert_eq!(error.code(), "usage");
        assert_eq!(error.message(), "python -: nothing was piped in");
    }

    #[test]
    fn dash_c_proposes_the_exact_invoke_input() {
        let invocation = proposal(&["-c", "print('hi')\nresult = 6 * 7"], None);
        assert_eq!(invocation.capability.as_str(), EVAL);
        assert_eq!(
            invocation.input,
            json!({"script": "print('hi')\nresult = 6 * 7"})
        );

        // Code is taken verbatim even when it looks like a flag, as the interpreter's `-c` does,
        // and a piped value is ignored rather than merged in.
        let invocation = proposal(&["-c", "-1"], Some("ignored"));
        assert_eq!(invocation.input, json!({"script": "-1"}));
    }

    #[test]
    fn dash_proposes_the_piped_value_as_the_exact_invoke_input() {
        let piped = "import json\nresult = json.loads('[1, 2]')\n";
        let invocation = proposal(&["-"], Some(piped));
        assert_eq!(invocation.capability.as_str(), EVAL);
        assert_eq!(invocation.input, json!({"script": piped}));

        // An empty here-document is an empty script, which Python runs to `result = None`.
        let invocation = proposal(&["-"], Some(""));
        assert_eq!(invocation.input, json!({"script": ""}));
    }

    /// The word deliberately does not repeat the script's size bound: `invoke` enforces it on the
    /// proposal exactly as on a direct call, before any VM is constructed. `tests/broker.rs` runs a
    /// proposal to completion against the real host.
    #[test]
    fn invoke_bounds_an_oversized_proposal_before_any_vm() {
        let oversized = "x".repeat(crate::limits::SCRIPT_BYTES + 1);
        let invocation = proposal(&["-"], Some(&oversized));
        let error = PythonProvider::invoke(&invocation.capability, invocation.input)
            .expect_err("invoke bounds the script");
        assert_eq!(error.code(), "input-too-large");
    }

    /// The rendered text is plain: the SDK's clap is built without `color`, so no escape byte can
    /// reach a model's transcript.
    #[test]
    fn no_rendered_text_contains_an_escape_byte() {
        for words in [&["--help"][..], &["--version"][..], &[][..], &["-c"][..]] {
            let (stdout, stderr, _) = rendered(words, None);
            assert!(!stdout.contains('\u{1b}'), "{words:?}: {stdout:?}");
            assert!(!stderr.contains('\u{1b}'), "{words:?}: {stderr:?}");
        }
    }

    /// Every capability the word can propose is one the manifest declares, under the word the
    /// manifest declares. Without this, a renamed capability would reach a model at runtime as an
    /// authorization denial.
    #[test]
    fn every_dispatch_target_is_declared_in_the_manifest() {
        let manifest = PythonProvider::manifest();
        assert_eq!(manifest.command_words, [crate::COMMAND_WORD]);
        let declared: Vec<String> = manifest
            .capabilities
            .iter()
            .map(|capability| capability.id.to_string())
            .collect();
        for (words, stdin) in [(&["-c", "result = 1"][..], None), (&["-"][..], Some("x"))] {
            let invocation = proposal(words, stdin);
            assert!(
                declared.contains(&invocation.capability.to_string()),
                "{words:?} proposes {} which the manifest does not declare",
                invocation.capability
            );
        }
    }
}
