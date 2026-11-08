# The marker ties the shell's filesystem view to the file service, not an identity or permission.
_sd_emit() {
  local result=$? directory marker
  if [ "$1" = ready ]; then
    directory=$(pwd -P | LC_ALL=C od -An -v -tx1 | tr -d ' \n')
    # pwd's final newline is not part of the path.
    directory=${directory%0a}
    marker=$(printf '%s' "$_sd_root/identity" | LC_ALL=C od -An -v -tx1 | tr -d ' \n')
    printf '\033]633;SODECK;@TOKEN@;ready;%s;%s\007' "$directory" "$marker"
  else
    printf '\033]633;SODECK;@TOKEN@;%s;%s\007' "$1" "${2-}"
  fi
  return "$result"
}
