#!/usr/bin/env bash
# Print every command's --help when the events skill loads, so Claude need not guess one.
for command in "" add get del stop serve; do
  printf '```\n'
  # shellcheck disable=SC2086 # an empty command is the top-level help
  events $command --help || echo 'events is not installed or not on PATH: see the README of the repo'
  printf '```\n'
done
