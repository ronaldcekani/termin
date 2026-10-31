//! Input editor state, shell tokenizer, history, and completions (`termin-editor`).
//!
//! See `docs/specs/input-editor.md`.

pub mod complete;
pub mod editor;
pub mod generators;
pub mod history;
pub mod lexer;
pub mod spec;
pub mod spec_compile;
pub mod specs_data;

// Re-exports of core types matching spec and contracts

pub use complete::{
    complete, AsyncJob, CompletionContext, CompletionResult, MAX_COMPLETION_CANDIDATES,
};
pub use editor::{
    Candidate, CompletionMenu, EditorState, HistoryNav, Suggestion, UndoStack, MAX_BUFFER_SIZE,
    MAX_UNDO_ENTRIES,
};
pub use generators::{
    generate_env_vars, generate_executables, generate_filepaths, generate_git_branches,
    generate_git_remotes, generate_git_tags, generate_history_args, generate_ssh_hosts,
    parse_git_status_porcelain_z, parse_make_targets, parse_npm_scripts,
};
pub use history::{
    decode_zsh_metafied, escape_tsv_string, format_tsv_entry, parse_bash_history, parse_tsv,
    parse_tsv_line, parse_zsh_history, parse_zsh_history_bytes, rotate_tsv, unescape_tsv_string,
    HistoryEntry, HistoryIndex, HistoryItem, HistorySearchResult, HISTORY_CAP,
};
pub use lexer::{
    active_command_segment, find_token_at, is_builtin, tokenize, CmdIdx, Token, TokenKind,
};
pub use spec::{
    ArgSpec, CommandSpec, OptionSpec, StaticArgSpec, StaticCommandSpec, StaticOptionSpec,
    StaticSubcommandSpec, SubcommandSpec,
};
pub use specs_data::{find_compiled_spec, COMPILED_SPECS};
