use termin_editor::{tokenize, CmdIdx, TokenKind};

#[test]
fn test_tokenizer_table_quotes() {
    let cases = [
        (
            "echo 'single quoted'",
            vec![
                (TokenKind::Command, "echo"),
                (TokenKind::Whitespace, " "),
                (TokenKind::String, "'single quoted'"),
            ],
        ),
        (
            "echo \"double quoted\"",
            vec![
                (TokenKind::Command, "echo"),
                (TokenKind::Whitespace, " "),
                (TokenKind::String, "\"double quoted\""),
            ],
        ),
        (
            "echo \"escaped \\\" quote\"",
            vec![
                (TokenKind::Command, "echo"),
                (TokenKind::Whitespace, " "),
                (TokenKind::String, "\"escaped \\\" quote\""),
            ],
        ),
        (
            "echo 'unterminated",
            vec![
                (TokenKind::Command, "echo"),
                (TokenKind::Whitespace, " "),
                (TokenKind::Error, "'unterminated"),
            ],
        ),
        (
            "echo \"unterminated double",
            vec![
                (TokenKind::Command, "echo"),
                (TokenKind::Whitespace, " "),
                (TokenKind::Error, "\"unterminated double"),
            ],
        ),
    ];

    for (input, expected) in cases {
        let tokens = tokenize(input);
        assert_eq!(
            tokens.len(),
            expected.len(),
            "Token count mismatch for input: {}",
            input
        );
        for (tok, (exp_kind, exp_str)) in tokens.iter().zip(expected.iter()) {
            assert_eq!(&tok.kind, exp_kind, "Kind mismatch in: {}", input);
            assert_eq!(tok.as_str(input), *exp_str, "Text mismatch in: {}", input);
        }
    }
}

#[test]
fn test_tokenizer_table_variables() {
    let cases = [
        ("$VAR", TokenKind::Variable, "$VAR"),
        ("${USER_NAME}", TokenKind::Variable, "${USER_NAME}"),
        ("$?", TokenKind::Variable, "$?"),
        ("$!", TokenKind::Variable, "$!"),
        ("$$", TokenKind::Variable, "$$"),
        ("$#", TokenKind::Variable, "$#"),
        ("$@", TokenKind::Variable, "$@"),
        ("$0", TokenKind::Variable, "$0"),
    ];

    for (input, exp_kind, exp_str) in cases {
        let tokens = tokenize(input);
        assert_eq!(tokens.len(), 1, "Failed for {}", input);
        assert_eq!(tokens[0].kind, exp_kind);
        assert_eq!(tokens[0].as_str(input), exp_str);
    }
}

#[test]
fn test_tokenizer_table_operators_and_pipes() {
    let input = "cargo build --release | tee build.log && ./run.sh >> out.txt 2>&1 || echo failed";
    let tokens = tokenize(input);

    let operators: Vec<&str> = tokens
        .iter()
        .filter(|t| t.kind == TokenKind::Operator)
        .map(|t| t.as_str(input))
        .collect();

    assert_eq!(operators, vec!["|", "&&", ">>", "2>&1", "||"]);

    // Check command contexts
    let commands: Vec<(&str, CmdIdx)> = tokens
        .iter()
        .filter(|t| t.kind == TokenKind::Command)
        .map(|t| (t.as_str(input), t.ctx))
        .collect();

    assert_eq!(
        commands,
        vec![
            ("cargo", CmdIdx(0)),
            ("tee", CmdIdx(1)),
            ("echo", CmdIdx(3)),
        ]
    );
}

#[test]
fn test_tokenizer_table_escapes_and_paths() {
    let input = "cat My\\ File.txt /var/log/syslog ~/.bashrc ./script.sh ../parent";
    let tokens = tokenize(input);

    let paths: Vec<&str> = tokens
        .iter()
        .filter(|t| t.kind == TokenKind::Path)
        .map(|t| t.as_str(input))
        .collect();

    assert!(paths.contains(&"/var/log/syslog"));
    assert!(paths.contains(&"~/.bashrc"));
    assert!(paths.contains(&"./script.sh"));
    assert!(paths.contains(&"../parent"));
}

#[test]
fn test_tokenizer_table_comments() {
    let input = "cargo test # run all tests\nls";
    let tokens = tokenize(input);

    let comments: Vec<&str> = tokens
        .iter()
        .filter(|t| t.kind == TokenKind::Comment)
        .map(|t| t.as_str(input))
        .collect();

    assert_eq!(comments, vec!["# run all tests"]);

    // Ensure '#' in middle of word is not treated as comment
    let input2 = "echo issue#123";
    let tokens2 = tokenize(input2);
    assert!(!tokens2.iter().any(|t| t.kind == TokenKind::Comment));
}

#[test]
fn test_tokenizer_table_subshells() {
    let input = "echo $(uname -m) `hostname` (subshell)";
    let tokens = tokenize(input);

    let subshells: Vec<&str> = tokens
        .iter()
        .filter(|t| t.kind == TokenKind::Subshell)
        .map(|t| t.as_str(input))
        .collect();

    assert_eq!(subshells, vec!["$(uname -m)", "`hostname`", "(subshell)"]);
}
