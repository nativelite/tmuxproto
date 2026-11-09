# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
