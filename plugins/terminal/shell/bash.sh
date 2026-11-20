# POSIX startup uses ENV once; leave POSIX mode before any user startup code.
set +o posix
_sd_root=${ENV%/bashrc}
if [ "$SODECK_ENV_SET" = x ]; then ENV=$SODECK_OLD_ENV; else unset ENV; fi
unset SODECK_ENV_SET SODECK_OLD_ENV
if [ -r /etc/profile ]; then . /etc/profile; fi
for _sd_profile in "$HOME/.bash_profile" "$HOME/.bash_login" "$HOME/.profile"; do
  if [ -r "$_sd_profile" ]; then . "$_sd_profile"; break; fi
done
unset _sd_profile
. "$_sd_root/integration"
if [[ ${BASH_VERSINFO[0]} -lt 4 || -n $(trap -p DEBUG) ]] || shopt -qo functrace || shopt -q extdebug; then
  _sd_emit unavailable hook-conflict
else
  _sd_pid=$BASHPID
  _sd_at_prompt=0
  _sd_sequence=0
  _sd_original_ps1=$PS1
  _sd_integrated_ps1=''
  _sd_prompt() {
    local result=$?
    if [[ $BASHPID = "$_sd_pid" ]]; then
      if [[ $_sd_sequence -gt 0 && $_sd_at_prompt = 0 ]]; then _sd_reading end "$result"; fi
      _sd_sequence=$((_sd_sequence + 1))
      _sd_emit ready
      _sd_reading prompt
      if [[ $PS1 != "$_sd_integrated_ps1" ]]; then _sd_original_ps1=$PS1; fi
      _sd_integrated_ps1="${_sd_original_ps1}"'\[\e]633;SODECK;@TOKEN@;reading;1;input;'"$_sd_sequence"'\a\]'
      PS1=$_sd_integrated_ps1
      _sd_at_prompt=1
    fi
    return "$result"
  }
  _sd_preexec() {
    local result=$?
    if [[ $BASHPID = "$_sd_pid" && $_sd_at_prompt = 1 ]]; then
      _sd_at_prompt=0
      _sd_emit busy
      _sd_reading start
    fi
    return "$result"
  }
  PROMPT_COMMAND=("${PROMPT_COMMAND[@]}" '_sd_prompt')
  trap '_sd_preexec' DEBUG
fi
