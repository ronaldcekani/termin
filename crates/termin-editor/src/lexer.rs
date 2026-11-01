//! Shell lexer and syntax tokenizer (`termin-editor`).
//!
//! See `docs/specs/input-editor.md §2`.

use std::ops::Range;

/// Index of a command context within a multi-command or pipeline input.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CmdIdx(pub u32);

/// Shell syntax token kinds.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TokenKind {
    /// Command executable or builtin name.
    Command,
    /// Positional or command argument.
    Argument,
    /// Command flag (e.g. `-f`, `--force`, `--key=val`).
    Flag,
    /// Quoted string (`'...'` or `"..."`).
    String,
    /// Environment or shell variable (`$VAR`, `${VAR}`).
    Variable,
    /// Shell operator (`|`, `&&`, `||`, `;`, `&`, `>`, `<`, `>>`, `2>`, etc.).
    Operator,
    /// Comment (`# ...`).
    Comment,
    /// File or directory path.
    Path,
    /// Subshell expression (`$(...)`, `(...)`, `` `...` ``).
    Subshell,
    /// Whitespace (` `, `\t`, `\n`, `\r`).
    Whitespace,
    /// Lexical or syntax error (e.g. unterminated quote).
    Error,
}

/// Token with byte range and command context index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub range: Range<usize>,
    pub ctx: CmdIdx,
}

impl Token {
    #[inline]
    pub fn new(kind: TokenKind, range: Range<usize>, ctx: CmdIdx) -> Self {
        Self { kind, range, ctx }
    }

    #[inline]
    pub fn as_str<'a>(&self, input: &'a str) -> &'a str {
        &input[self.range.clone()]
    }
}

/// Tokenize shell input string into typed tokens.
///
/// Follows POSIX-ish shell syntax:
/// - Splits on operators to find command positions.
/// - Skips environment variable assignments (e.g. `FOO=bar cmd`).
/// - Skips command prefixes (`sudo`, `env`, `time`, `nohup`, `exec`).
/// - Handles single/double quotes, subshells, variables, comments, flags, paths.
pub fn tokenize(input: &str) -> Vec<Token> {
    if input.is_empty() {
        return Vec::new();
    }

    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut tokens = Vec::new();
    let mut i = 0;
    let mut ctx = CmdIdx(0);

    // Track command position within the current context.
    // When true, the next non-assignment, non-whitespace token can be a Command.
    let mut expect_command = true;

    while i < len {
        let b = bytes[i];

        // 1. Whitespace
        if b.is_ascii_whitespace() {
            let start = i;
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            tokens.push(Token::new(TokenKind::Whitespace, start..i, ctx));
            continue;
        }

        // 2. Comments (only valid at start of segment or after whitespace/operator)
        if b == b'#' {
            let prev_is_space_or_start = tokens.last().map_or(true, |t| {
                matches!(t.kind, TokenKind::Whitespace | TokenKind::Operator)
            });
            if prev_is_space_or_start {
                let start = i;
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
                tokens.push(Token::new(TokenKind::Comment, start..i, ctx));
                continue;
            }
        }

        // 3. Operators
        if let Some((op_len, bumps_cmd)) = match_operator(&bytes[i..]) {
            let start = i;
            i += op_len;
            tokens.push(Token::new(TokenKind::Operator, start..i, ctx));
            if bumps_cmd {
                ctx.0 += 1;
                expect_command = true;
            }
            continue;
        }

        // 4. Subshells: $(...) or `...` or (...)
        if bytes[i..].starts_with(b"$(") {
            let start = i;
            i += 2;
            let mut depth = 1;
            let mut closed = false;
            while i < len {
                match bytes[i] {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            i += 1;
                            closed = true;
                            break;
                        }
                    }
                    b'\'' => {
                        i += 1;
                        while i < len && bytes[i] != b'\'' {
                            i += 1;
                        }
                        if i < len {
                            i += 1;
                        }
                    }
                    b'"' => {
                        i += 1;
                        while i < len {
                            if bytes[i] == b'\\' && i + 1 < len {
                                i += 2;
                            } else if bytes[i] == b'"' {
                                i += 1;
                                break;
                            } else {
                                i += 1;
                            }
                        }
                    }
                    b'\\' if i + 1 < len => i += 1,
                    _ => {}
                }
                i += 1;
            }
            if closed {
                tokens.push(Token::new(TokenKind::Subshell, start..i, ctx));
            } else {
                tokens.push(Token::new(TokenKind::Error, start..len, ctx));
                i = len;
            }
            expect_command = false;
            continue;
        }

        if b == b'`' {
            let start = i;
            i += 1;
            let mut closed = false;
            while i < len {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2;
                } else if bytes[i] == b'`' {
                    i += 1;
                    closed = true;
                    break;
                } else {
                    i += 1;
                }
            }
            if closed {
                tokens.push(Token::new(TokenKind::Subshell, start..i, ctx));
            } else {
                tokens.push(Token::new(TokenKind::Error, start..len, ctx));
                i = len;
            }
            expect_command = false;
            continue;
        }

        if b == b'(' {
            let start = i;
            i += 1;
            let mut depth = 1;
            let mut closed = false;
            while i < len {
                match bytes[i] {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            i += 1;
                            closed = true;
                            break;
                        }
                    }
                    b'\'' => {
                        i += 1;
                        while i < len && bytes[i] != b'\'' {
                            i += 1;
                        }
                        if i < len {
                            i += 1;
                        }
                    }
                    b'"' => {
                        i += 1;
                        while i < len {
                            if bytes[i] == b'\\' && i + 1 < len {
                                i += 2;
                            } else if bytes[i] == b'"' {
                                i += 1;
                                break;
                            } else {
                                i += 1;
                            }
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            if closed {
                tokens.push(Token::new(TokenKind::Subshell, start..i, ctx));
            } else {
                tokens.push(Token::new(TokenKind::Error, start..len, ctx));
                i = len;
            }
            expect_command = false;
            continue;
        }

        // 5. Strings
        if b == b'\'' {
            let start = i;
            i += 1;
            let mut closed = false;
            while i < len {
                if bytes[i] == b'\'' {
                    i += 1;
                    closed = true;
                    break;
                }
                i += 1;
            }
            if closed {
                tokens.push(Token::new(TokenKind::String, start..i, ctx));
            } else {
                tokens.push(Token::new(TokenKind::Error, start..len, ctx));
                i = len;
            }
            expect_command = false;
            continue;
        }

        if b == b'"' {
            let start = i;
            i += 1;
            let mut closed = false;
            while i < len {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2;
                } else if bytes[i] == b'"' {
                    i += 1;
                    closed = true;
                    break;
                } else {
                    i += 1;
                }
            }
            if closed {
                tokens.push(Token::new(TokenKind::String, start..i, ctx));
            } else {
                tokens.push(Token::new(TokenKind::Error, start..len, ctx));
                i = len;
            }
            expect_command = false;
            continue;
        }

        // 6. Standalone Variables: $VAR or ${VAR} or $?, $!, $$, etc.
        if b == b'$' && i + 1 < len && is_var_start_byte(bytes[i + 1]) {
            let start = i;
            i += 1;
            if bytes[i] == b'{' {
                i += 1;
                while i < len && bytes[i] != b'}' {
                    i += 1;
                }
                if i < len && bytes[i] == b'}' {
                    i += 1;
                    tokens.push(Token::new(TokenKind::Variable, start..i, ctx));
                } else {
                    tokens.push(Token::new(TokenKind::Error, start..len, ctx));
                    i = len;
                }
            } else if is_special_var_char(bytes[i]) {
                i += 1;
                tokens.push(Token::new(TokenKind::Variable, start..i, ctx));
            } else {
                while i < len && is_var_body_byte(bytes[i]) {
                    i += 1;
                }
                tokens.push(Token::new(TokenKind::Variable, start..i, ctx));
            }
            expect_command = false;
            continue;
        }

        // 7. General Word (Command, Argument, Flag, Path, Variable assignment)
        let start = i;
        while i < len && !is_word_terminator(bytes[i]) {
            if bytes[i] == b'\\' && i + 1 < len {
                i += 2;
            } else {
                i += 1;
            }
        }
        let word_str = &input[start..i];

        // Check if environment assignment: NAME=value
        if is_env_assignment(word_str) {
            tokens.push(Token::new(TokenKind::Variable, start..i, ctx));
            // Still expecting command after FOO=bar
            continue;
        }

        if word_str.starts_with('-') && word_str.len() > 1 {
            tokens.push(Token::new(TokenKind::Flag, start..i, ctx));
            continue;
        }

        if is_path(word_str) {
            tokens.push(Token::new(TokenKind::Path, start..i, ctx));
            expect_command = false;
            continue;
        }

        if expect_command {
            tokens.push(Token::new(TokenKind::Command, start..i, ctx));
            // If prefix utility like sudo, next word is also command
            expect_command = is_command_prefix(word_str);
        } else {
            tokens.push(Token::new(TokenKind::Argument, start..i, ctx));
        }
    }

    tokens
}

fn match_operator(bytes: &[u8]) -> Option<(usize, bool)> {
    // Returns (len, bumps_cmd_context)
    const MULTI_OPS: &[(&[u8], bool)] = &[
        (b"2>&1", false),
        (b"|&", true),
        (b"&&", true),
        (b"||", true),
        (b">>", false),
        (b"<<<", false),
        (b"<<", false),
        (b"&>", false),
        (b"1>", false),
        (b"2>", false),
    ];

    for &(op, bumps) in MULTI_OPS {
        if bytes.starts_with(op) {
            return Some((op.len(), bumps));
        }
    }

    match bytes.first() {
        Some(b'|') => Some((1, true)),
        Some(b';') => Some((1, true)),
        Some(b'&') => Some((1, true)),
        Some(b'>') => Some((1, false)),
        Some(b'<') => Some((1, false)),
        _ => None,
    }
}

#[inline]
fn is_word_terminator(b: u8) -> bool {
    b.is_ascii_whitespace()
        || matches!(
            b,
            b'|' | b'&' | b';' | b'(' | b')' | b'<' | b'>' | b'`' | b'\'' | b'"'
        )
}

#[inline]
fn is_var_start_byte(b: u8) -> bool {
    b == b'{' || is_special_var_char(b) || b.is_ascii_alphabetic() || b == b'_'
}

#[inline]
fn is_var_body_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[inline]
fn is_special_var_char(b: u8) -> bool {
    matches!(
        b,
        b'?' | b'!' | b'$' | b'#' | b'*' | b'@' | b'0'..=b'9' | b'-'
    )
}

#[inline]
fn is_path(s: &str) -> bool {
    s.starts_with("~/")
        || s == "~"
        || s.starts_with("./")
        || s == "."
        || s.starts_with("../")
        || s == ".."
        || s.contains('/')
}

fn is_env_assignment(s: &str) -> bool {
    if let Some(eq_pos) = s.find('=') {
        if eq_pos == 0 {
            return false;
        }
        let var_part = &s[..eq_pos];
        let bytes = var_part.as_bytes();
        if bytes[0].is_ascii_alphabetic() || bytes[0] == b'_' {
            return bytes
                .iter()
                .all(|&b| b.is_ascii_alphanumeric() || b == b'_');
        }
    }
    false
}

#[inline]
fn is_command_prefix(cmd: &str) -> bool {
    matches!(
        cmd,
        "sudo" | "env" | "time" | "nohup" | "exec" | "builtin" | "command"
    )
}

/// Check if a string is a standard POSIX/bash/zsh shell builtin.
pub fn is_builtin(cmd: &str) -> bool {
    matches!(
        cmd,
        "cd" | "echo"
            | "export"
            | "alias"
            | "unalias"
            | "source"
            | "."
            | "set"
            | "unset"
            | "read"
            | "exit"
            | "history"
            | "type"
            | "which"
            | "pwd"
            | "true"
            | "false"
            | "test"
            | "["
            | "exec"
            | "eval"
            | "shift"
            | "kill"
            | "trap"
            | "wait"
            | "bg"
            | "fg"
            | "jobs"
            | "umask"
            | "local"
            | "declare"
            | "pushd"
            | "popd"
            | "dirs"
            | "hash"
            | "help"
            | "builtin"
            | "command"
            | "printf"
            | "return"
    )
}

/// Find the token occupying or immediately preceding the given byte offset.
pub fn find_token_at(tokens: &[Token], offset: usize) -> Option<&Token> {
    for token in tokens {
        if token.range.contains(&offset) || token.range.end == offset {
            return Some(token);
        }
    }
    tokens.last()
}

/// Find all tokens in the command context encompassing `cursor`.
pub fn active_command_segment(tokens: &[Token], cursor: usize) -> (CmdIdx, Vec<&Token>) {
    let ctx = tokens
        .iter()
        .find(|t| t.range.contains(&cursor) || t.range.end == cursor)
        .map_or(tokens.last().map_or(CmdIdx(0), |t| t.ctx), |t| t.ctx);

    let segment_tokens: Vec<&Token> = tokens.iter().filter(|t| t.ctx == ctx).collect();
    (ctx, segment_tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_basic_command() {
        let tokens = tokenize("ls -la /tmp");
        assert_eq!(tokens.len(), 5);
        assert_eq!(tokens[0].kind, TokenKind::Command);
        assert_eq!(tokens[0].range, 0..2);
        assert_eq!(tokens[1].kind, TokenKind::Whitespace);
        assert_eq!(tokens[2].kind, TokenKind::Flag);
        assert_eq!(tokens[2].range, 3..6);
        assert_eq!(tokens[3].kind, TokenKind::Whitespace);
        assert_eq!(tokens[4].kind, TokenKind::Path);
        assert_eq!(tokens[4].range, 7..11);
    }

    #[test]
    fn test_tokenize_pipeline_and_operators() {
        let tokens = tokenize("cat foo.txt | grep -i bar && echo done");
        let op_pipe = tokens
            .iter()
            .find(|t| t.kind == TokenKind::Operator && t.range == (12..13));
        assert!(op_pipe.is_some());
        let op_and = tokens
            .iter()
            .find(|t| t.kind == TokenKind::Operator && t.range == (26..28));
        assert!(op_and.is_some());

        // Check command context increments
        let cat_tok = &tokens[0];
        assert_eq!(cat_tok.kind, TokenKind::Command);
        assert_eq!(cat_tok.ctx, CmdIdx(0));

        let grep_tok = tokens
            .iter()
            .find(|t| {
                t.kind == TokenKind::Command
                    && t.as_str("cat foo.txt | grep -i bar && echo done") == "grep"
            })
            .unwrap();
        assert_eq!(grep_tok.ctx, CmdIdx(1));

        let echo_tok = tokens
            .iter()
            .find(|t| {
                t.kind == TokenKind::Command
                    && t.as_str("cat foo.txt | grep -i bar && echo done") == "echo"
            })
            .unwrap();
        assert_eq!(echo_tok.ctx, CmdIdx(2));
    }

    #[test]
    fn test_tokenize_quotes_and_errors() {
        let input = "echo 'hello world' \"foo bar\"";
        let tokens = tokenize(input);
        let strings: Vec<&Token> = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::String)
            .collect();
        assert_eq!(strings.len(), 2);
        assert_eq!(strings[0].as_str(input), "'hello world'");
        assert_eq!(strings[1].as_str(input), "\"foo bar\"");

        let unclosed = "echo 'unterminated";
        let tokens2 = tokenize(unclosed);
        assert!(tokens2.iter().any(|t| t.kind == TokenKind::Error));
    }

    #[test]
    fn test_tokenize_env_and_prefix() {
        let input = "FOO=bar sudo git commit -m 'initial'";
        let tokens = tokenize(input);
        assert_eq!(tokens[0].kind, TokenKind::Variable); // FOO=bar
        let cmd_tokens: Vec<&Token> = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::Command)
            .collect();
        assert_eq!(cmd_tokens.len(), 2);
        assert_eq!(cmd_tokens[0].as_str(input), "sudo");
        assert_eq!(cmd_tokens[1].as_str(input), "git");
    }

    #[test]
    fn test_tokenize_subshell_and_variable() {
        let input = "echo $(date +%s) $USER ${HOME}";
        let tokens = tokenize(input);
        let sub = tokens
            .iter()
            .find(|t| t.kind == TokenKind::Subshell)
            .unwrap();
        assert_eq!(sub.as_str(input), "$(date +%s)");
        let vars: Vec<&Token> = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::Variable)
            .collect();
        assert_eq!(vars.len(), 2);
        assert_eq!(vars[0].as_str(input), "$USER");
        assert_eq!(vars[1].as_str(input), "${HOME}");
    }
}
