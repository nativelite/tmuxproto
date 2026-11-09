# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
