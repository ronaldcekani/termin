# Termin zsh shell integration
# See docs/specs/pty-and-shell.md §4

if [[ -n "$TERMIN_SHELL_INTEGRATION_LOADED" ]]; then
  return 0
fi
export TERMIN_SHELL_INTEGRATION_LOADED=1

# Helper for RFC 3986 percent-encoding
__termin_urlencode() {
  local LC_ALL=C
  local string="$1"
  local length="${#string}"
  local i char
  for (( i = 0; i < length; i++ )); do
    char="${string:$i:1}"
    case "$char" in
      [a-zA-Z0-9.~_-]) printf '%s' "$char" ;;
      *) printf '%%%02X' "'$char" ;;
    esac
  done
}

# Helper for URL path encoding (preserves /)
__termin_urlencode_path() {
  local LC_ALL=C
  local string="$1"
  local length="${#string}"
  local i char
  for (( i = 0; i < length; i++ )); do
    char="${string:$i:1}"
    case "$char" in
      [a-zA-Z0-9.~_/-]) printf '%s' "$char" ;;
      *) printf '%%%02X' "'$char" ;;
    esac
  done
}

__termin_started=0
__termin_last_exit=0

__termin_precmd() {
  local exit_code=$?
  if [[ $__termin_started -eq 0 ]]; then
    __termin_started=1
    # Emit startup metadata: OSC 6973;termin;shell=zsh;version=1;hist=<HISTFILE> ST
    printf '\033]6973;termin;shell=zsh;version=1;hist=%s\033\\' "${HISTFILE:-$HOME/.zsh_history}"
  else
    # OSC 133;D;<last exit> ST
    printf '\033]133;D;%d\033\\' "$exit_code"
  fi

  # OSC 7;file://<host><cwd> ST
  local host="${HOST:-localhost}"
  local encoded_pwd
  encoded_pwd="$(__termin_urlencode_path "$PWD")"
  printf '\033]7;file://%s%s\033\\' "$host" "$encoded_pwd"
}

__termin_preexec() {
  local cmd="$1"
  local encoded_cmd
  encoded_cmd="$(__termin_urlencode "$cmd")"
  # OSC 133;C;cmdline_url=<urlencoded cmd> ST
  printf '\033]133;C;cmdline_url=%s\033\\' "$encoded_cmd"
}

__termin_setup_prompt() {
  local mark_a=$'%{\033]133;A\033\\%}'
  local mark_b=$'%{\033]133;B\033\\%}'

  if [[ "$PS1" != *"$mark_a"* ]]; then
    PS1="${mark_a}${PS1}"
  fi
  if [[ "$PS1" != *"$mark_b"* ]]; then
    PS1="${PS1}${mark_b}"
  fi
}

autoload -Uz add-zsh-hook
add-zsh-hook precmd __termin_precmd
add-zsh-hook precmd __termin_setup_prompt
add-zsh-hook preexec __termin_preexec
