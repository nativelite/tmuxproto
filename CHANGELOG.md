# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0] - 2026-11-16

### Added
- **Claude Code 2.1.325's teammate launch is served.** That release no longer
  passes the teammate's command on the split. It splits with the placeholder
  `cat`, styles the pane, then sends
  `respawn-pane -k -t %N -- '<one shell command line>'`. Read from the
  installed 2.1.325 binary, and since run once against atrium by a live
  2.1.325 lead on Windows (one teammate, started and bound).
  - `Verb::RespawnPane { target, kill, command }`, parsed from
    `respawn-pane` / `respawnp` (a pane target and exactly one argument after
    `--`), and `Broker::respawn(id, command)`, which gets the command line
    untouched. The method has a default that refuses, so an existing `Broker`
    compiles unchanged and serves nothing new until it opts in.
  - `PLACEHOLDER` (`"cat"`) and `Broker::hold(focus, beside, size)`: a
    `split-window` whose whole command is that word asks the host for a pane
    that holds its place. The default calls `spawn` with `["cat"]`, which is
    what the split did before.
  - `parse_teammate_command` and `TeammateCommand { cwd, env, argv }`: the
    command line read without a shell. One grammar,
    `cd <word> && env <NAME=word>* <word>+`, with the Linux tool-cgroup prefix
    (`CGROUP_PREFIX`) recognised and dropped, POSIX quoting for words, and an
    error with a reason for anything a shell would act on outside a quote
    (`;`, `|`, `&`, `$`, a backquote, `(`, `)`, `<`, `>`, `*`, `?`, `[`, `{`,
    `~`, `#`, `!`, a second `&&`), for any control character, and for input
    over `MAX_COMMAND_BYTES` (24 KiB, so the line fits one host request).
  - `respawn-pane` given tmux's `-c <dir>` or `-e VAR=val` is refused with
    a reason rather than served with the option ignored.
- Tests pin the 2.1.325 strings as fixtures, `-S <socket>` / `-L <name>`
  before every verb, and `set-option -p -t %N remain-on-exit failed` among the
  accepted no-ops.

### Changed
- **Breaking: `Verb` has a new variant and is now `#[non_exhaustive]`.** A
  `match` on it needs a wildcard arm, once; the next verb this crate learns
  will then not be a breaking change. A `Broker` implementation compiles
  unchanged (both new methods have defaults).

## [0.3.0] - 2026-11-08

### Changed
- **`split-window` carries its `-t` target and `-l` size; `Broker::spawn`
  takes `beside: Option<usize>` and `size: Option<&str>`.** Claude Code's
  first teammate splits the window (`-t @0 -h -l 70%`) and each later one
  splits an existing teammate's pane (`-t %N -v|-h`); the shim parsed `-t`
  and dropped it for `split-window`, so a host could only ever place the new
  pane beside the caller (atrium issue #17). `Verb::SplitWindow` now has
  `target: Option<Target>` and `size: Option<String>` (the raw `-l` value);
  `run` passes `beside = Some(N)` for `-t %N` and `None` for a window target
  or no target, and `size` as written. A host honours `size` when its layout
  can and ignores it otherwise. Breaking for a `Broker` implementation (two
  added parameters) and for a match on `Verb::SplitWindow`; nothing else
  changes.

## [0.2.0] - 2026-11-08

### Changed
- **`Broker::spawn` takes `focus: bool`, and `Verb::SplitWindow` /
  `Verb::NewWindow` carry `detached`.** tmux's `-d` means the new pane does
  not take focus, and Claude Code passes it on every teammate spawn; a host
  that focused the new pane moved the lead's human into the teammate. Breaking
  for a `Broker` implementation (one added parameter); nothing else changes.

## [0.1.0] - 2026-11-08

### Added
- First cut, moved out of atrium 0.42.0's `src/tmux_shim.rs` so atrium and
  fwterm serve Claude Code's agent teams from one parser: `parse` (the tmux
  argv of the verbs Claude Code 2.1.289 uses, global `-S/-L/-f` skipped,
  per-verb valued flags, `--` and positionals), `Target` (`%N` pane, else the
  window), `Verb`, `expand` (`#{pane_id}`, `#{window_id}`, `#{pane_title}`,
  `#{window_panes}`, `#{session_name}`, `#{client_control_mode}`, …),
  `sh_words` (POSIX-shell word splitting so no shell is needed on the host),
  `agent_name` (`--agent-name` from a teammate's argv), the `Broker` trait a
  host implements (`list`, `spawn`, `kill`, `send`), `run` returning an
  `Outcome` (stdout, stderr, exit code), and `Host` (session name, version
  suffix). `capture-pane` is refused; layout and style verbs are accepted as
  no-ops.
