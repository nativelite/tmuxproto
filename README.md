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

then reads the new pane's id back, and later `kill-pane`s it. Its backend knows
two pane managers, tmux and iTerm2, and picks tmux whenever `TMUX` is set. A
pane host that wants those teammates as its own panes answers to the name: a
`tmux` executable first on the pane's `PATH`, `TMUX` and `TMUX_PANE` set, and
the verbs served. This crate is the half of that every host shares, so
[atrium](https://github.com/nativelite/atrium) and fwterm parse tmux once.

## Install

```toml
[dependencies]
tmuxproto = { package = "nativelite-tmuxproto", version = "0.1" }
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
    fn spawn(&mut self, here: bool, role: Option<&str>, argv: &[String]) -> Result<usize, String> {
        let _ = (here, role, argv);
        Ok(2) // open the pane, return its id
    }
    fn kill(&mut self, _id: usize) -> Result<(), String> { Ok(()) }
    fn send(&mut self, _id: usize, _text: &str) -> Result<(), String> { Ok(()) }
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

Read from the installed Claude Code 2.1.289:

| verb | does |
| --- | --- |
| `-V` | version line |
| `display-message -p [-t] <fmt>` | expands `#{pane_id}`, `#{window_id}`, `#{pane_title}`, `#{pane_index}`, `#{window_panes}`, `#{window_name}`, `#{session_name}`, `#{session_id}`, `#{client_control_mode}`; unknown variables expand to nothing |
| `list-panes -F`, `list-windows -F` | the caller and its direct children; one window |
| `split-window … -P -F <fmt> -- <cmd>` | `Broker::spawn(here = true)`, role from `--agent-name`; the command is split with `sh_words` so no shell is needed |
| `new-window -n <name> … -- <cmd>` | `Broker::spawn(here = false)` |
| `kill-pane -t %N` | `Broker::kill` |
| `send-keys -t %N <keys…> [Enter]` | `Broker::send` |
| `set-option`, `select-pane`, `select-layout`, `resize-pane`, `show-environment`, `has-session`, `new-session`, `switch-client`, `attach-session` | accepted, exit 0, no output |
| `capture-pane` | refused, exit 1: a host should never hand one pane the text of another |
| anything else | exit 1 with a message |

## Development

```bash
python dev.py check   # zero-dependency guard + cargo fmt --check + cargo test
python dev.py test
```

MIT. Part of [nativelite](https://github.com/nativelite): lightweight packages
built on native tools.
