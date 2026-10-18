//! The server must refuse a non-loopback bind unless a token is configured.
use std::process::Command;

fn run(args: &[&str], token: Option<&str>) -> (Option<i32>, String) {
    let home = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kore-self"));
    cmd.args(args)
        .env("KORE_SELF_HOME", home.path())
        .env_remove("KORE_SELF_TOKEN");
    if let Some(t) = token {
        cmd.env("KORE_SELF_TOKEN", t);
    }
    let out = cmd.output().unwrap();
    (out.status.code(), String::from_utf8_lossy(&out.stderr).to_string())
}

#[test]
fn api_refuses_public_bind_without_token() {
    let (code, err) = run(&["bind-test", "api", "0", "--bind", "0.0.0.0"], None);
    assert_eq!(code, Some(2), "stderr: {err}");
    assert!(err.contains("KORE_SELF_TOKEN"), "stderr: {err}");
}

#[test]
fn live_refuses_public_bind_without_token() {
    let (code, err) = run(&["bind-test", "live", "0", "--bind", "0.0.0.0"], None);
    assert_eq!(code, Some(2), "stderr: {err}");
}

#[test]
fn short_token_is_not_enough() {
    let (code, _) = run(&["bind-test", "api", "0", "--bind", "0.0.0.0"], Some("short"));
    assert_eq!(code, Some(2));
}

#[test]
fn invalid_owner_is_rejected() {
    let (code, err) = run(&["../evil", "status"], None);
    assert_eq!(code, Some(2), "stderr: {err}");
}
