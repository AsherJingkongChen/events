---
name: events
description: Schedule things that happen on their own at a moment; today, a Pushover message or alarm on the user's phone. Use when the user wants to be reminded, woken or told something at a time ("remind me", "wake me at", "alarm", "ping me when"), or wants to see or cancel what is scheduled.
allowed-tools: Bash(events --help), Bash(events add --help), Bash(events get --help), Bash(events del --help), Bash(events stop --help), Bash(events serve --help), Bash(events skill --help)
---

# events

`events serve` runs all the time on this machine and keeps the clock; you only tell it what to do, through the `events` command. A Claude session sleeps, is compacted, is cleared: never be the clock yourself.

What you hand it, and what it hands back, is a pyramid. What to judge at each level:

- **Event**: when (`--at`), who (`--label`, your name), how hard to try (`--retries`, `--timeout`).
  - When you do not know what time it is now, read the clock before you name a moment: `date -Iseconds`. `--at` needs an offset; when the user names a time of day, it is their offset.
  - One event is one moment. To warn early, add a second event at the earlier moment.
  - Its moment must be still to come. There is no update: `events del ID`, then add again.
  - serve does one thing at a time. While it tries an event, which with the network down takes all of retries × timeout seconds, the next event waits and so does every command: space events that matter a minute apart.
  - Expired events are never deleted for you; they are the record. Delete only what the user wants gone.
  - **Function**: what happens at the moment. Today one kind, `pushover`, a message on the user's phone.
    - **Arguments**: the function's fields, by `--function`. Two to start from:
      - A notice, one short sound: `{"pushover":{"title":"…","message":"…","sound":"cashregister"}}`
      - An alarm, a long sound again every 30 seconds until the user taps it, for 25 minutes at most: `{"pushover":{"title":"…","message":"…","priority":2,"retry":30,"expire":1500,"sound":"updown"}}`. `retry` is the seconds between repeats, 30 at least; `expire` is the seconds it keeps repeating, 10800 at most; Pushover repeats 50 times at most.
      - A message others on the machine should not see in `ps` goes by `--function @-` from a heredoc.
    - **Result**: what came back, once the event has expired. `result.http.status` 2xx: done. Another status: failed, and `result.http.body` says why. `result.http` null: missed, and `result.error` says why. `result` null: missed and never tried, because all through its time serve was not running, or was held by another event. `events get --expired --short` names each one done, failed or missed. Nothing missed or failed is tried again: tell the user.
- **Keys**: they live in the home's `.env`, which only the user fills (`events serve --help` names them). When `result.error` names one, tell the user which and where it comes from. Never open `.env` or anything else in the home.
- **serve not running**: the README of the repo says how it is kept alive; say so to the user rather than starting one on your own.
- **Something wrong with events itself**: tell the user, and with their word open an issue at the address that ends `events --help`.

## Every command

```!
events --help
```

```!
events add --help
```

```!
events get --help
```

```!
events del --help
```

```!
events stop --help
```

```!
events serve --help
```

```!
events skill --help
```
