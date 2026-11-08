umask 077
_sd_fallback() { printf '\033]633;SODECK;@TOKEN@;unavailable;prepare-failed\007'; exec @SHELL@ -l; }
command -v od >/dev/null && command -v tr >/dev/null || _sd_fallback
_sd_dir=$(mktemp -d /tmp/sodeck-terminal.XXXXXXXX) || _sd_fallback
_sd_cleanup() { rm -f -- "$_sd_dir/bashrc" "$_sd_dir/.zshenv" "$_sd_dir/.zprofile" "$_sd_dir/.zshrc" "$_sd_dir/.zlogin" "$_sd_dir/integration" "$_sd_dir/identity"; rmdir -- "$_sd_dir" 2>/dev/null; }
trap '_sd_cleanup' EXIT
trap 'exit 129' HUP
trap 'exit 143' TERM
printf '%s' '@TOKEN@' > "$_sd_dir/identity" || _sd_fallback
cat > "$_sd_dir/integration" <<'SODECK_INTEGRATION' || _sd_fallback
@INTEGRATION@
SODECK_INTEGRATION
case @SHELL@ in
  */bash)
    cat > "$_sd_dir/bashrc" <<'SODECK_BASH' || _sd_fallback
@BASH@
SODECK_BASH
    SODECK_OLD_ENV=${ENV-} SODECK_ENV_SET=${ENV+x} ENV="$_sd_dir/bashrc" @SHELL@ --posix --login -i
    ;;
  */zsh)
    for _sd_stage in .zshenv .zprofile .zshrc .zlogin; do
      printf "_sd_stage='%s'\n" "$_sd_stage" > "$_sd_dir/$_sd_stage" || _sd_fallback
      cat >> "$_sd_dir/$_sd_stage" <<'SODECK_ZSH' || _sd_fallback
@ZSH@
SODECK_ZSH
    done
    SODECK_ZDOTDIR=${ZDOTDIR-} SODECK_ZDOTDIR_SET=${ZDOTDIR+x} SODECK_DIR="$_sd_dir" ZDOTDIR="$_sd_dir" @SHELL@ -il
    ;;
esac
_sd_result=$?
exit "$_sd_result"
