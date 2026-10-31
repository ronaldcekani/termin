# Termin bash shell integration
# See docs/specs/pty-and-shell.md §4
if [ -n "$TERMIN_SHELL_INTEGRATION" ]; then
  return 0
fi
export TERMIN_SHELL_INTEGRATION=1
