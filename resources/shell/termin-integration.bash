# Termin bash shell integration
# See docs/specs/pty-and-shell.md §4

if [ -n "$TERMIN_SHELL_INTEGRATION_LOADED" ]; then
  return 0
fi
export TERMIN_SHELL_INTEGRATION_LOADED=1

# Helper for RFC 3986 percent-encoding
__termin_urlencode() {
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
__termin_preexec_ran=0

__termin_precmd() {
  local exit_code=$?
  if [ $__termin_started -eq 0 ]; then
    __termin_started=1
    # Emit startup metadata: OSC 6973;termin;shell=bash;version=1;hist=<HISTFILE> ST
    printf '\033]6973;termin;shell=bash;version=1;hist=%s\033\\' "${HISTFILE:-$HOME/.bash_history}"
  else
    # OSC 133;D;<last exit> ST
    printf '\033]133;D;%d\033\\' "$exit_code"
  fi

  # OSC 7;file://<host><cwd> ST
  local host="${HOSTNAME:-localhost}"
  local encoded_pwd
  encoded_pwd="$(__termin_urlencode_path "$PWD")"
  printf '\033]7;file://%s%s\033\\' "$host" "$encoded_pwd"

  __termin_preexec_ran=0
}

__termin_preexec() {
  if [ $__termin_preexec_ran -eq 1 ]; then
    return 0
  fi
  # Avoid executing on PROMPT_COMMAND internal commands
  case "$BASH_COMMAND" in
    *__termin_precmd*|*__termin_setup_prompt*) return 0 ;;
  esac
  __termin_preexec_ran=1
  local cmd="$BASH_COMMAND"
  local encoded_cmd
  encoded_cmd="$(__termin_urlencode "$cmd")"
  # OSC 133;C;cmdline_url=<urlencoded cmd> ST
  printf '\033]133;C;cmdline_url=%s\033\\' "$encoded_cmd"
}

__termin_setup_prompt() {
  local mark_a='\[\033]133;A\033\\\]'
  local mark_b='\[\033]133;B\033\\\]'

  case "$PS1" in
    *"$mark_a"*) ;;
    *) PS1="${mark_a}${PS1}" ;;
  esac
  case "$PS1" in
    *"$mark_b"*) ;;
    *) PS1="${PS1}${mark_b}" ;;
  esac
}

# Chain DEBUG trap
__termin_existing_debug_trap="$(trap -p DEBUG | sed -e "s/^trap -- '//" -e "s/' DEBUG$//")"
if [ -n "$__termin_existing_debug_trap" ]; then
  trap '__termin_preexec; '"$__termin_existing_debug_trap" DEBUG
else
  trap '__termin_preexec' DEBUG
fi

# Chain PROMPT_COMMAND
if [ -z "$PROMPT_COMMAND" ]; then
  PROMPT_COMMAND="__termin_precmd; __termin_setup_prompt"
else
  case "$PROMPT_COMMAND" in
    *__termin_precmd*) ;;
    *) PROMPT_COMMAND="__termin_precmd; $PROMPT_COMMAND; __termin_setup_prompt" ;;
  esac
fi
