use termin_editor::{complete, CompletionContext};

#[test]
fn test_golden_git_checkout() {
    let ctx = CompletionContext::new("/workspace", "/usr/bin", "zsh");

    // Typing "git ch"
    let res = complete("git ch", 6, &ctx);
    let labels: Vec<&str> = res.candidates.iter().map(|c| c.label.as_str()).collect();
    assert!(
        labels.contains(&"checkout"),
        "Must suggest checkout for 'git ch': {:?}",
        labels
    );

    // Typing "git checkout -"
    let res2 = complete("git checkout -", 14, &ctx);
    let flags: Vec<&str> = res2.candidates.iter().map(|c| c.label.as_str()).collect();
    assert!(
        flags.contains(&"-b"),
        "Must suggest -b for 'git checkout -': {:?}",
        flags
    );
    assert!(
        flags.contains(&"--force"),
        "Must suggest --force for 'git checkout -': {:?}",
        flags
    );
}

#[test]
fn test_golden_cargo_build_and_test() {
    let ctx = CompletionContext::new("/workspace", "/usr/bin", "zsh");

    // Typing "cargo b"
    let res = complete("cargo b", 7, &ctx);
    let labels: Vec<&str> = res.candidates.iter().map(|c| c.label.as_str()).collect();
    assert!(
        labels.contains(&"build"),
        "Must suggest build: {:?}",
        labels
    );

    // Typing "cargo build --"
    let res2 = complete("cargo build --", 14, &ctx);
    let flags: Vec<&str> = res2.candidates.iter().map(|c| c.label.as_str()).collect();
    assert!(
        flags.contains(&"--release"),
        "Must suggest --release: {:?}",
        flags
    );
    assert!(
        flags.contains(&"--workspace"),
        "Must suggest --workspace: {:?}",
        flags
    );
    assert!(
        flags.contains(&"--target"),
        "Must suggest --target: {:?}",
        flags
    );

    // Typing "cargo test --"
    let res3 = complete("cargo test --", 13, &ctx);
    let test_flags: Vec<&str> = res3.candidates.iter().map(|c| c.label.as_str()).collect();
    assert!(
        test_flags.contains(&"--doc"),
        "Must suggest --doc: {:?}",
        test_flags
    );
    assert!(
        test_flags.contains(&"--no-run"),
        "Must suggest --no-run: {:?}",
        test_flags
    );
}

#[test]
fn test_golden_docker_and_kubectl() {
    let ctx = CompletionContext::new("/workspace", "/usr/bin", "zsh");

    // "docker r"
    let res = complete("docker r", 8, &ctx);
    let labels: Vec<&str> = res.candidates.iter().map(|c| c.label.as_str()).collect();
    assert!(labels.contains(&"run"), "Must suggest run: {:?}", labels);
    assert!(labels.contains(&"rm"), "Must suggest rm: {:?}", labels);

    // "docker run -"
    let res2 = complete("docker run -", 12, &ctx);
    let flags: Vec<&str> = res2.candidates.iter().map(|c| c.label.as_str()).collect();
    assert!(flags.contains(&"-d"), "Must suggest -d: {:?}", flags);
    assert!(flags.contains(&"-p"), "Must suggest -p: {:?}", flags);

    // "kubectl g"
    let res3 = complete("kubectl g", 9, &ctx);
    let k_labels: Vec<&str> = res3.candidates.iter().map(|c| c.label.as_str()).collect();
    assert!(
        k_labels.contains(&"get"),
        "Must suggest get: {:?}",
        k_labels
    );

    // "kubectl get p"
    let res4 = complete("kubectl get p", 13, &ctx);
    let res_labels: Vec<&str> = res4.candidates.iter().map(|c| c.label.as_str()).collect();
    assert!(
        res_labels.contains(&"pods"),
        "Must suggest pods: {:?}",
        res_labels
    );
}

#[test]
fn test_golden_filepaths_completion() {
    let temp_dir = std::env::temp_dir().join("termin_golden_files");
    let _ = std::fs::create_dir_all(&temp_dir);
    let _ = std::fs::write(temp_dir.join("main.rs"), "fn main() {}");
    let _ = std::fs::write(temp_dir.join("lib.rs"), "pub mod foo;");
    let _ = std::fs::create_dir_all(temp_dir.join("src_sub"));

    let ctx = CompletionContext::new(temp_dir.to_str().unwrap(), "/usr/bin", "zsh");

    // "cat m"
    let res = complete("cat m", 5, &ctx);
    let labels: Vec<&str> = res.candidates.iter().map(|c| c.label.as_str()).collect();
    assert!(
        labels.contains(&"main.rs"),
        "Must suggest main.rs: {:?}",
        labels
    );

    // "cd s" -> folders only
    let res_cd = complete("cd s", 4, &ctx);
    let cd_labels: Vec<&str> = res_cd.candidates.iter().map(|c| c.label.as_str()).collect();
    assert!(
        cd_labels.contains(&"src_sub/"),
        "Must suggest src_sub/: {:?}",
        cd_labels
    );
    assert!(
        !cd_labels.contains(&"main.rs"),
        "Must not suggest main.rs for cd"
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}
