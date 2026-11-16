//! The command string Claude Code hands `respawn-pane` to start a teammate,
//! parsed without a shell.
//!
//! Claude Code 2.1.325 builds one POSIX shell command line and gives it to
//! tmux, which would run it with `sh -c`:
//!
//! ```text
//! cd <cwd> && env <NAME=value>... <program> [<arg>...]
//! ```
//!
//! optionally led, on Linux with its tool-cgroup feature, by
//!
//! ```text
//! sh -c '{ echo $PPID > "$1"/cgroup.procs; } 2>/dev/null' sh <dir>;
//! ```
//!
//! Each word is quoted by Claude Code's own quoting function: left bare when
//! it is all `A-Z a-z 0-9 _ . / : = @ + , -`, otherwise wrapped in single
//! quotes with every `'` written `'"'"'`; an empty word is `''`.
//!
//! A pane host that serves `respawn-pane` must not hand this string to a
//! shell: it comes from a process inside a pane, and on Windows there is no
//! `sh`, `cd` or `env` to run it anyway. [`parse_teammate_command`] accepts
//! exactly the grammar above and nothing else. Anything a shell would act on
//! outside a quote (a second command, a pipe, a redirection, an expansion, a
//! glob, a comment) is an error with a reason, never a word.

/// The Linux tool-cgroup prefix, up to the directory word, exactly as Claude
/// Code 2.1.325 writes it. Recognised and dropped: a pane host places the
/// teammate's process itself.
pub const CGROUP_PREFIX: &str = r#"sh -c '{ echo $PPID > "$1"/cgroup.procs; } 2>/dev/null' sh "#;

/// The longest command string parsed: 24 KiB. The real one is a few
/// kilobytes (its environment list is long). The bound is set so that a
/// line of this size still fits a host's request once it is JSON-escaped
/// (atrium's control channel carries 64 KiB a request, and a line of
/// Windows paths doubles its backslashes), so an oversized line is refused
/// here, with this crate's reason, and not by a transport underneath.
pub const MAX_COMMAND_BYTES: usize = 24 * 1024;

/// A teammate command, taken apart: where it runs, the environment
/// assignments in the order written, and the program with its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeammateCommand {
    /// The word after `cd`.
    pub cwd: String,
    /// Each `NAME=value` after `env`, in order; a value may be empty.
    pub env: Vec<(String, String)>,
    /// The program and its arguments; never empty.
    pub argv: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    /// A word after quote removal. `bare`: no quote or backslash was used.
    Word { text: String, bare: bool },
    /// An unquoted `&&`.
    AndAnd,
    /// An unquoted `;`.
    Semi,
}

/// May `c` stand unquoted in a word? The set Claude Code's quoting leaves
/// bare; everything else it wraps in quotes, so everything else, unquoted, is
/// outside the grammar.
fn is_bare(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | ':' | '=' | '@' | '+' | ',' | '-')
}

/// A character for an error message: itself when it is printable ASCII, its
/// code point otherwise, so a message never carries a character that hides
/// or reorders text.
fn shown(c: char) -> String {
    if c.is_ascii_graphic() {
        format!("`{c}`")
    } else {
        format!("U+{:04X}", c as u32)
    }
}

/// Why an unquoted `c` is refused.
fn unquoted(c: char, at: usize) -> String {
    let what = match c {
        '|' => "a pipe",
        '&' => "a background job or a second `&&`",
        ';' => "a command separator",
        '$' => "an expansion",
        '`' => "a command substitution",
        '(' | ')' => "a subshell",
        '<' | '>' => "a redirection",
        '*' | '?' | '[' | ']' => "a glob",
        '{' | '}' => "a brace group or expansion",
        '~' => "a home-directory expansion",
        '#' => "a comment",
        '!' => "a history expansion or negation",
        _ => "not a character the teammate command leaves unquoted",
    };
    format!("an unquoted {} at byte {at}: {what}", shown(c))
}

fn lex(s: &str) -> Result<Vec<(Tok, usize)>, String> {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let mut toks: Vec<(Tok, usize)> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (at, c) = chars[i];
        match c {
            ' ' => {
                i += 1;
                continue;
            }
            '&' => {
                if chars.get(i + 1).map(|p| p.1) == Some('&') {
                    toks.push((Tok::AndAnd, at));
                    i += 2;
                    continue;
                }
                return Err(unquoted('&', at));
            }
            ';' => {
                toks.push((Tok::Semi, at));
                i += 1;
                continue;
            }
            _ => {}
        }
        let mut text = String::new();
        let mut bare = true;
        while let Some(&(pos, c)) = chars.get(i) {
            match c {
                ' ' | '&' | ';' => break,
                '\'' => {
                    bare = false;
                    i += 1;
                    loop {
                        match chars.get(i).map(|p| p.1) {
                            None => {
                                return Err(format!(
                                    "a single quote opened at byte {pos} is never closed"
                                ))
                            }
                            Some('\'') => {
                                i += 1;
                                break;
                            }
                            Some(d) => {
                                text.push(d);
                                i += 1;
                            }
                        }
                    }
                }
                '"' => {
                    bare = false;
                    i += 1;
                    loop {
                        match chars.get(i).copied() {
                            None => {
                                return Err(format!(
                                    "a double quote opened at byte {pos} is never closed"
                                ))
                            }
                            Some((_, '"')) => {
                                i += 1;
                                break;
                            }
                            // POSIX: inside double quotes a backslash escapes
                            // only `"`, `\`, `$` and a backquote; before any
                            // other character it is itself.
                            Some((_, '\\')) => match chars.get(i + 1).map(|p| p.1) {
                                Some(e) if matches!(e, '"' | '\\' | '$' | '`') => {
                                    text.push(e);
                                    i += 2;
                                }
                                Some(_) => {
                                    text.push('\\');
                                    i += 1;
                                }
                                None => {
                                    return Err(format!(
                                        "a double quote opened at byte {pos} is never closed"
                                    ))
                                }
                            },
                            Some((q, d @ ('$' | '`'))) => {
                                return Err(format!(
                                    "{} inside double quotes at byte {q}: an expansion",
                                    shown(d)
                                ))
                            }
                            Some((_, d)) => {
                                text.push(d);
                                i += 1;
                            }
                        }
                    }
                }
                '\\' => {
                    bare = false;
                    match chars.get(i + 1).map(|p| p.1) {
                        Some(d) => {
                            text.push(d);
                            i += 2;
                        }
                        None => return Err("a backslash at the end escapes nothing".to_string()),
                    }
                }
                c if is_bare(c) => {
                    text.push(c);
                    i += 1;
                }
                c => return Err(unquoted(c, pos)),
            }
        }
        toks.push((Tok::Word { text, bare }, at));
    }
    Ok(toks)
}

/// Is `name` a variable name: a letter or `_`, then letters, digits and `_`?
fn is_var_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

const SHAPE: &str = "expected `cd <dir> && env [NAME=value]... <program> [<arg>]...`";

/// The next token, which must be a word; `want` names what belongs there.
fn next_word(
    toks: &mut impl Iterator<Item = (Tok, usize)>,
    want: &str,
) -> Result<(String, bool), String> {
    match toks.next() {
        Some((Tok::Word { text, bare }, _)) => Ok((text, bare)),
        Some((Tok::AndAnd, at)) => Err(format!("`&&` at byte {at}, where {want} belongs")),
        Some((Tok::Semi, at)) => Err(unquoted(';', at)),
        None => Err(format!("the command ends where {want} belongs; {SHAPE}")),
    }
}

/// Parse a teammate command string into its working directory, environment
/// assignments and argv, or say why it is not one. Pure.
///
/// Accepted: an optional [`CGROUP_PREFIX`] with its directory word and `;`
/// (dropped), then `cd <word> && env <NAME=word>* <word>+`. Words follow
/// POSIX quoting: single quotes are literal; double quotes keep everything
/// but `\"`, `\\`, `` \` `` and `\$`, and an unescaped `$` or backquote in
/// them is refused; a backslash outside quotes takes the next character
/// literally; adjacent pieces join into one word.
///
/// Refused, with the reason: any control character (a newline included);
/// outside quotes, every character but `A-Z a-z 0-9 _ . / : = @ + , -`,
/// which covers `;`, `|`, a lone `&`, `$`, a backquote, `(`, `)`, `<`, `>`,
/// `*`, `?`, `[`, `{`, `~`, `#` and `!`; a `&&` or `;` anywhere but the one
/// place the grammar has it; an option to `cd` or to `env`; an assignment
/// whose name is not a variable name; and a command with no program.
///
/// After `env`, a word with a `=` in it is an assignment until the first word
/// without one, quoted or not, as `env` itself reads its arguments.
pub fn parse_teammate_command(command: &str) -> Result<TeammateCommand, String> {
    if command.len() > MAX_COMMAND_BYTES {
        return Err(format!(
            "the command is {} bytes, over the {MAX_COMMAND_BYTES} this host reads",
            command.len()
        ));
    }
    if let Some((at, c)) = command.char_indices().find(|(_, c)| c.is_control()) {
        return Err(format!("a control character ({}) at byte {at}", shown(c)));
    }
    let (body, prefixed) = match command.strip_prefix(CGROUP_PREFIX) {
        Some(rest) => (rest, true),
        None => (command, false),
    };
    let mut toks = lex(body)?.into_iter();
    if prefixed {
        // The directory the prefix names, then the `;` that ends it.
        match (toks.next(), toks.next()) {
            (Some((Tok::Word { .. }, _)), Some((Tok::Semi, _))) => {}
            _ => {
                return Err(
                    "the cgroup prefix is not followed by one directory word and `;`".to_string(),
                )
            }
        }
    }
    let (cd, bare) = next_word(&mut toks, "`cd`")?;
    if cd != "cd" || !bare {
        return Err(format!("the command does not start with `cd`; {SHAPE}"));
    }
    let (cwd, _) = next_word(&mut toks, "the directory")?;
    if cwd.is_empty() || cwd.starts_with('-') {
        return Err("`cd` is given an empty directory or an option, not a directory".to_string());
    }
    match toks.next() {
        Some((Tok::AndAnd, _)) => {}
        Some((Tok::Semi, at)) => return Err(unquoted(';', at)),
        _ => return Err(format!("no `&&` after the directory; {SHAPE}")),
    }
    let (env_word, bare) = next_word(&mut toks, "`env`")?;
    if env_word != "env" || !bare {
        return Err(format!("no `env` after `&&`; {SHAPE}"));
    }
    let mut env: Vec<(String, String)> = Vec::new();
    let mut argv: Vec<String> = Vec::new();
    for (tok, at) in toks {
        let text = match tok {
            Tok::Word { text, .. } => text,
            Tok::AndAnd => return Err(format!("a second `&&` at byte {at}: another command")),
            Tok::Semi => return Err(unquoted(';', at)),
        };
        if !argv.is_empty() {
            argv.push(text);
            continue;
        }
        match text.split_once('=') {
            Some((name, value)) => {
                if !is_var_name(name) {
                    return Err(format!(
                        "an assignment at byte {at} whose name is not a variable name"
                    ));
                }
                env.push((name.to_string(), value.to_string()));
            }
            None if text.starts_with('-') => {
                return Err(format!("an option to `env` at byte {at}"));
            }
            None if text.is_empty() => {
                return Err(format!(
                    "an empty word at byte {at}, where the program belongs"
                ));
            }
            None => argv.push(text),
        }
    }
    if argv.is_empty() {
        return Err(format!("no program after `env`; {SHAPE}"));
    }
    Ok(TeammateCommand { cwd, env, argv })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- fixtures -----------------------------------------------------------
    //
    // The strings Claude Code 2.1.325 builds, written out from its own code
    // (the installed binary's embedded JavaScript, read 2026-11-15; no live
    // lead was run to capture them):
    //
    //   G = `cd ${Q([cwd])} && env ${ENV} ${Q(program)} ${TEAMMATE_FLAGS}${INHERITED}`
    //
    // where Q quotes as the module docs say, ENV starts
    // `CLAUDECODE=1 CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`, forwards the
    // lead's provider, proxy and config variables and ends
    // `CLAUDE_CODE_EVEN_MORE_DANGEROUSLY_SKIP_PERMISSIONS=`, and a flag is
    // `--flag value`, or `--flag=value` when the value starts with `-` or
    // reads as a URL.

    /// The least a 2.1.325 lead sends: no task list, no verify key, no
    /// inherited flags (its permission mode is unset only in tests of its
    /// own; kept as the minimal shape).
    const FIXTURE_MIN: &str = "cd /home/dev/demo && env CLAUDECODE=1 CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1 ANTHROPIC_PROFILE='' ANTHROPIC_CONFIG_DIR='' CLAUDE_CODE_ATTRIBUTION_PROJECT='' CLAUDE_CODE_USER_DELEGATED_SESSION_DIRECT_API='' CLAUDE_CODE_USER_DELEGATED_SESSION='' CLAUDE_CODE_USER_DELEGATED_SESSION_ANY_CHECK_MODEL='' CLAUDE_CODE_USER_DELEGATED_SESSION_ALLOWED_CHECK_MODELS='' CLAUDE_CODE_EVEN_MORE_DANGEROUSLY_SKIP_PERMISSIONS= /home/dev/.local/bin/claude --agent-id researcher@demo --agent-name researcher --team-name demo --agent-color blue --parent-session-id 0b9f6c1e-2a67-4c58-9d0e-6f4f8a1d2c33";

    /// A full one: a task list, forwarded variables (one with a space, one
    /// with a quote, one emptied), every teammate flag, and the inherited
    /// flags a plan-required teammate gets from a lead with deny rules, a
    /// tool list, a settings file, a plugin directory and an MCP config.
    const FIXTURE_FULL: &str = "cd '/home/dev/my project' && env CLAUDECODE=1 CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1 CLAUDE_CODE_TASK_LIST_ID=demo CLAUDE_CODE_USE_BEDROCK=1 AWS_REGION=us-east-1 AWS_PROFILE='team profile' ANTHROPIC_BASE_URL=https://proxy.example:8443/v1 CLAUDE_CONFIG_DIR=/home/dev/.claude-work HTTPS_PROXY=http://127.0.0.1:3128 NO_PROXY='localhost,*.internal' DISABLE_TELEMETRY='' ANTHROPIC_PROFILE='it'\"'\"'s' ANTHROPIC_CONFIG_DIR='' CLAUDE_CODE_ATTRIBUTION_PROJECT='' CLAUDE_CODE_USER_DELEGATED_SESSION_DIRECT_API='' CLAUDE_CODE_USER_DELEGATED_SESSION='' CLAUDE_CODE_USER_DELEGATED_SESSION_ANY_CHECK_MODEL='' CLAUDE_CODE_USER_DELEGATED_SESSION_ALLOWED_CHECK_MODELS='' CLAUDE_CODE_EVEN_MORE_DANGEROUSLY_SKIP_PERMISSIONS= /home/dev/.local/bin/claude --agent-id reviewer@demo --agent-name reviewer --team-name demo --agent-color colour208 --parent-session-id 0b9f6c1e-2a67-4c58-9d0e-6f4f8a1d2c33 --lead-verify-key 1763200000000.MCowBQYDK2VwAyEAq3x1Zb0m2m1Yw6i8cQ9fYt5n0u7JH2k4L-_dV3sT1aE --plan-mode-required --agent-type code-reviewer --permission-mode plan --proactivity ask --disallowed-tools 'Bash(rm -rf /*)' --disallowed-tools 'Read(./.env)' --tools Read --tools Grep --effort high --settings /home/dev/.local/state/atrium/hooks/settings.json --setting-sources=user,project --plugin-dir /home/dev/.local/share/atrium/mod --mcp-config '{\"mcpServers\":{}}' --strict-mcp-config --no-chrome --model claude-opus-4-5";

    /// Linux with the tool-cgroup feature: the prefix, then the command.
    const FIXTURE_CGROUP: &str = "sh -c '{ echo $PPID > \"$1\"/cgroup.procs; } 2>/dev/null' sh /sys/fs/cgroup/user.slice/claude-agent; cd /home/dev/demo && env CLAUDECODE=1 CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1 CLAUDE_CODE_EVEN_MORE_DANGEROUSLY_SKIP_PERMISSIONS= /home/dev/.local/bin/claude --agent-id researcher@demo --agent-name researcher --team-name demo --agent-color blue --parent-session-id 0b9f6c1e-2a67-4c58-9d0e-6f4f8a1d2c33 --dangerously-skip-permissions";

    /// Windows: a backslash is outside the bare set, so both paths are in
    /// single quotes.
    const FIXTURE_WINDOWS: &str = "cd 'D:\\projects\\demo' && env CLAUDECODE=1 CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1 CLAUDE_CODE_TASK_LIST_ID=demo ANTHROPIC_PROFILE='' ANTHROPIC_CONFIG_DIR='' CLAUDE_CODE_EVEN_MORE_DANGEROUSLY_SKIP_PERMISSIONS= 'C:\\Users\\dev\\AppData\\Roaming\\npm\\node_modules\\@anthropic-ai\\claude-code\\bin\\claude.exe' --agent-id researcher@demo --agent-name researcher --team-name demo --agent-color blue --parent-session-id 0b9f6c1e-2a67-4c58-9d0e-6f4f8a1d2c33 --lead-verify-key 1763200000000.MCowBQYDK2VwAyEAq3x1Zb0m2m1Yw6i8cQ9fYt5n0u7JH2k4L-_dV3sT1aE --permission-mode acceptEdits --model claude-opus-4-5";

    fn get<'a>(c: &'a TeammateCommand, name: &str) -> Option<&'a str> {
        c.env
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn the_minimal_2_1_325_string_parses() {
        let c = parse_teammate_command(FIXTURE_MIN).unwrap();
        assert_eq!(c.cwd, "/home/dev/demo");
        assert_eq!(c.env.len(), 10);
        assert_eq!(get(&c, "CLAUDECODE"), Some("1"));
        assert_eq!(get(&c, "ANTHROPIC_PROFILE"), Some(""), "NAME='' is empty");
        assert_eq!(
            get(&c, "CLAUDE_CODE_EVEN_MORE_DANGEROUSLY_SKIP_PERMISSIONS"),
            Some(""),
            "NAME= is empty"
        );
        assert_eq!(c.argv[0], "/home/dev/.local/bin/claude");
        assert_eq!(
            &c.argv[1..],
            [
                "--agent-id",
                "researcher@demo",
                "--agent-name",
                "researcher",
                "--team-name",
                "demo",
                "--agent-color",
                "blue",
                "--parent-session-id",
                "0b9f6c1e-2a67-4c58-9d0e-6f4f8a1d2c33"
            ]
        );
    }

    #[test]
    fn the_full_2_1_325_string_parses_with_its_quoted_words() {
        let c = parse_teammate_command(FIXTURE_FULL).unwrap();
        assert_eq!(c.cwd, "/home/dev/my project");
        assert_eq!(get(&c, "CLAUDE_CODE_TASK_LIST_ID"), Some("demo"));
        assert_eq!(get(&c, "AWS_PROFILE"), Some("team profile"));
        assert_eq!(get(&c, "NO_PROXY"), Some("localhost,*.internal"));
        assert_eq!(get(&c, "ANTHROPIC_PROFILE"), Some("it's"));
        assert_eq!(get(&c, "DISABLE_TELEMETRY"), Some(""));
        let has = |a: &str, b: &str| c.argv.windows(2).any(|w| w[0] == a && w[1] == b);
        assert!(has("--disallowed-tools", "Bash(rm -rf /*)"));
        assert!(has("--mcp-config", "{\"mcpServers\":{}}"));
        assert!(has("--permission-mode", "plan"));
        assert!(c.argv.iter().any(|a| a == "--setting-sources=user,project"));
        assert!(c.argv.iter().any(|a| a == "--plan-mode-required"));
        assert_eq!(c.argv.last().map(String::as_str), Some("claude-opus-4-5"));
    }

    #[test]
    fn the_cgroup_prefix_is_recognised_and_dropped() {
        let c = parse_teammate_command(FIXTURE_CGROUP).unwrap();
        assert_eq!(c.cwd, "/home/dev/demo");
        assert_eq!(c.argv[0], "/home/dev/.local/bin/claude");
        assert_eq!(
            c.argv.last().map(String::as_str),
            Some("--dangerously-skip-permissions")
        );
        // Any other first command is not the prefix: it is refused, not run.
        for other in [
            "sh -c 'echo hi' sh /x; cd /d && env claude",
            "sh -c '{ echo $PPID > \"$1\"/cgroup.procs; } 2>/dev/null' sh /a /b; cd /d && env claude",
            "sh -c '{ echo $PPID > \"$1\"/cgroup.procs; } 2>/dev/null' sh /a && cd /d && env claude",
            "sh -c '{ echo $PPID > \"$1\"/cgroup.procs; } 2>/dev/null' sh; cd /d && env claude",
            "sh -c '{ echo $PPID > \"$1\"/cgroup.procs; } 2>/dev/null' sh /a; sh -c '{ echo $PPID > \"$1\"/cgroup.procs; } 2>/dev/null' sh /a; cd /d && env claude",
        ] {
            assert!(parse_teammate_command(other).is_err(), "{other}");
        }
    }

    #[test]
    fn the_windows_string_keeps_its_backslashes() {
        let c = parse_teammate_command(FIXTURE_WINDOWS).unwrap();
        assert_eq!(c.cwd, "D:\\projects\\demo");
        assert_eq!(
            c.argv[0],
            "C:\\Users\\dev\\AppData\\Roaming\\npm\\node_modules\\@anthropic-ai\\claude-code\\bin\\claude.exe"
        );
    }

    /// Claude Code 2.1.325's quoting function, word for word.
    fn q(word: &str) -> String {
        if word.is_empty() {
            return "''".to_string();
        }
        if word.chars().all(is_bare) {
            return word.to_string();
        }
        format!("'{}'", word.replace('\'', "'\"'\"'"))
    }

    /// Values a shell would act on if they were not quoted.
    const NASTY: &[&str] = &[
        "plain",
        "",
        "two words",
        "  lead and trail  ",
        "it's",
        "''",
        "'",
        "\"double\"",
        "back\\slash",
        "\\",
        "a && b",
        "a; b",
        "a | b > c < d",
        "$HOME and ${X} and $(id)",
        "`id`",
        "* ? [a-z] {a,b}",
        "~root",
        "# not a comment",
        "!history",
        "NAME=value",
        "-dash",
        "--flag=value",
        "caf\u{e9} \u{65e5}\u{672c}",
        "%PATH% ^ &",
        "'\"'\"'",
        "a'b\"c\\d e",
    ];

    #[test]
    fn every_quoted_value_round_trips_in_every_position() {
        for v in NASTY {
            // As the directory (not empty and not an option: `cd` refuses those).
            if !v.is_empty() && !v.starts_with('-') {
                let c = parse_teammate_command(&format!("cd {} && env claude", q(v))).unwrap();
                assert_eq!(&c.cwd, v);
            }
            // As an environment value.
            let line = format!("cd /d && env A={} B= C='' claude", q(v));
            let c = parse_teammate_command(&line).unwrap_or_else(|e| panic!("{line}: {e}"));
            assert_eq!(c.env[0], ("A".to_string(), v.to_string()), "{line}");
            assert_eq!(c.env[1], ("B".to_string(), String::new()));
            assert_eq!(c.env[2], ("C".to_string(), String::new()));
            assert_eq!(c.argv, ["claude"]);
            // As an argument, alone and after a flag.
            let line = format!("cd /d && env claude --flag {} {}", q(v), q(v));
            let c = parse_teammate_command(&line).unwrap_or_else(|e| panic!("{line}: {e}"));
            assert_eq!(c.argv, ["claude", "--flag", v, v], "{line}");
        }
    }

    #[test]
    fn the_other_posix_quotings_are_read_too() {
        let c = parse_teammate_command(
            r#"cd "/my dir/it's" && env A="say \"hi\" \\ \$5 \`x\` \n" B=a\ b\'\"\\ C='x'"y"z\ w claude "--a b" plain\&\&"#,
        )
        .unwrap();
        assert_eq!(c.cwd, "/my dir/it's");
        assert_eq!(c.env[0].1, "say \"hi\" \\ $5 `x` \\n");
        assert_eq!(c.env[1].1, "a b'\"\\");
        assert_eq!(c.env[2].1, "xyz w", "adjacent pieces are one word");
        assert_eq!(c.argv, ["claude", "--a b", "plain&&"]);
    }

    /// Every character a shell acts on, unquoted, in every position a word
    /// can be: refused, with a reason, never read as part of a word.
    #[test]
    fn every_unquoted_metacharacter_is_refused_everywhere() {
        let metas = [
            ';', '|', '&', '$', '`', '(', ')', '<', '>', '*', '?', '[', ']', '{', '}', '~', '#',
            '!', '%', '^',
        ];
        for m in metas {
            for line in [
                format!("cd /d{m} && env A=1 claude --x y"),
                format!("cd {m}/d && env A=1 claude --x y"),
                format!("cd /d && env A={m}1 claude --x y"),
                format!("cd /d && env A=1{m} claude --x y"),
                format!("cd /d && env A=1 {m}claude --x y"),
                format!("cd /d && env A=1 claude{m} --x y"),
                format!("cd /d && env A=1 claude --x {m}y"),
                format!("cd /d && env A=1 claude --x y{m}"),
                format!("cd /d && env A=1 claude --x y {m}"),
                format!("cd /d && env A=1 claude --x y {m} id"),
                format!("{m} cd /d && env A=1 claude --x y"),
                format!("cd /d {m} env A=1 claude --x y"),
                format!("cd /d &&{m} env A=1 claude --x y"),
            ] {
                let got = parse_teammate_command(&line);
                assert!(got.is_err(), "{line:?} parsed as {got:?}");
                assert!(!got.unwrap_err().is_empty());
            }
        }
        // Whitespace that is not a space, and every other control character.
        for c in ['\n', '\r', '\t', '\0', '\x1b', '\x7f', '\u{85}'] {
            for line in [
                format!("cd /d && env claude{c}id"),
                format!("cd /d && env claude 'a{c}b'"),
                format!("cd /d && env claude{c}"),
            ] {
                let e = parse_teammate_command(&line).unwrap_err();
                assert!(e.contains("control character"), "{line:?}: {e}");
            }
        }
    }

    #[test]
    fn expansions_inside_double_quotes_are_refused() {
        for line in [
            r#"cd /d && env A="$HOME" claude"#,
            r#"cd /d && env A="`id`" claude"#,
            r#"cd /d && env claude "$(id)""#,
        ] {
            let e = parse_teammate_command(line).unwrap_err();
            assert!(e.contains("expansion"), "{line}: {e}");
        }
    }

    #[test]
    fn anything_outside_the_grammar_is_refused_with_a_reason() {
        for (line, why) in [
            ("", "ends where"),
            ("claude --agent-id x", "does not start with `cd`"),
            ("'cd' /d && env claude", "does not start with `cd`"),
            ("cd && env claude", "`&&` at byte"),
            ("cd /d", "no `&&`"),
            ("cd /d /e && env claude", "no `&&`"),
            ("cd -P && env claude", "option"),
            ("cd '' && env claude", "empty directory"),
            ("cd /d && claude", "no `env`"),
            ("cd /d && \"env\" claude", "no `env`"),
            ("cd /d && env", "no program"),
            ("cd /d && env A=1", "no program"),
            ("cd /d && env -i claude", "option to `env`"),
            ("cd /d && env -u A claude", "option to `env`"),
            ("cd /d && env '' claude", "empty word"),
            ("cd /d && env 1A=1 claude", "not a variable name"),
            ("cd /d && env =1 claude", "not a variable name"),
            ("cd /d && env 'A B=1' claude", "not a variable name"),
            ("cd /d && env claude && id", "second `&&`"),
            ("cd /d && env claude; id", "unquoted `;`"),
            ("cd /d; env claude", "unquoted `;`"),
            ("cd /d && env claude & id", "unquoted `&`"),
            ("cd /d && env claude 'open", "never closed"),
            ("cd /d && env claude \"open", "never closed"),
            ("cd /d && env claude \"open\\", "never closed"),
            ("cd /d && env claude back\\", "escapes nothing"),
        ] {
            let e = parse_teammate_command(line).unwrap_err();
            assert!(e.contains(why), "{line:?}: {e:?} lacks {why:?}");
        }
        let long = format!("cd /d && env claude {}", "a".repeat(MAX_COMMAND_BYTES));
        assert!(parse_teammate_command(&long).unwrap_err().contains("bytes"));
    }

    #[test]
    fn an_error_never_echoes_a_character_that_does_not_show() {
        let e = parse_teammate_command("cd /d && env claude \u{202e}x").unwrap_err();
        assert!(e.contains("U+202E"), "{e}");
        assert!(!e.contains('\u{202e}'));
    }

    #[test]
    fn a_word_with_an_equals_sign_is_an_assignment_only_before_the_program() {
        let c = parse_teammate_command("cd /d && env A=1 'B=two words' claude C=3 --m=x").unwrap();
        assert_eq!(
            c.env,
            [
                ("A".to_string(), "1".to_string()),
                ("B".to_string(), "two words".to_string())
            ]
        );
        assert_eq!(c.argv, ["claude", "C=3", "--m=x"]);
    }

    /// A small deterministic generator: no clock, no dependency.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0 >> 33
        }
    }

    /// Words drawn from an alphabet of everything a shell acts on, quoted as
    /// Claude Code quotes: every one comes back as it went in.
    #[test]
    fn generated_words_round_trip_through_the_quoting() {
        const ALPHABET: &[char] = &[
            'a', 'Z', '0', ' ', ' ', '\'', '"', '\\', '&', ';', '|', '$', '`', '(', ')', '<', '>',
            '*', '?', '[', ']', '{', '}', '~', '#', '!', '=', '-', '/', '.', ':', ',', '@', '+',
            '%', '^', '\u{e9}',
        ];
        let mut rng = Lcg(0x2_1325);
        for _ in 0..4000 {
            let mut words: Vec<String> = Vec::new();
            for _ in 0..(1 + rng.next() % 5) {
                let len = rng.next() % 9;
                words.push(
                    (0..len)
                        .map(|_| ALPHABET[(rng.next() % ALPHABET.len() as u64) as usize])
                        .collect(),
                );
            }
            let value = &words[0];
            let args: Vec<String> = words.iter().map(|w| q(w)).collect();
            let line = format!("cd /d && env V={} /bin/claude {}", q(value), args.join(" "));
            let c = parse_teammate_command(&line).unwrap_or_else(|e| panic!("{line:?}: {e}"));
            assert_eq!(c.cwd, "/d");
            assert_eq!(c.env, [("V".to_string(), value.clone())], "{line:?}");
            assert_eq!(c.argv[0], "/bin/claude");
            assert_eq!(&c.argv[1..], &words[..], "{line:?}");
        }
    }

    /// The same alphabet with no quoting at all: whatever parses holds only
    /// the bare set, so no metacharacter ever reached a word unquoted.
    #[test]
    fn generated_unquoted_lines_never_yield_a_metacharacter() {
        const ALPHABET: &[char] = &[
            'a', 'b', '1', ' ', ' ', ' ', '&', ';', '|', '$', '`', '(', ')', '<', '>', '*', '?',
            '~', '#', '=', '-', '/', '.',
        ];
        let mut rng = Lcg(0x325);
        let mut parsed = 0;
        for _ in 0..20000 {
            let len = rng.next() % 24;
            let tail: String = (0..len)
                .map(|_| ALPHABET[(rng.next() % ALPHABET.len() as u64) as usize])
                .collect();
            let line = format!("cd /d && env {tail}");
            if let Ok(c) = parse_teammate_command(&line) {
                parsed += 1;
                let all = c.env.iter().flat_map(|(n, v)| [n, v]).chain(c.argv.iter());
                for word in all {
                    assert!(word.chars().all(is_bare), "{line:?} gave {word:?}");
                }
            }
        }
        assert!(parsed > 0, "the generator reaches the accepting paths too");
    }
}
