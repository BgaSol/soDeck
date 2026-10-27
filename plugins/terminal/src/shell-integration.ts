/** No dotfile writes or keystroke injection. The SSH exec compatibility boundary
 * is explicit in the terminal setting and T-010 startup review. */
export function shellCommand(tag:string):string {
  if(!/^[a-f0-9-]{16,80}$/.test(tag))throw Error('Invalid integration tag');
  return `if [ -n "\${BASH_VERSION-}" ]; then
__sodeck_prompt() {
  local __sd_rc=$? __sd_stat __sd_rest __sd_start __sd_root __sd_ns __sd_logical __sd_physical
  local IFS=$' \\t\\n'
  [[ \${PS0-} == '$(__sodeck_busy)' && \${PROMPT_COMMAND-} == __sodeck_prompt ]] || return "$__sd_rc"
  if [[ -z \${__sd_pid-} ]]; then
    __sd_pid=$$; __sd_seq=0
    export -n __sd_pid __sd_seq PROMPT_COMMAND PS0 2>/dev/null
    export -nf __sodeck_prompt __sodeck_busy 2>/dev/null
  fi
  [[ $$ == "$__sd_pid" ]] || return "$__sd_rc"
  __sd_seq=$((__sd_seq+2))
  IFS= read -r __sd_stat < /proc/$$/stat
  __sd_rest=\${__sd_stat##*) }; set -- $__sd_rest; __sd_start=\${20}
  __sd_root=$(stat -Lc '%d;%i' -- /proc/$$/root 2>/dev/null) || return "$__sd_rc"
  __sd_ns=$(readlink /proc/$$/ns/mnt 2>/dev/null) || return "$__sd_rc"
  __sd_logical=$(printf '%s' "$PWD" | base64 -w0)
  __sd_physical=$(builtin pwd -P | base64 -w0)
  printf '\\033]633;SoDeck;1;${tag};%s;ready;%s;%s;%s;%s;%s;%s;%s\\033\\\\' "$__sd_seq" "$$" "$__sd_start" "$EUID" "$__sd_ns" "$__sd_root" "$__sd_logical" "$__sd_physical"
  return "$__sd_rc"
}
__sodeck_busy() {
  [[ -n \${__sd_pid-} && $$ == "$__sd_pid" ]] || return
  printf '\\033]633;SoDeck;1;${tag};%s;busy\\033\\\\' "$((__sd_seq+1))"
}
export -f __sodeck_prompt __sodeck_busy
export PROMPT_COMMAND=__sodeck_prompt
export PS0='$(__sodeck_busy)'
exec "$BASH" --login -i
else exec "\${SHELL:-/bin/sh}" -l; fi`;
}
