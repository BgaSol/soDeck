if [[ $_sd_stage = .zshenv ]]; then
  _sd_root=$SODECK_DIR
  if [[ $SODECK_ZDOTDIR_SET = x ]]; then ZDOTDIR=$SODECK_ZDOTDIR; else unset ZDOTDIR; fi
  unset SODECK_DIR SODECK_ZDOTDIR SODECK_ZDOTDIR_SET
else
  if [[ $_sd_zdotdir_set = x ]]; then ZDOTDIR=$_sd_zdotdir; else unset ZDOTDIR; fi
fi
if [[ -o rcs && -r ${ZDOTDIR:-$HOME}/$_sd_stage ]]; then source "${ZDOTDIR:-$HOME}/$_sd_stage"; fi
if [[ $_sd_stage = .zlogin ]]; then
  source "$_sd_root/integration"
  _sd_pid=$$
  _sd_sequence=0
  _sd_running=0
  _sd_original_ps1=$PS1
  _sd_integrated_ps1=''
  _sd_precmd() {
    local result=$?
    if [[ $$ = $_sd_pid && $ZSH_SUBSHELL = 0 ]]; then
      if [[ $_sd_running = 1 ]]; then _sd_reading end "$result"; fi
      _sd_running=0
      _sd_sequence=$((_sd_sequence + 1))
      _sd_emit ready
      _sd_reading prompt
      if [[ $PS1 != "$_sd_integrated_ps1" ]]; then _sd_original_ps1=$PS1; fi
      _sd_integrated_ps1="${_sd_original_ps1}"'%{'"$(printf '\033]633;SODECK;@TOKEN@;reading;1;input;%s\007' "$_sd_sequence")"'%}'
      PS1=$_sd_integrated_ps1
    fi
    return $result
  }
  _sd_preexec() { local result=$?; if [[ $$ = $_sd_pid && $ZSH_SUBSHELL = 0 ]]; then _sd_running=1; _sd_emit busy; _sd_reading start; fi; return $result; }
  precmd_functions+=(_sd_precmd)
  preexec_functions+=(_sd_preexec)
  unset _sd_stage _sd_zdotdir _sd_zdotdir_set
else
  _sd_zdotdir=${ZDOTDIR-}
  _sd_zdotdir_set=${ZDOTDIR+x}
  ZDOTDIR=$_sd_root
fi
