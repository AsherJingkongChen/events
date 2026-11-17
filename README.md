# events

Claude plans your events. They happen.

A small scheduler for Claude agents. `events serve` keeps your events and, at each one's moment, runs its function: today, a Pushover message to your phone, an alarm if you want one. Claude adds, reads and deletes events through the `events` command, and the skill in `.claude/skills/events` tells it how.

This page is the first-time read: installing and uninstalling. Daily use is in the skill.

## Install

On Linux or macOS, with Rust.

1. `cargo install --locked --git https://github.com/AsherJingkongChen/events`
2. From a clone, copy the skill: `cp -r .claude/skills/events ~/.claude/skills/`
3. Keep `events serve` running under your platform's supervisor: a systemd user service with lingering on Linux, a launchd agent on macOS. Your Claude can write it. On its first start serve makes its home, `~/.local/share/events` (700), with an empty `store.jsonl` and a `.env` of empty keys. One serve runs a home; a second exits with an error.
4. Fill `~/.local/share/events/.env`: `PUSHOVER_API_TOKEN` from an application you make at https://pushover.net/apps/build, `PUSHOVER_USER_KEY` from your Pushover dashboard. It is read each time a message is sent, so serve needs no restart.

`EVENTS_HOME` puts the home elsewhere; so does `XDG_DATA_HOME`, as `$XDG_DATA_HOME/events`.

## Uninstall

1. Stop and remove the service: `systemctl --user disable --now` it and delete its unit, or `launchctl bootout` it and delete its plist.
2. `cargo uninstall events`
3. `rm -r ~/.claude/skills/events`
4. `rm -r ~/.local/share/events`. It holds your keys (`.env`) and the record of every event (`store.jsonl`): keep a copy of what you want.

## Something wrong

Open an issue at https://github.com/AsherJingkongChen/events/issues.

MIT licensed.
