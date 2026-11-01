//! Completion engine: pipeline, spec walker, candidate collection, fuzzy ranking.
//!
//! See `docs/specs/input-editor.md §4`.

use nucleo_matcher::{Config, Matcher, Utf32Str};
use rustc_hash::FxHashSet;
use std::ops::Range;

use crate::editor::Candidate;
use crate::generators::*;
use crate::history::HistoryIndex;
use crate::lexer::{active_command_segment, tokenize, Token, TokenKind};
use crate::spec::*;
use crate::specs_data::find_compiled_spec;

/// Maximum number of candidates returned by completion engine.
pub const MAX_COMPLETION_CANDIDATES: usize = 200;

/// Asynchronous generator jobs to be executed by the background worker thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AsyncJob {
    GitChangedFiles { cwd: String },
    NpmScripts { package_json_path: String },
    MakeTargets { makefile_path: String },
}

/// Context supplied to the completion engine.
#[derive(Clone, Debug, Default)]
pub struct CompletionContext {
    pub cwd: String,
    pub env_path: String,
    pub shell_kind: String,
    pub env_vars: Vec<(String, String)>,
    pub history: Option<HistoryIndex>,
}

impl CompletionContext {
    pub fn new(
        cwd: impl Into<String>,
        env_path: impl Into<String>,
        shell_kind: impl Into<String>,
    ) -> Self {
        Self {
            cwd: cwd.into(),
            env_path: env_path.into(),
            shell_kind: shell_kind.into(),
            env_vars: Vec::new(),
            history: None,
        }
    }
}

/// Result produced by the completion pipeline.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompletionResult {
    pub candidates: Vec<Candidate>,
    pub replace_range: Range<usize>,
    pub async_jobs: Vec<AsyncJob>,
}

/// Query completion engine for candidates at current cursor offset.
pub fn complete(buffer: &str, cursor: usize, ctx: &CompletionContext) -> CompletionResult {
    let cursor = cursor.min(buffer.len());
    let tokens = tokenize(buffer);

    // 1. Identify active segment and token under cursor
    let (_, segment_tokens) = active_command_segment(&tokens, cursor);

    // Find the token at cursor (or trailing empty token)
    let (cursor_token, token_prefix, replace_range) =
        find_cursor_token(buffer, &segment_tokens, cursor);

    // 2. Identify the command in this segment
    let cmd_token = segment_tokens
        .iter()
        .find(|t| t.kind == TokenKind::Command && t.range.end <= cursor);

    let mut raw_candidates = Vec::new();
    let mut async_jobs = Vec::new();

    // If cursor is on the command itself (or no command has been typed yet)
    let is_completing_command = cmd_token.is_none()
        || cursor_token.is_some_and(|t| t.kind == TokenKind::Command && t.range.contains(&cursor));

    if is_completing_command {
        // Collect command candidates from compiled specs, builtins, and PATH
        for spec in crate::specs_data::COMPILED_SPECS {
            raw_candidates.push(Candidate {
                label: spec.name.to_string(),
                detail: Some(spec.description.to_string()),
                insert_text: spec.name.to_string(),
                score: 0,
            });
        }
        if !ctx.env_path.is_empty() {
            let execs = generate_executables(&ctx.env_path);
            raw_candidates.extend(execs);
        }
    } else {
        let cmd_tok = cmd_token.unwrap();
        let cmd_name = cmd_tok.as_str(buffer);

        // 3. Resolve spec for command
        if let Some(spec) = find_compiled_spec(cmd_name) {
            walk_spec_and_collect(
                spec,
                buffer,
                &segment_tokens,
                cursor_token,
                &token_prefix,
                ctx,
                &mut raw_candidates,
                &mut async_jobs,
            );
        } else {
            // Unknown command fallback: filepaths + history args
            raw_candidates.extend(generate_filepaths(&ctx.cwd, &token_prefix, false));
            if let Some(ref hist) = ctx.history {
                raw_candidates.extend(generate_history_args(hist, cmd_name));
            }
        }
    }

    // 6. Fuzzy rank, dedupe, and cap at 200
    let ranked = rank_and_dedupe_candidates(raw_candidates, &token_prefix);

    CompletionResult {
        candidates: ranked,
        replace_range,
        async_jobs,
    }
}

fn find_cursor_token<'a>(
    buffer: &'a str,
    tokens: &[&'a Token],
    cursor: usize,
) -> (Option<&'a Token>, String, Range<usize>) {
    for &tok in tokens {
        if (tok.range.contains(&cursor) || tok.range.end == cursor)
            && tok.kind != TokenKind::Whitespace
        {
            let prefix = &buffer[tok.range.start..cursor];
            return (Some(tok), prefix.to_string(), tok.range.clone());
        }
    }

    // Cursor is in whitespace or at end of buffer
    (None, String::new(), cursor..cursor)
}

#[allow(clippy::too_many_arguments)]
fn walk_spec_and_collect(
    spec: &'static StaticCommandSpec,
    buffer: &str,
    tokens: &[&Token],
    cursor_token: Option<&Token>,
    token_prefix: &str,
    ctx: &CompletionContext,
    candidates: &mut Vec<Candidate>,
    async_jobs: &mut Vec<AsyncJob>,
) {
    let mut current_sub: Option<&'static StaticSubcommandSpec> = None;
    let mut positional_arg_index = 0;

    // Filter tokens between command and cursor
    for &tok in tokens {
        if tok.kind == TokenKind::Command
            || tok.kind == TokenKind::Whitespace
            || tok.kind == TokenKind::Comment
        {
            continue;
        }

        // Stop if we reached the cursor token
        if let Some(cur_tok) = cursor_token {
            if tok.range == cur_tok.range {
                break;
            }
        }

        let word = tok.as_str(buffer);

        // Check if word matches a subcommand
        if current_sub.is_none() {
            if let Some(sub) = spec.find_subcommand(word) {
                current_sub = Some(sub);
                positional_arg_index = 0;
                continue;
            }
        } else if let Some(sub) = current_sub {
            if let Some(nested) = sub.find_subcommand(word) {
                current_sub = Some(nested);
                positional_arg_index = 0;
                continue;
            }
        }

        if !word.starts_with('-') {
            positional_arg_index += 1;
        }
    }

    // Determine what is expected at cursor
    let is_flag = token_prefix.starts_with('-');

    // Options/Flags
    if is_flag || token_prefix.is_empty() {
        let options = if let Some(sub) = current_sub {
            sub.options
        } else {
            spec.options
        };

        for opt in options {
            for &name in opt.names {
                candidates.push(Candidate {
                    label: name.to_string(),
                    detail: Some(opt.description.to_string()),
                    insert_text: name.to_string(),
                    score: 0,
                });
            }
        }

        for opt in spec.persistent_options {
            for &name in opt.names {
                candidates.push(Candidate {
                    label: name.to_string(),
                    detail: Some(opt.description.to_string()),
                    insert_text: name.to_string(),
                    score: 0,
                });
            }
        }
    }

    // Subcommands
    let subcommands = if let Some(sub) = current_sub {
        sub.subcommands
    } else {
        spec.subcommands
    };

    if !subcommands.is_empty() && !is_flag {
        for sub in subcommands {
            candidates.push(Candidate {
                label: sub.name.to_string(),
                detail: Some(sub.description.to_string()),
                insert_text: sub.name.to_string(),
                score: 0,
            });
            for &alias in sub.aliases {
                candidates.push(Candidate {
                    label: alias.to_string(),
                    detail: Some(format!("alias for {}", sub.name)),
                    insert_text: alias.to_string(),
                    score: 0,
                });
            }
        }
    }

    // Positional Arguments
    let args = if let Some(sub) = current_sub {
        sub.args
    } else {
        spec.args
    };

    let arg_spec = if !args.is_empty() {
        if positional_arg_index < args.len() {
            Some(&args[positional_arg_index])
        } else if args.last().is_some_and(|a| a.variadic) {
            args.last()
        } else {
            None
        }
    } else {
        None
    };

    if let Some(arg) = arg_spec {
        for &sug in arg.suggestions {
            candidates.push(Candidate {
                label: sug.to_string(),
                detail: Some(arg.name.to_string()),
                insert_text: sug.to_string(),
                score: 0,
            });
        }

        for &gen_name in arg.generators {
            run_generator(gen_name, token_prefix, ctx, candidates, async_jobs);
        }

        if let Some(template) = arg.template {
            match template {
                "filepaths" => {
                    candidates.extend(generate_filepaths(&ctx.cwd, token_prefix, false));
                }
                "folders" => {
                    candidates.extend(generate_filepaths(&ctx.cwd, token_prefix, true));
                }
                _ => {}
            }
        }
    }
}

fn run_generator(
    gen_id: &str,
    token_prefix: &str,
    ctx: &CompletionContext,
    candidates: &mut Vec<Candidate>,
    async_jobs: &mut Vec<AsyncJob>,
) {
    match gen_id {
        "filepaths" => {
            candidates.extend(generate_filepaths(&ctx.cwd, token_prefix, false));
        }
        "folders" => {
            candidates.extend(generate_filepaths(&ctx.cwd, token_prefix, true));
        }
        "executables" => {
            candidates.extend(generate_executables(&ctx.env_path));
        }
        "env_vars" => {
            candidates.extend(generate_env_vars(&ctx.env_vars, token_prefix));
        }
        "git_branches" => {
            candidates.extend(generate_git_branches(&ctx.cwd));
        }
        "git_tags" => {
            candidates.extend(generate_git_tags(&ctx.cwd));
        }
        "git_remotes" => {
            candidates.extend(generate_git_remotes(&ctx.cwd));
        }
        "ssh_hosts" => {
            candidates.extend(generate_ssh_hosts());
        }
        "history_args" => {
            if let Some(ref hist) = ctx.history {
                candidates.extend(generate_history_args(hist, ""));
            }
        }
        "git_changed_files" => {
            async_jobs.push(AsyncJob::GitChangedFiles {
                cwd: ctx.cwd.clone(),
            });
        }
        "npm_scripts" => {
            let pkg_json = std::path::Path::new(&ctx.cwd).join("package.json");
            async_jobs.push(AsyncJob::NpmScripts {
                package_json_path: pkg_json.to_string_lossy().to_string(),
            });
        }
        "make_targets" => {
            let makefile = std::path::Path::new(&ctx.cwd).join("Makefile");
            async_jobs.push(AsyncJob::MakeTargets {
                makefile_path: makefile.to_string_lossy().to_string(),
            });
        }
        _ => {}
    }
}

fn rank_and_dedupe_candidates(candidates: Vec<Candidate>, token_prefix: &str) -> Vec<Candidate> {
    if candidates.is_empty() {
        return Vec::new();
    }

    let mut seen = FxHashSet::default();
    let mut deduplicated = Vec::new();

    for c in candidates {
        if seen.insert(c.label.clone()) {
            deduplicated.push(c);
        }
    }

    if token_prefix.is_empty() {
        deduplicated.truncate(MAX_COMPLETION_CANDIDATES);
        return deduplicated;
    }

    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut prefix_buf = Vec::new();
    let query_u32 = Utf32Str::new(token_prefix, &mut prefix_buf);

    let prefix_lower = token_prefix.to_lowercase();
    let mut scored_candidates = Vec::with_capacity(deduplicated.len());
    let mut item_buf = Vec::new();

    for mut c in deduplicated {
        let label_lower = c.label.to_lowercase();
        let starts_with = label_lower.starts_with(&prefix_lower);

        item_buf.clear();
        let haystack = Utf32Str::new(&c.label, &mut item_buf);

        let match_score = matcher.fuzzy_match(haystack, query_u32);

        if starts_with || match_score.is_some() {
            let base_score = match_score.unwrap_or(1) as u32;
            // Prefix matches receive a substantial score bonus
            let bonus = if starts_with { 10_000 } else { 0 };
            c.score = base_score + bonus;
            scored_candidates.push(c);
        }
    }

    scored_candidates.sort_by_key(|a| std::cmp::Reverse(a.score));
    scored_candidates.truncate(MAX_COMPLETION_CANDIDATES);
    scored_candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_completion_git_subcommands() {
        let ctx = CompletionContext::new("/workspace", "/usr/bin", "zsh");
        let result = complete("git ch", 6, &ctx);
        assert!(!result.candidates.is_empty());

        let labels: Vec<&str> = result.candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"checkout"));
    }

    #[test]
    fn test_completion_cargo_options() {
        let ctx = CompletionContext::new("/workspace", "/usr/bin", "zsh");
        let result = complete("cargo build --rel", 17, &ctx);
        assert!(!result.candidates.is_empty());

        let labels: Vec<&str> = result.candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"--release"));
    }

    #[test]
    fn test_completion_filepaths() {
        let dir = std::env::temp_dir().join("termin_test_comp");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("test_file_a.txt"), "a");
        let _ = std::fs::write(dir.join("test_file_b.txt"), "b");

        let ctx = CompletionContext::new(dir.to_str().unwrap(), "/usr/bin", "zsh");
        let result = complete("cat test_", 9, &ctx);

        let labels: Vec<&str> = result.candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"test_file_a.txt"));
        assert!(labels.contains(&"test_file_b.txt"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
