# events

Claude plans your events. They happen.

A small scheduler for Claude agents. `events serve` keeps your events and, at each one's moment, runs its function: today, a Pushover message to your phone, an alarm if you want one. Claude adds, reads and deletes events through the `events` command, and the skill in `.claude/skills/events` tells it how.

This page is the first-time read: installing and uninstalling. Daily use is in the skill.

## Install

On Linux or macOS.

1. Take the archive for your machine from https://github.com/AsherJingkongChen/events/releases: there is one for Linux on x86_64 and one for macOS on Apple silicon. It holds the `events` binary: put it on your PATH. `gh attestation verify ARCHIVE --repo AsherJingkongChen/events` checks that the archive was built here. On any other machine, build the binary: `cargo install --locked --git https://github.com/AsherJingkongChen/events`
2. Put the skill where your Claude reads skills: `events skill ~/.claude/skills`. The binary carries its own skill, so the two are always one version; run it again after you upgrade.
3. Keep `events serve` running under your platform's supervisor, started at boot and started again when it fails: a systemd user service with lingering on Linux, a launchd agent on macOS. Your Claude can write it. On its first start serve makes its home, `~/.local/share/events` (700), with an empty `store.jsonl` and a `.env` of empty keys. One serve runs a home; a second exits with an error.
4. Fill the keys in `~/.local/share/events/.env`; `events serve --help` says which they are and where they come from. serve reads them each time it sends, so it needs no restart.

The home is `$EVENTS_HOME`; if that is unset, `$XDG_DATA_HOME/events`; if that is unset too, `~/.local/share/events`.

## Uninstall

1. Stop and remove the service: `systemctl --user disable --now` it and delete its unit, or `launchctl bootout` it and delete its plist.
2. Delete the `events` binary, or `cargo uninstall events` if you built it.
3. `rm -r ~/.claude/skills/events`
4. `rm -r ~/.local/share/events`. It holds your keys (`.env`) and the record of every event (`store.jsonl`): keep a copy of what you want.

## Something wrong

Open an issue at https://github.com/AsherJingkongChen/events/issues.

MIT licensed.
