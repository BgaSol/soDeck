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
  _sd_precmd() { local result=$?; if [[ $$ = $_sd_pid && $ZSH_SUBSHELL = 0 ]]; then _sd_emit ready; fi; return $result; }
  _sd_preexec() { local result=$?; if [[ $$ = $_sd_pid && $ZSH_SUBSHELL = 0 ]]; then _sd_emit busy; fi; return $result; }
  precmd_functions+=(_sd_precmd)
  preexec_functions+=(_sd_preexec)
  unset _sd_stage _sd_zdotdir _sd_zdotdir_set
else
  _sd_zdotdir=${ZDOTDIR-}
  _sd_zdotdir_set=${ZDOTDIR+x}
  ZDOTDIR=$_sd_root
fi
