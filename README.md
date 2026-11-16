# nativelite-tmuxproto

**The tmux command-line protocol a pane host needs to serve Claude Code's agent
teams.** Argv parsing, `#{format}` expansion, POSIX shell words, and a verb
runner over *your* pane backend. **Zero runtime dependencies**: the Rust
standard library only.

## Why

Claude Code's split-pane teams do not draw panes. The lead runs a program named
`tmux` and asks it to split the window and run the teammate:

```
tmux split-window -d -t @0 -h -l 70% -P -F '#{pane_id}' -- claude --agent-name researcher …
```

then reads the new pane's id back, and later `kill-pane`s it. That is Claude
Code 2.1.289. Since 2.1.325 the split runs a placeholder and the teammate is
started in it afterwards, as one shell command line:

```
tmux -S <socket> split-window -d -t %0 -h -l 70% -P -F '#{pane_id}' -- cat
tmux -S <socket> respawn-pane -k -t %1 -- 'cd /work && env CLAUDECODE=1 … /path/to/claude --agent-id …'
```

Both are served. Its backend knows
two pane managers, tmux and iTerm2, and picks tmux whenever `TMUX` is set. A
pane host that wants those teammates as its own panes answers to the name: a
`tmux` executable first on the pane's `PATH`, `TMUX` and `TMUX_PANE` set, and
the verbs served. This crate is the half of that every host shares, so
[atrium](https://github.com/nativelite/atrium) and fwterm parse tmux once.

## Install

```toml
[dependencies]
tmuxproto = { package = "nativelite-tmuxproto", version = "0.3.1" }
```

## Use

Implement [`Broker`](src/lib.rs) over your panes, then serve:

```rust
use tmuxproto::{parse, run, needs_broker, Broker, Host, Node, NoBroker, Verb};

const HOST: Host = Host { session_name: "myterm", version_suffix: "myterm" };

struct Panes; // your backend
impl Broker for Panes {
    fn caller(&self) -> Option<usize> { std::env::var("MYTERM_PANE").ok()?.parse().ok() }
    fn list(&mut self) -> Result<Vec<Node>, String> { Ok(vec![(1, None, None, "claude".into())]) }
    fn spawn(
        &mut self,
        here: bool,            // split the caller's window, or open a new one
        focus: bool,           // false for tmux's -d
        beside: Option<usize>, // the pane `-t %N` targeted; None = beside the caller
        size: Option<&str>,    // the raw `-l` value ("70%", "20"), if your layout can use it
        role: Option<&str>,
        argv: &[String],
    ) -> Result<usize, String> {
        let _ = (here, focus, beside, size, role, argv);
        Ok(2) // open the pane, return its id
    }
    fn kill(&mut self, _id: usize) -> Result<(), String> { Ok(()) }
    fn send(&mut self, _id: usize, _text: &str) -> Result<(), String> { Ok(()) }
    // Claude Code 2.1.325 (both have defaults; override them to serve it):
    fn hold(&mut self, focus: bool, beside: Option<usize>, size: Option<&str>) -> Result<usize, String> {
        let _ = (focus, beside, size);
        Ok(2) // open a pane that only holds its place, return its id
    }
    fn respawn(&mut self, id: usize, command: &str) -> Result<(), String> {
        // Never `sh -c` this: take it apart and decide what to honour.
        let teammate = tmuxproto::parse_teammate_command(command)?;
        let _ = (id, teammate.cwd, teammate.env, teammate.argv);
        Ok(()) // start the teammate in pane `id`, keeping the id
    }
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let verb = parse(&args);
    let out = if needs_broker(&verb) {
        run(verb, &mut Panes, HOST)
    } else {
        run(verb, &mut NoBroker, HOST)
    };
    print!("{}", out.stdout);
    eprint!("{}", out.stderr);
    std::process::ExitCode::from(out.code)
}
```

What a host must match so a Claude Code lead can move between hosts:

- `TMUX=<endpoint>,<pid>,0` (Claude Code reads the first field as the socket
  and may pass it back as `-S <path>`, which `parse` skips), `TMUX_PANE=%<id>`.
- Pane ids `%N`; one window `@0`; `#{session_name}` is your `Host::session_name`
  and must match `[A-Za-z0-9_.-]{1,64}`, because Claude Code validates
  `display-message -p '#{session_name}:#{window_id}.#{pane_id}'`.
- `tmux -V` prints `tmux 3.5a-<suffix>`; Claude Code wants a major of 2 or more.
- On Windows the executable must be `tmux.exe` on `PATH`: Claude Code is a
  native binary there and resolves executables only, never `.cmd` shims.
- Set `CLAUDE_CODE_TMUX_TRUECOLOR=1` beside `TMUX`, or Claude Code drops to
  256 colours assuming a real tmux in between.

## What is served

Read from two installed Claude Code releases, 2.1.289 and 2.1.325 (the code
embedded in each binary; the crate's tests carry the 2.1.325 strings as
fixtures):

| verb | does |
| --- | --- |
| `-V` | version line |
| `display-message -p [-t] <fmt>` | expands `#{pane_id}`, `#{window_id}`, `#{pane_title}`, `#{pane_index}`, `#{window_panes}`, `#{window_name}`, `#{session_name}`, `#{session_id}`, `#{client_control_mode}`; unknown variables expand to nothing |
| `list-panes -F`, `list-windows -F` | the caller and its direct children; one window |
| `split-window [-d] [-t <target>] [-h\|-v] [-l <size>] … -P -F <fmt> -- <cmd>` | `Broker::spawn(here = true, focus = !detached, beside, size)`, role from `--agent-name`; the command is split with `sh_words` so no shell is needed. Claude Code passes `-d`, so the lead's human keeps the keyboard. `-t %N` is `beside = Some(N)`: the host places the new pane next to that pane (Claude Code's second and later teammates target an existing teammate); `-t @0` or no `-t` is `beside = None`, next to the caller. `-l` is parsed and handed to the host raw (`"70%"`, `"20"`): the new pane's share of the target pane, or a count of cells. atrium honours it as `ctl spawn --here --size`, keeping the ratio on its split tree through later spawns and resizes; a host whose layout is equal-size may ignore it, and a value the host refuses is dropped so the split still happens. `-h`/`-v` are accepted; the host's own layout decides the orientation |
| `split-window … -- cat` | Claude Code 2.1.325's placeholder: a split whose whole command is the word `cat` (`PLACEHOLDER`) is `Broker::hold(focus, beside, size)`, placed like any split. The default `hold` asks `spawn` for `["cat"]`, as a host that predates it did |
| `new-window [-d] -n <name> … -- <cmd>` | `Broker::spawn(here = false, focus = !detached, beside = None, size = None)` |
| `respawn-pane [-k] -t %N -- <command string>` | `Broker::respawn(id, command)`, with the command line untouched: exactly one argument after `--`, and a pane target. The default `respawn` refuses |
| `kill-pane -t %N` | `Broker::kill` |
| `send-keys -t %N <keys…> [Enter]` | `Broker::send` |
| `set-option`, `select-pane`, `select-layout`, `resize-pane`, `show-environment`, `has-session`, `new-session`, `switch-client`, `attach-session` | accepted, exit 0, no output |
| `capture-pane` | refused, exit 1: a host should never hand one pane the text of another |
| anything else | exit 1 with a message |

`-S <socket>` and `-L <name>` before the verb take a value and are skipped
(2.1.325 leads every call with `-S`, the first field of `$TMUX`).
`set-option -p -t %N remain-on-exit failed`, which 2.1.325 sends before the
respawn, is one of the accepted `set-option`s.

## The teammate command

`respawn-pane`'s command is a POSIX shell line, written for tmux to run with
`sh -c`. A pane host must not do that: the line comes from a process inside a
pane, and Windows has no `sh`, `cd` or `env`. `parse_teammate_command` reads it
instead:

```rust
let t = tmuxproto::parse_teammate_command(
    "cd '/work/my project' && env CLAUDECODE=1 AWS_PROFILE='team a' /usr/bin/claude --agent-name dev",
)?;
assert_eq!(t.cwd, "/work/my project");
assert_eq!(t.env[1], ("AWS_PROFILE".into(), "team a".into()));
assert_eq!(t.argv, ["/usr/bin/claude", "--agent-name", "dev"]);
```

It accepts one grammar and nothing else:

```
[ sh -c '{ echo $PPID > "$1"/cgroup.procs; } 2>/dev/null' sh <dir>; ]   (Linux tool-cgroup prefix: recognised, dropped)
cd <word> && env <NAME=word>* <word>+
```

- Words use POSIX quoting: single quotes are literal, double quotes keep all
  but `\"`, `\\`, `` \` `` and `\$`, a backslash outside quotes takes the next
  character literally, and adjacent pieces are one word. (Claude Code itself
  writes a word bare when it is all `A-Z a-z 0-9 _ . / : = @ + , -`, and
  otherwise in single quotes with each `'` as `'"'"'`.)
- Outside quotes only that bare set is a word character. Everything else is an
  error that says why: `;`, `|`, a lone `&`, `$`, a backquote, `(`, `)`, `<`,
  `>`, `*`, `?`, `[`, `{`, `~`, `#`, `!`, and any second `&&`. An unescaped `$`
  or backquote inside double quotes is an error too.
- Any control character, a newline and a tab included, is an error wherever it
  is (Claude Code refuses to send one).
- An option to `cd` or to `env`, an assignment whose name is not a variable
  name, or no program after `env`: errors.
- After `env`, a word with `=` in it is an assignment until the first word
  without one, as `env` reads its arguments.

What to do with `cwd`, `env` and `argv` is the host's decision. atrium, for
one, runs its own recorded claude in place of `argv[0]`, takes the flags it
knows from a closed table, and applies three of the environment assignments.

## Development

```bash
python dev.py check   # zero-dependency guard + cargo fmt --check + cargo test
python dev.py test
```

MIT. Part of [nativelite](https://github.com/nativelite): lightweight packages
built on native tools.
