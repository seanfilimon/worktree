//! The line protocol Git speaks to a remote helper (gitremote-helpers(7)).
//!
//! Git writes one command per line on the helper's stdin and reads the
//! answers from its stdout. `fetch` and `push` commands arrive in batches
//! terminated by a blank line; a blank line (or end of input) where a
//! command is expected ends the conversation.
//!
//! This module only parses and formats; [`serve`] drives a [`Backend`] that
//! does the work, so the protocol is testable without Git or a server.

use std::io::{BufRead, Write};

use crate::error::{GitCompatError, Result};

/// Capabilities announced to Git, in order.
pub const CAPABILITIES: &[&str] = &["fetch", "push", "option"];

/// One command line sent by Git.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `capabilities`
    Capabilities,
    /// `list` or `list for-push`
    List { for_push: bool },
    /// `option <name> <value>`
    Option { name: String, value: String },
    /// `fetch <sha> <name>`
    Fetch(FetchSpec),
    /// `push [+]<src>:<dst>`
    Push(PushSpec),
}

/// One line of a `fetch` batch: Git wants object `sha` (advertised as `name`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchSpec {
    pub sha: String,
    pub name: String,
}

/// One line of a `push` batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushSpec {
    /// The refspec started with `+` (`git push --force`).
    pub force: bool,
    /// Local ref or object to push; `None` asks to delete `dst`.
    pub src: Option<String>,
    /// Full name of the remote ref (`refs/heads/main`).
    pub dst: String,
}

/// A ref advertised by `list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListedRef {
    /// `<sha> <name>`
    Object { name: String, sha: String },
    /// `@<target> <name>`: `name` is a symbolic ref to `target`.
    Symbolic { name: String, target: String },
}

/// Outcome of pushing one ref: `Ok` or the reason Git shows the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushResult {
    pub dst: String,
    pub outcome: std::result::Result<(), String>,
}

/// What a helper must be able to do; implemented by
/// [`super::Helper`] and by fakes in tests.
pub trait Backend {
    /// Set an option; `false` means the option is not supported.
    fn option(&mut self, name: &str, value: &str) -> bool;
    /// The remote's refs. For `for_push`, a remote tree that does not exist
    /// yet lists as empty instead of failing.
    fn list(&mut self, for_push: bool) -> Result<Vec<ListedRef>>;
    /// Make every wanted object (and its history) available locally.
    fn fetch(&mut self, wants: &[FetchSpec]) -> Result<()>;
    /// Push each refspec; one result per spec, in order.
    fn push(&mut self, specs: &[PushSpec]) -> Result<Vec<PushResult>>;
}

fn malformed(line: &str) -> GitCompatError {
    GitCompatError::ProtocolError(format!("unexpected remote-helper command: {line:?}"))
}

/// Parse one non-blank command line.
///
/// Errors with `ProtocolError` for unknown commands and malformed
/// arguments.
pub fn parse_command(line: &str) -> Result<Command> {
    let (verb, rest) = match line.split_once(' ') {
        Some((verb, rest)) => (verb, rest),
        None => (line, ""),
    };
    match verb {
        "capabilities" if rest.is_empty() => Ok(Command::Capabilities),
        "list" if rest.is_empty() => Ok(Command::List { for_push: false }),
        "list" if rest == "for-push" => Ok(Command::List { for_push: true }),
        "option" => {
            let (name, value) = rest.split_once(' ').ok_or_else(|| malformed(line))?;
            if name.is_empty() {
                return Err(malformed(line));
            }
            Ok(Command::Option {
                name: name.to_owned(),
                value: value.to_owned(),
            })
        }
        "fetch" => {
            let (sha, name) = rest.split_once(' ').ok_or_else(|| malformed(line))?;
            let hex = sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit());
            if !hex || name.is_empty() {
                return Err(malformed(line));
            }
            Ok(Command::Fetch(FetchSpec {
                sha: sha.to_ascii_lowercase(),
                name: name.to_owned(),
            }))
        }
        "push" => parse_push_spec(rest)
            .map(Command::Push)
            .ok_or_else(|| malformed(line)),
        _ => Err(malformed(line)),
    }
}

/// Parse `[+]<src>:<dst>`; `None` if there is no destination.
fn parse_push_spec(spec: &str) -> Option<PushSpec> {
    let (force, spec) = match spec.strip_prefix('+') {
        Some(rest) => (true, rest),
        None => (false, spec),
    };
    // Ref names cannot contain ':', so the last one separates src and dst.
    let (src, dst) = spec.rsplit_once(':')?;
    if dst.is_empty() {
        return None;
    }
    Some(PushSpec {
        force,
        src: (!src.is_empty()).then(|| src.to_owned()),
        dst: dst.to_owned(),
    })
}

/// A reason on one line, as `error <dst> <why>` requires.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Next line without its terminator; `None` at end of input.
fn read_line(input: &mut impl BufRead) -> Result<Option<String>> {
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    while line.ends_with('\n') || line.ends_with('\r') {
        line.pop();
    }
    Ok(Some(line))
}

/// Read the rest of a batch whose first line was already parsed: more
/// lines of the same command, up to the blank line.
fn read_batch<T>(
    input: &mut impl BufRead,
    first: T,
    mut pick: impl FnMut(Command) -> Option<T>,
) -> Result<Vec<T>> {
    let mut batch = vec![first];
    while let Some(line) = read_line(input)? {
        if line.is_empty() {
            break;
        }
        batch.push(pick(parse_command(&line)?).ok_or_else(|| malformed(&line))?);
    }
    Ok(batch)
}

/// Serve Git's commands from `input`, answering on `output`, until the
/// blank line or end of input that ends the conversation.
///
/// Errors (a malformed command, a failing backend, broken pipes) end the
/// conversation; the caller reports them on stderr and exits non-zero,
/// which Git turns into a failed fetch or push.
pub fn serve(
    backend: &mut impl Backend,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<()> {
    while let Some(line) = read_line(input)? {
        if line.is_empty() {
            break;
        }
        match parse_command(&line)? {
            Command::Capabilities => {
                for capability in CAPABILITIES {
                    writeln!(output, "{capability}")?;
                }
                writeln!(output)?;
            }
            Command::Option { name, value } => {
                let answer = if backend.option(&name, &value) {
                    "ok"
                } else {
                    "unsupported"
                };
                writeln!(output, "{answer}")?;
            }
            Command::List { for_push } => {
                for listed in backend.list(for_push)? {
                    match listed {
                        ListedRef::Object { name, sha } => writeln!(output, "{sha} {name}")?,
                        ListedRef::Symbolic { name, target } => {
                            writeln!(output, "@{target} {name}")?
                        }
                    }
                }
                writeln!(output)?;
            }
            Command::Fetch(first) => {
                let wants = read_batch(input, first, |c| match c {
                    Command::Fetch(spec) => Some(spec),
                    _ => None,
                })?;
                backend.fetch(&wants)?;
                writeln!(output)?;
            }
            Command::Push(first) => {
                let specs = read_batch(input, first, |c| match c {
                    Command::Push(spec) => Some(spec),
                    _ => None,
                })?;
                for result in backend.push(&specs)? {
                    match result.outcome {
                        Ok(()) => writeln!(output, "ok {}", result.dst)?,
                        Err(why) => writeln!(output, "error {} {}", result.dst, one_line(&why))?,
                    }
                }
                writeln!(output)?;
            }
        }
        output.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn parses_simple_commands() {
        assert_eq!(
            parse_command("capabilities").unwrap(),
            Command::Capabilities
        );
        assert_eq!(
            parse_command("list").unwrap(),
            Command::List { for_push: false }
        );
        assert_eq!(
            parse_command("list for-push").unwrap(),
            Command::List { for_push: true }
        );
        assert_eq!(
            parse_command("option verbosity 2").unwrap(),
            Command::Option {
                name: "verbosity".into(),
                value: "2".into()
            }
        );
        // Option values may contain spaces.
        assert_eq!(
            parse_command("option push-option a b").unwrap(),
            Command::Option {
                name: "push-option".into(),
                value: "a b".into()
            }
        );
    }

    #[test]
    fn parses_fetch() {
        let upper = SHA.to_ascii_uppercase();
        assert_eq!(
            parse_command(&format!("fetch {upper} refs/heads/main")).unwrap(),
            Command::Fetch(FetchSpec {
                sha: SHA.into(),
                name: "refs/heads/main".into()
            })
        );
        for bad in [
            "fetch".to_owned(),
            format!("fetch {SHA}"),
            format!("fetch {SHA} "),
            "fetch abc refs/heads/main".to_owned(),
            format!("fetch {}g refs/heads/main", &SHA[..39]),
        ] {
            assert!(
                matches!(parse_command(&bad), Err(GitCompatError::ProtocolError(_))),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn parses_push_refspecs() {
        let spec = |line: &str| match parse_command(line).unwrap() {
            Command::Push(spec) => spec,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            spec("push refs/heads/main:refs/heads/main"),
            PushSpec {
                force: false,
                src: Some("refs/heads/main".into()),
                dst: "refs/heads/main".into()
            }
        );
        assert_eq!(
            spec("push +HEAD:refs/heads/topic"),
            PushSpec {
                force: true,
                src: Some("HEAD".into()),
                dst: "refs/heads/topic".into()
            }
        );
        // Deletion: empty source.
        assert_eq!(
            spec("push :refs/heads/gone"),
            PushSpec {
                force: false,
                src: None,
                dst: "refs/heads/gone".into()
            }
        );
        for bad in ["push", "push refs/heads/main", "push refs/heads/main:"] {
            assert!(parse_command(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn rejects_unknown_commands() {
        for bad in [
            "import refs/heads/main",
            "connect git-upload-pack",
            "list x",
            "",
        ] {
            assert!(
                matches!(parse_command(bad), Err(GitCompatError::ProtocolError(_))),
                "{bad:?}"
            );
        }
    }

    /// Records calls and answers from canned data.
    #[derive(Default)]
    struct Fake {
        calls: Vec<String>,
        fail_fetch: bool,
    }

    impl Backend for Fake {
        fn option(&mut self, name: &str, value: &str) -> bool {
            self.calls.push(format!("option {name}={value}"));
            name == "verbosity"
        }

        fn list(&mut self, for_push: bool) -> Result<Vec<ListedRef>> {
            self.calls.push(format!("list {for_push}"));
            Ok(vec![
                ListedRef::Symbolic {
                    name: "HEAD".into(),
                    target: "refs/heads/main".into(),
                },
                ListedRef::Object {
                    name: "refs/heads/main".into(),
                    sha: SHA.into(),
                },
            ])
        }

        fn fetch(&mut self, wants: &[FetchSpec]) -> Result<()> {
            self.calls.push(format!("fetch x{}", wants.len()));
            if self.fail_fetch {
                return Err(GitCompatError::RemoteError("boom".into()));
            }
            Ok(())
        }

        fn push(&mut self, specs: &[PushSpec]) -> Result<Vec<PushResult>> {
            self.calls.push(format!("push x{}", specs.len()));
            Ok(specs
                .iter()
                .map(|spec| PushResult {
                    dst: spec.dst.clone(),
                    outcome: if spec.force {
                        Err("history is\nappend-only".into())
                    } else {
                        Ok(())
                    },
                })
                .collect())
        }
    }

    fn converse(fake: &mut Fake, input: &str) -> (Result<()>, String) {
        let mut out = Vec::new();
        let result = serve(fake, &mut input.as_bytes(), &mut out);
        (result, String::from_utf8(out).unwrap())
    }

    #[test]
    fn serves_a_fetch_conversation() {
        let mut fake = Fake::default();
        let input = format!(
            "capabilities\noption verbosity 1\noption depth 3\nlist\n\
             fetch {SHA} refs/heads/main\nfetch {SHA} refs/tags/v1\n\n\n"
        );
        let (result, out) = converse(&mut fake, &input);
        result.unwrap();
        assert_eq!(
            out,
            format!(
                "fetch\npush\noption\n\nok\nunsupported\n\
                 @refs/heads/main HEAD\n{SHA} refs/heads/main\n\n\n"
            )
        );
        assert_eq!(
            fake.calls,
            vec![
                "option verbosity=1",
                "option depth=3",
                "list false",
                "fetch x2"
            ]
        );
    }

    #[test]
    fn serves_a_push_batch_with_single_line_errors() {
        let mut fake = Fake::default();
        let input =
            "list for-push\npush refs/heads/a:refs/heads/a\npush +refs/heads/b:refs/heads/b\n\n";
        let (result, out) = converse(&mut fake, input);
        result.unwrap();
        assert!(out.ends_with("\n\nok refs/heads/a\nerror refs/heads/b history is append-only\n\n"));
        assert_eq!(fake.calls, vec!["list true", "push x2"]);
    }

    #[test]
    fn end_of_input_ends_the_conversation_and_batches() {
        let mut fake = Fake::default();
        // No trailing blank lines at all; CRLF line endings are tolerated.
        let (result, out) = converse(&mut fake, &format!("fetch {SHA} refs/heads/main\r\n"));
        result.unwrap();
        assert_eq!(out, "\n");
        assert_eq!(fake.calls, vec!["fetch x1"]);

        let (result, out) = converse(&mut Fake::default(), "");
        result.unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn errors_stop_the_conversation() {
        let mut fake = Fake {
            fail_fetch: true,
            ..Fake::default()
        };
        let (result, out) = converse(&mut fake, &format!("fetch {SHA} refs/heads/main\n\nlist\n"));
        assert!(matches!(result, Err(GitCompatError::RemoteError(_))));
        assert!(out.is_empty());
        assert_eq!(fake.calls, vec!["fetch x1"]);

        // A batch must not mix commands.
        let (result, _) = converse(
            &mut Fake::default(),
            &format!("fetch {SHA} refs/heads/main\nlist\n\n"),
        );
        assert!(matches!(result, Err(GitCompatError::ProtocolError(_))));

        let (result, _) = converse(&mut Fake::default(), "bogus\n");
        assert!(matches!(result, Err(GitCompatError::ProtocolError(_))));
    }
}
