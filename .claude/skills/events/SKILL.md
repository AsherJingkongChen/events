---
name: events
description: Schedule things that happen on their own at a moment; today, a Pushover message or alarm on the user's phone. Use when the user wants to be reminded, woken or told something at a time ("remind me", "wake me at", "alarm", "ping me when"), or wants to see or cancel what is scheduled.
allowed-tools: Bash(${CLAUDE_SKILL_DIR}/help.sh)
---

# events

`events serve` runs all the time on this machine; if `events get` says serve is not running, the README of the repo tells how it is kept alive. You reach it only through the `events` command. Never read or edit the files in its home (`~/.local/share/events`): `.env` there holds the user's keys.

- **Add**: `events add --at <RFC 3339> --label <your name> < function.json`. The function is JSON on stdin, never on the command line, where anyone on the machine can read it. Today its one kind is `{"pushover":{...}}`, its fields named as https://pushover.net/api names them: `message` is required; `title`, `priority`, `sound`, `url` and the rest are optional.
- **One event is one moment.** To warn early, add a second event at the earlier moment.
- **Waking someone**: `"priority":2` with `"retry"` (seconds between repeats, 30 or more) and `"expire"` (seconds it keeps repeating, 10800 at most) repeats until the user taps it. `"priority":1` sounds through quiet hours.
- **Space events apart.** serve's event loop does one thing at a time: events at the same moment run one after another, and a slow one holds the next for up to its retries × timeout.
- **Check**: `events get --short` lists what waits; `events get --expired --short` what has passed. Read one in full with `events get ID`, and pick fields with jq. An expired event's `result.exit_code` is 0 when it was done; null when it never ran or was killed, and `result.error` says why; anything else, it failed: `result.http` holds Pushover's reply. A missed or failed event is not tried again; tell the user.
- **Change**: there is no update. `events del ID`, then add again.
- **Keep the record.** Expired events are never deleted for you; delete only what the user wants gone.
- **Something wrong with events itself**: tell the user, and with their word open an issue at the address that ends `events --help`.

## Every command

```!
"${CLAUDE_SKILL_DIR}/help.sh"
```
