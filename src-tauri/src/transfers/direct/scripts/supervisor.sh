#!/bin/sh
# Protocol 2. No user path is parsed as a line protocol or evaluated as code.
# Each run is launched using `exec sh` in its own SSH execution process group.
set -f
umask 077
mode=$1
root=$2
run=$3
attempt=$4
case "$run:$attempt" in *[!a-f0-9:-]*) exit 64;; esac
test -d "$root" && test ! -L "$root" || exit 64
IFS= read -r actual < "$root/run" || exit 64
test "$actual" = "$run" || exit 64

lock() {
    tries=0
    until mkdir "$root/gate" 2>/dev/null; do
        tries=$((tries + 1))
        test "$tries" -lt 5 && test -d "$root" || return 1
        sleep 1
    done
}
unlock() { rmdir "$root/gate"; }
seal() {
    lock || return 1
    : > "$root/closed" || { unlock; return 1; }
    unlock
}
record() {
    # All fields are bounded ASCII tokens, never paths or diagnostic output.
    printf '2 %s %s %s %s\n' "$run" "$attempt" "$1" "$2" > "$record.next" &&
        mv "$record.next" "$record"
}
lease() {
    { test -f "$root/closed" || test -f "$root/stop"; } && return 1
    value=
    IFS= read -r value < "$root/lease" 2>/dev/null || value=
    # A heartbeat write may momentarily be incomplete; count it as a missed
    # tick, never as immediate cancellation.
    case "$value" in ''|*[!0-9]*) idle=$((idle + 1)); test "$idle" -lt 30; return $?;; esac
    if test "$value" = "$last"; then idle=$((idle + 1)); else idle=0; last=$value; fi
    test "$idle" -lt 30
}

case "$mode" in
    seal) seal; exit $?;;
    guard)
        attempt=00000000-0000-0000-0000-000000000000
        record="$root/guard.state"
        trap 'seal; record closed 0; exit 0' HUP INT TERM
        lock || exit 74
        if test -f "$root/closed" || test -d "$root/guard-admitted"; then unlock; exit 75; fi
        mkdir "$root/guard-admitted" || { unlock; exit 74; }
        unlock || exit 74
        last=; idle=0
        record ready 0 || exit 74
        while lease; do sleep 1; done
        seal && record closed 0
        exit $?;;
    run) ;;
    *) exit 64;;
esac

job="$root/job-$attempt"
test -d "$job" && test ! -L "$job" || exit 64
record="$job/state"
owner=$$
group=$(ps -o pgid= -p "$owner") || exit 69
set -- $group
if test "$#" -ne 1 || test "$1" != "$owner"; then
    # A request that was never admitted must never overwrite the receipt of
    # another invocation of this attempt.
    printf '%s\n' '远端 SSH 命令没有独立进程组，不能安全监督直传' >&2
    exit 69
fi
cancelled=0
trap 'cancelled=1' HUP INT TERM
lock || exit 74
if test -f "$root/closed" || test -e "$job/admitted" || test ! -f "$job/command.sh"; then
    unlock
    exit 75
fi
mkdir "$job/admitted" || { unlock; exit 74; }
record running 0 || { unlock; exit 74; }
unlock || exit 74

# A background command otherwise inherits /dev/null as stdin on POSIX shells.
# Keep the binary stream explicitly, independently of lease/control files.
exec 3<&0
sh "$job/command.sh" <&3 &
child=$!
terminate() {
    cancelled=1
    trap '' HUP INT TERM
    # The live group leader (this shell) prevents process-group ID reuse.
    kill -TERM -"$owner" 2>/dev/null || :
    sleep 2
    # Never signal a saved child PID. If termination cannot be confirmed,
    # kill our still-owned group; no terminal receipt is written in that case.
    if kill -0 "$child" 2>/dev/null; then kill -KILL -"$owner"; fi
}
trap 'terminate' HUP INT TERM
(
    trap - HUP INT TERM
    last=; idle=0
    while test ! -f "$job/done"; do
        if ! lease; then kill -TERM -"$owner" 2>/dev/null; exit 0; fi
        sleep 1
    done
) < /dev/null &
watcher=$!
test "$cancelled" -eq 0 || terminate
wait "$child"
code=$?
if test "$cancelled" -ne 0; then
    # wait may have been interrupted by a trap. Reap before recording stopped.
    wait "$child" 2>/dev/null || :
    phase=cancelled; code=130
elif test "$code" -eq 0; then phase=done
else phase=failed
fi
: > "$job/done" || exit 74
trap '' HUP INT TERM
kill -TERM -"$owner" 2>/dev/null || :
wait "$watcher" 2>/dev/null || :
record "$phase" "$code" || exit 74
exit "$code"
