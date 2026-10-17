#!/bin/sh
# CLI failure fixture for cua::call. No desktop access or networking.
dir=$(dirname "$0")
echo $$ > "$dir/cli.pid"
case "$ICU_CLI_FAILURE_CHILD" in
  flood)
    # A descendant inherits stdout and stderr, so pipe EOF alone cannot end the read.
    sleep 30 & echo $! > "$dir/descendant.pid"
    head -c 34000000 /dev/zero | tr '\0' x
    wait ;;
  hang)
    sleep 30 & echo $! > "$dir/descendant.pid"
    wait ;;
  fail)
    printf 'fixture refused get_window_state' >&2
    exit 3 ;;
esac
