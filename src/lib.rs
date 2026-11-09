//! The tmux command-line protocol a pane host needs to serve Claude Code's
//! agent teams.
//!
//! Claude Code's split-pane teams do not draw panes. The lead runs a program
//! named `tmux` and asks it to split the window and run the teammate
//! (`split-window -d -t @0 -h -l 70% -P -F '#{pane_id}' -- claude --agent-name …`),
//! then reads the new pane's id back. Its backend registry knows two pane
//! managers, tmux and iTerm2, and picks tmux whenever `TMUX` is set in the
//! lead's environment. A pane host that wants those teammates as its own panes
//! answers to the name: it puts a `tmux` executable first on the pane's `PATH`,
//! sets `TMUX` and `TMUX_PANE`, and serves the verbs.
//!
//! This crate is the half of that work every host shares: parsing the argv,
//! expanding `#{…}` formats, splitting the teammate's shell-quoted command into
//! words, and running a verb against a [`Broker`], the host's own pane backend.
//! atrium and fwterm each implement [`Broker`]; nothing here touches a pane.
//!
//! The verb set is what the installed Claude Code (2.1.289) was read to use:
//! `-V`, `display-message -p`, `list-panes -F`, `list-windows -F`,
//! `split-window`, `new-window`, `kill-pane`, `send-keys`, and a set of layout
//! and style verbs honoured by doing nothing (`set-option`, `select-pane`,
//! `select-layout`, `resize-pane`, `show-environment`, `has-session`,
//! `new-session`, `switch-client`, `attach-session`). Anything else exits 1
//! with a message. `capture-pane` is refused on purpose: a host should never
//! hand one pane the text of another.
//!
//! Model: one window, `@0`, named after the host. Its panes are the caller and
//! the caller's direct children, which is the team as the lead sees it. Pane
//! ids are `%<id>`. Claude Code validates
//! `display-message -p '#{session_name}:#{window_id}.#{pane_id}'` against
//! `^[A-Za-z0-9_.-]{1,64}:@?\d+\.%?\d+$`, so a host's name must fit the first
//! class, and `tmux -V` must report a major of 2 or more.
//!
//! Zero third-party dependencies: the Rust standard library only.

/// The one window every pane reports.
pub const WINDOW_ID: &str = "@0";

/// Who is answering as tmux.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Host {
    /// `#{session_name}` and `#{window_name}`; `[A-Za-z0-9_.-]{1,64}`.
    pub session_name: &'static str,
    /// Appended to the version line: `tmux 3.5a-<suffix>`.
    pub version_suffix: &'static str,
}

impl Host {
    /// What `tmux -V` prints. Claude Code parses the major from `tmux (\d+)`
    /// and wants 2 or newer; the suffix says who answered.
    pub fn version_line(&self) -> String {
        format!("tmux 3.5a-{}", self.version_suffix)
    }
}

/// A `-t` target as the teams backend writes them: `%3`, `@0`, `atrium:@0.%3`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// A pane, by id.
    Pane(usize),
    /// Any window or session reference: there is one window.
    Window,
}

/// `%N` anywhere in the target names pane N; everything else is the window.
pub fn parse_target(s: &str) -> Target {
    if let Some(i) = s.rfind('%') {
        let digits: String = s[i + 1..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if let Ok(n) = digits.parse() {
            return Target::Pane(n);
        }
    }
    Target::Window
}

/// A parsed tmux command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verb {
    Version,
    DisplayMessage {
        target: Option<Target>,
        fmt: String,
    },
    ListPanes {
        fmt: String,
    },
    ListWindows {
        fmt: String,
    },
    SplitWindow {
        print: Option<String>,
        command: Option<String>,
    },
    NewWindow {
        name: Option<String>,
        print: Option<String>,
        command: Option<String>,
    },
    KillPane {
        target: Target,
    },
    SendKeys {
        target: Option<Target>,
        text: String,
    },
    /// Honoured by doing nothing, exit 0.
    Accept(String),
    /// Exit 1 with a message.
    Unsupported(String),
}

/// Global tmux options that take a value, skipped before the verb.
const GLOBAL_VALUED: &[&str] = &["-S", "-L", "-f", "-c", "-T"];
const ACCEPTED: &[&str] = &[
    "set-option",
    "set",
    "set-window-option",
    "setw",
    "select-pane",
    "selectp",
    "select-layout",
    "selectl",
    "resize-pane",
    "resizep",
    "show-environment",
    "showenv",
    "has-session",
    "has",
    "new-session",
    "new",
    "switch-client",
    "switchc",
    "attach-session",
    "attach",
    "select-window",
    "selectw",
    "refresh-client",
    "refresh",
];

/// The flags that take a value, per verb, so a positional is never mistaken
/// for a value or the other way round.
fn valued_flags(verb: &str) -> &'static [&'static str] {
    match verb {
        "split-window" | "splitw" => &["-t", "-l", "-c", "-e", "-F", "-T", "-p"],
        "new-window" | "neww" => &["-t", "-n", "-c", "-e", "-F"],
        "display-message" | "display" => &["-t", "-c", "-d", "-F"],
        "list-panes" | "lsp" | "list-windows" | "lsw" => &["-t", "-F", "-f"],
        "kill-pane" | "killp" => &["-t"],
        "send-keys" | "send" => &["-t", "-N", "-c"],
        _ => &[],
    }
}

/// Parse a tmux command line (without `argv[0]`) into the verb to serve.
pub fn parse(args: &[String]) -> Verb {
    let mut i = 0;
    // Global options: `-S path` (the socket Claude Code reads from `$TMUX`),
    // `-L name`, `-f file`, `-u`, `-2`, `-C`, `-CC`, `-V`.
    while i < args.len() {
        let a = args[i].as_str();
        if a == "-V" || a == "-v" || a == "--version" {
            return Verb::Version;
        }
        if GLOBAL_VALUED.contains(&a) {
            i += 2;
            continue;
        }
        if a.starts_with('-') && a.len() > 1 {
            i += 1;
            continue;
        }
        break;
    }
    let Some(verb) = args.get(i).map(String::as_str) else {
        return Verb::Unsupported("no command".to_string());
    };
    let rest = &args[i + 1..];
    let valued = valued_flags(verb);
    // Options -> (flag, value); `--` ends them; the remainder is positional.
    let mut opts: Vec<(String, Option<String>)> = Vec::new();
    let mut positional: Vec<String> = Vec::new();
    let mut j = 0;
    while j < rest.len() {
        let a = rest[j].as_str();
        if a == "--" {
            positional.extend(rest[j + 1..].iter().cloned());
            break;
        }
        if a.starts_with('-') && a.len() > 1 && positional.is_empty() {
            if valued.contains(&a) {
                opts.push((a.to_string(), rest.get(j + 1).cloned()));
                j += 2;
            } else {
                opts.push((a.to_string(), None));
                j += 1;
            }
            continue;
        }
        positional.push(a.to_string());
        j += 1;
    }
    let opt = |flag: &str| -> Option<String> {
        opts.iter()
            .rev()
            .find(|(f, _)| f == flag)
            .and_then(|(_, v)| v.clone())
    };
    let has = |flag: &str| opts.iter().any(|(f, _)| f == flag);
    let target = opt("-t").map(|t| parse_target(&t));
    let command = if positional.is_empty() {
        None
    } else {
        Some(positional.join(" "))
    };
    match verb {
        "display-message" | "display" => Verb::DisplayMessage {
            target,
            fmt: positional.join(" "),
        },
        "list-panes" | "lsp" => Verb::ListPanes {
            fmt: opt("-F").unwrap_or_else(|| "#{pane_id}".to_string()),
        },
        "list-windows" | "lsw" => Verb::ListWindows {
            fmt: opt("-F").unwrap_or_else(|| "#{window_name}".to_string()),
        },
        "split-window" | "splitw" => Verb::SplitWindow {
            print: has("-P").then(|| opt("-F").unwrap_or_else(|| "#{pane_id}".to_string())),
            command,
        },
        "new-window" | "neww" => Verb::NewWindow {
            name: opt("-n"),
            print: has("-P").then(|| opt("-F").unwrap_or_else(|| "#{pane_id}".to_string())),
            command,
        },
        "kill-pane" | "killp" => match target {
            Some(t) => Verb::KillPane { target: t },
            None => Verb::Unsupported("kill-pane: needs -t <pane>".to_string()),
        },
        "send-keys" | "send" => {
            // `send-keys -t %3 'text' Enter`: the keys are typed in order; a
            // trailing `Enter` is the submit the host's send adds anyway.
            let mut keys = positional.clone();
            if keys.last().map(String::as_str) == Some("Enter") {
                keys.pop();
            }
            Verb::SendKeys {
                target,
                text: keys.join(""),
            }
        }
        "capture-pane" | "capturep" => Verb::Unsupported(
            "capture-pane: this host does not hand one pane the text of another".to_string(),
        ),
        v if ACCEPTED.contains(&v) => Verb::Accept(v.to_string()),
        v => Verb::Unsupported(format!("{v}: not served by this tmux")),
    }
}

/// Split a POSIX-shell command string into words: whitespace separates, single
/// quotes are literal, double quotes keep `\"`, `\\`, `` \` `` and `\$` escapes,
/// a bare backslash escapes the next character. This is how tmux's command is
/// written (Claude Code shell-quotes each argument), and parsing it here means
/// no shell is needed on the machine, Windows included.
pub fn sh_words(cmd: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut chars = cmd.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                for d in chars.by_ref() {
                    if d == '\'' {
                        break;
                    }
                    cur.push(d);
                }
            }
            '"' => {
                in_word = true;
                while let Some(d) = chars.next() {
                    match d {
                        '"' => break,
                        '\\' => match chars.peek() {
                            Some(&e) if matches!(e, '"' | '\\' | '`' | '$') => {
                                cur.push(e);
                                chars.next();
                            }
                            _ => cur.push('\\'),
                        },
                        _ => cur.push(d),
                    }
                }
            }
            '\\' => {
                in_word = true;
                if let Some(d) = chars.next() {
                    cur.push(d);
                }
            }
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            c => {
                in_word = true;
                cur.push(c);
            }
        }
    }
    if in_word {
        words.push(cur);
    }
    words
}

/// The value after `--agent-name` in a teammate's argv: the pane's role label.
pub fn agent_name(argv: &[String]) -> Option<String> {
    argv.windows(2)
        .find(|w| w[0] == "--agent-name")
        .map(|w| w[1].clone())
        .or_else(|| {
            argv.iter()
                .find_map(|a| a.strip_prefix("--agent-name=").map(str::to_string))
        })
}

/// One pane as the shim shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneView {
    pub id: usize,
    pub title: String,
    pub role: Option<String>,
}

/// The window: the caller and its direct children, in broker order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub host: Host,
    pub caller: Option<usize>,
    pub panes: Vec<PaneView>,
}

impl View {
    pub fn pane(&self, id: usize) -> Option<&PaneView> {
        self.panes.iter().find(|p| p.id == id)
    }
    pub fn caller_pane(&self) -> Option<&PaneView> {
        self.caller.and_then(|c| self.pane(c))
    }
}

/// Expand tmux `#{variable}` references. Unknown variables expand to nothing,
/// as tmux does; `#{?…}` conditionals and `#[…]` styles are not needed by the
/// verbs served and pass through untouched.
pub fn expand(fmt: &str, pane: Option<&PaneView>, view: &View) -> String {
    let mut out = String::new();
    let mut rest = fmt;
    while let Some(i) = rest.find("#{") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[i..]);
            return out;
        };
        let var = &after[..end];
        let idx = pane
            .and_then(|p| view.panes.iter().position(|q| q.id == p.id))
            .unwrap_or(0);
        match var {
            "pane_id" => {
                if let Some(p) = pane {
                    out.push('%');
                    out.push_str(&p.id.to_string());
                }
            }
            "pane_index" => out.push_str(&idx.to_string()),
            "pane_title" => {
                if let Some(p) = pane {
                    out.push_str(p.role.as_deref().unwrap_or(&p.title));
                }
            }
            "window_id" => out.push_str(WINDOW_ID),
            "window_index" => out.push('0'),
            "window_name" | "session_name" => out.push_str(view.host.session_name),
            "window_panes" => out.push_str(&view.panes.len().to_string()),
            "session_id" => out.push_str("$0"),
            "client_control_mode" => out.push('0'),
            _ => {}
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// A broker node as the shim needs it: `(id, parent, role, title)`.
pub type Node = (usize, Option<usize>, Option<String>, String);

/// The host's pane backend. Everything a verb does to a pane goes through
/// here; the crate never touches one itself.
pub trait Broker {
    /// The pane the shim runs in, if known.
    fn caller(&self) -> Option<usize>;
    /// Every pane the caller may see, as `(id, parent, role, title)`.
    fn list(&mut self) -> Result<Vec<Node>, String>;
    /// `here`: tiled beside the caller; else a new window. Returns the new id.
    fn spawn(&mut self, here: bool, role: Option<&str>, argv: &[String]) -> Result<usize, String>;
    fn kill(&mut self, id: usize) -> Result<(), String>;
    fn send(&mut self, id: usize, text: &str) -> Result<(), String>;
}

/// What a served verb prints and how it exits.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Outcome {
    pub stdout: String,
    pub stderr: String,
    pub code: u8,
}

impl Outcome {
    pub fn ok(stdout: String) -> Outcome {
        Outcome {
            stdout,
            stderr: String::new(),
            code: 0,
        }
    }
    pub fn err(msg: impl Into<String>) -> Outcome {
        Outcome {
            stdout: String::new(),
            stderr: msg.into(),
            code: 1,
        }
    }
}

fn view_of(host: Host, broker: &mut dyn Broker) -> Result<View, String> {
    let caller = broker.caller();
    let nodes = broker.list()?;
    let mut panes: Vec<PaneView> = Vec::new();
    for (id, parent, role, title) in nodes {
        let mine = Some(id) == caller || (caller.is_some() && parent == caller);
        if mine {
            panes.push(PaneView { id, title, role });
        }
    }
    // The caller first, as tmux lists the window's original pane first.
    if let Some(c) = caller {
        if let Some(i) = panes.iter().position(|p| p.id == c) {
            let me = panes.remove(i);
            panes.insert(0, me);
        }
    }
    Ok(View {
        host,
        caller,
        panes,
    })
}

fn spawn_from(
    host: Host,
    broker: &mut dyn Broker,
    here: bool,
    name: Option<&str>,
    command: Option<&str>,
    print: Option<&str>,
    verb: &str,
) -> Outcome {
    let Some(command) = command else {
        return Outcome::err(format!(
            "{verb}: needs a command (this tmux does not open a bare shell)"
        ));
    };
    let argv = sh_words(command);
    if argv.is_empty() {
        return Outcome::err(format!("{verb}: empty command"));
    }
    let role = name.map(str::to_string).or_else(|| agent_name(&argv));
    let id = match broker.spawn(here, role.as_deref(), &argv) {
        Ok(id) => id,
        Err(e) => return Outcome::err(format!("{verb}: {e}")),
    };
    let Some(fmt) = print else {
        return Outcome::ok(String::new());
    };
    let view = match view_of(host, broker) {
        Ok(v) => v,
        Err(e) => return Outcome::err(format!("{verb}: {e}")),
    };
    let pane = view.pane(id).cloned().unwrap_or(PaneView {
        id,
        title: String::new(),
        role,
    });
    Outcome::ok(format!("{}\n", expand(fmt, Some(&pane), &view)))
}

/// Serve one parsed verb against the broker, as `host`.
pub fn run(verb: Verb, broker: &mut dyn Broker, host: Host) -> Outcome {
    match verb {
        Verb::Version => Outcome::ok(format!("{}\n", host.version_line())),
        Verb::Accept(_) => Outcome::ok(String::new()),
        Verb::Unsupported(msg) => Outcome::err(msg),
        Verb::DisplayMessage { target, fmt } => {
            let view = match view_of(host, broker) {
                Ok(v) => v,
                Err(e) => return Outcome::err(format!("display-message: {e}")),
            };
            let pane = match target {
                Some(Target::Pane(id)) => view.pane(id).cloned(),
                _ => view.caller_pane().cloned(),
            };
            Outcome::ok(format!("{}\n", expand(&fmt, pane.as_ref(), &view)))
        }
        Verb::ListPanes { fmt } => {
            let view = match view_of(host, broker) {
                Ok(v) => v,
                Err(e) => return Outcome::err(format!("list-panes: {e}")),
            };
            let mut out = String::new();
            for p in &view.panes {
                out.push_str(&expand(&fmt, Some(p), &view));
                out.push('\n');
            }
            Outcome::ok(out)
        }
        Verb::ListWindows { fmt } => {
            let view = match view_of(host, broker) {
                Ok(v) => v,
                Err(e) => return Outcome::err(format!("list-windows: {e}")),
            };
            Outcome::ok(format!("{}\n", expand(&fmt, view.caller_pane(), &view)))
        }
        Verb::SplitWindow { print, command } => spawn_from(
            host,
            broker,
            true,
            None,
            command.as_deref(),
            print.as_deref(),
            "split-window",
        ),
        Verb::NewWindow {
            name,
            print,
            command,
        } => spawn_from(
            host,
            broker,
            false,
            name.as_deref(),
            command.as_deref(),
            print.as_deref(),
            "new-window",
        ),
        Verb::KillPane { target } => match target {
            Target::Pane(id) => match broker.kill(id) {
                Ok(()) => Outcome::ok(String::new()),
                Err(e) => Outcome::err(format!("kill-pane: {e}")),
            },
            Target::Window => Outcome::err("kill-pane: names a window, not a pane"),
        },
        Verb::SendKeys { target, text } => {
            let id = match target {
                Some(Target::Pane(id)) => id,
                _ => match broker.caller() {
                    Some(c) => c,
                    None => return Outcome::err("send-keys: needs -t <pane>"),
                },
            };
            match broker.send(id, &text) {
                Ok(()) => Outcome::ok(String::new()),
                Err(e) => Outcome::err(format!("send-keys: {e}")),
            }
        }
    }
}

/// A broker with no panes, for verbs that never touch one (`-V`, the accepted
/// no-ops, the refusals). Every pane operation fails with "no session".
pub struct NoBroker;

impl Broker for NoBroker {
    fn caller(&self) -> Option<usize> {
        None
    }
    fn list(&mut self) -> Result<Vec<Node>, String> {
        Ok(Vec::new())
    }
    fn spawn(&mut self, _: bool, _: Option<&str>, _: &[String]) -> Result<usize, String> {
        Err("no session".to_string())
    }
    fn kill(&mut self, _: usize) -> Result<(), String> {
        Err("no session".to_string())
    }
    fn send(&mut self, _: usize, _: &str) -> Result<(), String> {
        Err("no session".to_string())
    }
}

/// Does `verb` need a [`Broker`] at all? `false` for `-V`, the accepted
/// no-ops and the refusals, so a host can answer those before it has a
/// session to connect to.
pub fn needs_broker(verb: &Verb) -> bool {
    !matches!(verb, Verb::Version | Verb::Accept(_) | Verb::Unsupported(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST: Host = Host {
        session_name: "atrium",
        version_suffix: "atrium",
    };

    fn a(s: &str) -> Vec<String> {
        s.split(' ')
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect()
    }

    #[derive(Default)]
    struct Fake {
        caller: Option<usize>,
        nodes: Vec<Node>,
        spawned: Vec<(bool, Option<String>, Vec<String>)>,
        killed: Vec<usize>,
        sent: Vec<(usize, String)>,
        next: usize,
    }
    impl Broker for Fake {
        fn caller(&self) -> Option<usize> {
            self.caller
        }
        fn list(&mut self) -> Result<Vec<Node>, String> {
            Ok(self.nodes.clone())
        }
        fn spawn(
            &mut self,
            here: bool,
            role: Option<&str>,
            argv: &[String],
        ) -> Result<usize, String> {
            self.spawned
                .push((here, role.map(str::to_string), argv.to_vec()));
            self.next += 1;
            let id = 100 + self.next;
            self.nodes
                .push((id, self.caller, role.map(str::to_string), argv[0].clone()));
            Ok(id)
        }
        fn kill(&mut self, id: usize) -> Result<(), String> {
            self.killed.push(id);
            Ok(())
        }
        fn send(&mut self, id: usize, text: &str) -> Result<(), String> {
            self.sent.push((id, text.to_string()));
            Ok(())
        }
    }

    fn team() -> Fake {
        Fake {
            caller: Some(3),
            nodes: vec![
                (1, None, None, "cmd".into()),
                (3, Some(1), Some("lead".into()), "claude".into()),
                (4, Some(3), Some("researcher".into()), "claude".into()),
                (9, Some(4), None, "claude".into()),
            ],
            ..Fake::default()
        }
    }

    #[test]
    fn version_is_a_tmux_two_or_newer_and_names_the_host() {
        assert_eq!(parse(&a("-V")), Verb::Version);
        let out = run(Verb::Version, &mut NoBroker, HOST);
        assert_eq!(out.code, 0);
        assert_eq!(out.stdout, "tmux 3.5a-atrium\n");
        assert!(!needs_broker(&Verb::Version));
        assert!(needs_broker(&parse(&a("list-panes"))));
    }

    #[test]
    fn targets_name_a_pane_or_the_window() {
        assert_eq!(parse_target("%3"), Target::Pane(3));
        assert_eq!(parse_target("atrium:@0.%12"), Target::Pane(12));
        assert_eq!(parse_target("@0"), Target::Window);
        assert_eq!(parse_target("atrium"), Target::Window);
    }

    #[test]
    fn the_claude_code_split_is_parsed_and_the_teammate_named() {
        let v = parse(&a(
            "split-window -d -t @0 -h -l 70% -P -F #{pane_id} -- claude --agent-id abc --agent-name researcher --team-name t1",
        ));
        let Verb::SplitWindow { print, command } = &v else {
            panic!("{v:?}");
        };
        assert_eq!(print.as_deref(), Some("#{pane_id}"));
        assert!(command.as_deref().unwrap().starts_with("claude --agent-id"));
        let mut b = team();
        let out = run(v.clone(), &mut b, HOST);
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert_eq!(out.stdout, "%101\n");
        let (here, role, argv) = &b.spawned[0];
        assert!(here);
        assert_eq!(role.as_deref(), Some("researcher"));
        assert_eq!(argv[0], "claude");
    }

    #[test]
    fn a_shell_quoted_command_becomes_argv_without_a_shell() {
        let w = sh_words(r#"claude --agent-name 'dev one' --flag "a \"b\" c" plain\ word"#);
        assert_eq!(
            w,
            vec![
                "claude",
                "--agent-name",
                "dev one",
                "--flag",
                "a \"b\" c",
                "plain word"
            ]
        );
        assert!(sh_words("   ").is_empty());
        assert_eq!(
            agent_name(&a("claude --agent-name=x")).as_deref(),
            Some("x")
        );
    }

    #[test]
    fn display_message_round_trips_the_pane_address_claude_validates() {
        let v = parse(&a(
            "display-message -p -t %3 #{session_name}:#{window_id}.#{pane_id}",
        ));
        assert_eq!(run(v, &mut team(), HOST).stdout, "atrium:@0.%3\n");
        let v = parse(&a("display-message -p #{window_id}"));
        assert_eq!(run(v, &mut team(), HOST).stdout, "@0\n");
        let v = parse(&a("display-message -p #{client_control_mode}"));
        assert_eq!(run(v, &mut team(), HOST).stdout, "0\n");
        let v = parse(&a("display-message -p #{nope}x"));
        assert_eq!(run(v, &mut team(), HOST).stdout, "x\n");
    }

    #[test]
    fn list_panes_is_the_caller_and_its_children_only() {
        let v = parse(&a("list-panes -t @0 -F #{pane_id}"));
        assert_eq!(run(v, &mut team(), HOST).stdout, "%3\n%4\n");
        let v = parse(&a("list-panes -F #{pane_title}"));
        assert_eq!(run(v, &mut team(), HOST).stdout, "lead\nresearcher\n");
        let v = parse(&a("list-windows -F #{window_name}:#{window_panes}"));
        assert_eq!(run(v, &mut team(), HOST).stdout, "atrium:2\n");
    }

    #[test]
    fn kill_and_send_reach_the_named_pane() {
        let mut b = team();
        assert_eq!(run(parse(&a("kill-pane -t %4")), &mut b, HOST).code, 0);
        assert_eq!(b.killed, vec![4]);
        assert_eq!(
            run(parse(&a("send-keys -t %4 hello Enter")), &mut b, HOST).code,
            0
        );
        assert_eq!(b.sent, vec![(4, "hello".to_string())]);
        assert_eq!(run(parse(&a("kill-pane -t @0")), &mut b, HOST).code, 1);
    }

    #[test]
    fn style_and_layout_verbs_are_honoured_by_doing_nothing() {
        for line in [
            "set-option -w -t @0 pane-border-status top",
            "select-pane -t %4 -T researcher",
            "select-layout -t @0 main-vertical",
            "resize-pane -t %3 -x 30%",
            "show-environment -g",
            "-S /tmp/sock has-session -t atrium",
        ] {
            let out = run(parse(&a(line)), &mut NoBroker, HOST);
            assert_eq!(out.code, 0, "{line}: {}", out.stderr);
            assert!(out.stdout.is_empty(), "{line}");
        }
    }

    #[test]
    fn capture_pane_and_unknown_verbs_exit_one() {
        assert_eq!(
            run(parse(&a("capture-pane -p -t %4")), &mut team(), HOST).code,
            1
        );
        assert_eq!(run(parse(&a("choose-tree")), &mut team(), HOST).code, 1);
        assert_eq!(run(parse(&a("")), &mut team(), HOST).code, 1);
        assert_eq!(run(parse(&a("split-window -P")), &mut team(), HOST).code, 1);
    }

    #[test]
    fn new_window_opens_a_window_named_by_n() {
        let mut b = team();
        let out = run(
            parse(&a(
                "new-window -t atrium -n worker -P -F #{pane_id} -- claude",
            )),
            &mut b,
            HOST,
        );
        assert_eq!(out.stdout, "%101\n");
        let (here, role, _) = &b.spawned[0];
        assert!(!here);
        assert_eq!(role.as_deref(), Some("worker"));
    }
}
