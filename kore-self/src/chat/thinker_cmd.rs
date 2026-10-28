//! `CommandThinker`: the chat thinker is a program the OWNER configures.
//!
//! SECURITY: this executes a program chosen by whoever wrote `thinker.txt`.
//! Only put a program there that you trust and would run yourself. What this
//! module does: runs it WITHOUT a shell (`std::process::Command`, the argument
//! list comes from `arg =` lines, never from a shell string), with an absolute
//! program path that must exist, working directory `<owner dir>/scratch`, a
//! cleared environment (PATH, SystemRoot, TEMP/TMP/TMPDIR=scratch, plus only the
//! variable names listed in `env_allow`), no stderr, a wall-clock timeout and an
//! output-size cap; the JSON request goes to its stdin and the JSON reply is
//! read from its stdout. What it can NOT do: stop the program from using the
//! network, the disk or your account on its own (it runs with your rights; this
//! crate has no sandbox). The reply can only PROPOSE tool calls; each goes
//! through `authorize` and the normal tool-argument validation before anything
//! happens.
//!
//! File `<owner dir>/thinker.txt` (hand-edited, reserved: no tool can read or write it):
//!
//! ```text
//! # one setting per line: key = value. Unknown keys are an error.
//! command = C:\tools\my_model_cli.exe     # absolute path, required
//! arg = --json                            # repeat for each argument, in order
//! arg = --model
//! arg = small
//! timeout_secs = 60                       # 1..=600, default 30
//! max_output_bytes = 65536                # 1024..=4194304, default 65536
//! env_allow = MY_API_KEY                  # repeat; names of variables to pass through
//! ```
//!
//! (A `#` only starts a comment at the beginning of a line.)

use super::{ChatContext, ChatReply, ChatThinker, ToolCall};
use crate::safety;
use serde_json::Value;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const THINKER_FILE: &str = "thinker.txt";
const MAX_CONFIG_BYTES: u64 = 16 * 1024;
const MAX_ARGS: usize = 64;
const MAX_TOOL_CALLS: usize = 5;
const MAX_REPLY_BYTES: usize = 8000;

#[derive(Debug, Clone, PartialEq)]
pub struct ThinkerConfig {
    pub command: PathBuf,
    pub args: Vec<String>,
    pub timeout_secs: u64,
    pub max_output_bytes: usize,
    pub env_allow: Vec<String>,
}

/// Parse the file text. Every problem is an error.
pub fn parse_config(text: &str) -> Result<ThinkerConfig, String> {
    let (mut command, mut args, mut timeout, mut cap, mut env) = (None::<String>, Vec::new(), 30u64, 65536usize, Vec::<String>::new());
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let l = raw.trim().trim_start_matches('\u{feff}');
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let Some((k, v)) = l.split_once('=') else {
            return Err(format!("line {n}: expected 'key = value'"));
        };
        let (k, v) = (k.trim(), v.trim());
        match k {
            "command" => {
                if v.is_empty() || command.is_some() {
                    return Err(format!("line {n}: 'command' must be given once and not be empty"));
                }
                command = Some(v.to_string());
            }
            "arg" => {
                if v.is_empty() || v.contains('\0') {
                    return Err(format!("line {n}: empty or invalid 'arg'"));
                }
                if args.len() >= MAX_ARGS {
                    return Err(format!("line {n}: more than {MAX_ARGS} args"));
                }
                args.push(v.to_string());
            }
            "timeout_secs" => {
                timeout = v.parse().ok().filter(|t| (1..=600).contains(t)).ok_or_else(|| format!("line {n}: timeout_secs must be 1..=600"))?;
            }
            "max_output_bytes" => {
                cap = v.parse().ok().filter(|c| (1024..=4_194_304).contains(c)).ok_or_else(|| format!("line {n}: max_output_bytes must be 1024..=4194304"))?;
            }
            "env_allow" => {
                if v.is_empty() || !v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    return Err(format!("line {n}: env_allow takes a variable name (letters, digits, '_')"));
                }
                env.push(v.to_string());
            }
            other => return Err(format!("line {n}: unknown key '{}'", safety::trunc(other, 40))),
        }
    }
    let command = command.ok_or("no 'command = ...' line")?;
    let command = PathBuf::from(command);
    if !command.is_absolute() {
        return Err(format!("command '{}' is not an absolute path (a relative path would be resolved against the working directory or PATH; refused)", command.display()));
    }
    Ok(ThinkerConfig { command, args, timeout_secs: timeout, max_output_bytes: cap, env_allow: env })
}

/// Load the owner's configuration. `Ok(None)`: no thinker.txt (the scripted thinker is used).
pub fn load_config(owner: &str) -> Result<Option<ThinkerConfig>, String> {
    load_config_file(owner, THINKER_FILE)
}

/// As `load_config` for another file of the same format in the owner directory (the advisor command uses `advisor.txt`).
pub fn load_config_file(owner: &str, file: &str) -> Result<Option<ThinkerConfig>, String> {
    let p = safety::owner_dir(owner).join(file);
    let meta = match std::fs::metadata(&p) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("cannot read {file}: {e}")),
    };
    if meta.len() > MAX_CONFIG_BYTES {
        return Err(format!("{file} is larger than {MAX_CONFIG_BYTES} bytes"));
    }
    if let Some(e) = crate::authorize::perm_problem(&p) {
        return Err(e);
    }
    let bytes = std::fs::read(&p).map_err(|e| format!("cannot read {file}: {e}"))?;
    let text = String::from_utf8(bytes).map_err(|_| format!("{file} is not valid UTF-8"))?;
    let cfg = parse_config(&text).map_err(|e| format!("{file}: {e}"))?;
    let md = std::fs::metadata(&cfg.command).map_err(|e| format!("{file}: command '{}' not found: {e}", cfg.command.display()))?;
    if !md.is_file() {
        return Err(format!("{file}: command '{}' is not a file", cfg.command.display()));
    }
    if let Some(e) = crate::authorize::perm_problem(&cfg.command) {
        return Err(format!("{file}: {e}"));
    }
    Ok(Some(cfg))
}

pub struct CommandThinker {
    pub cfg: ThinkerConfig,
    pub scratch: PathBuf,
}

impl CommandThinker {
    pub fn new(cfg: ThinkerConfig, owner: &str) -> Result<CommandThinker, String> {
        let scratch = safety::owner_dir(owner).join("scratch");
        std::fs::create_dir_all(&scratch).map_err(|e| format!("cannot create scratch dir: {e}"))?;
        Ok(CommandThinker { cfg, scratch })
    }

    /// Run the program once with `input` on stdin; returns its stdout (at most `max_output_bytes`).
    pub fn run(&self, input: &str) -> Result<Vec<u8>, String> {
        run_program(&self.cfg, &self.scratch, input)
    }
}

pub fn build_env(cfg: &ThinkerConfig, scratch: &Path) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = Vec::new();
    for k in ["PATH", "SystemRoot", "SYSTEMROOT"] {
        if let Ok(v) = std::env::var(k) {
            env.push((k.to_string(), v));
        }
    }
    let s = scratch.to_string_lossy().to_string();
    for k in ["TEMP", "TMP", "TMPDIR"] {
        env.push((k.to_string(), s.clone()));
    }
    for k in &cfg.env_allow {
        if let Ok(v) = std::env::var(k) {
            env.push((k.clone(), v));
        }
    }
    env
}

pub(crate) fn run_program(cfg: &ThinkerConfig, scratch: &Path, input: &str) -> Result<Vec<u8>, String> {
    if !cfg.command.is_absolute() {
        return Err("command path is not absolute".into());
    }
    let mut cmd = Command::new(&cfg.command);
    cmd.args(&cfg.args).current_dir(scratch).env_clear().stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    for (k, v) in build_env(cfg, scratch) {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().map_err(|e| format!("cannot start the external command: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let data = input.as_bytes().to_vec();
    // writer thread: a program that never reads must not block us
    std::thread::spawn(move || {
        let _ = stdin.write_all(&data);
        let _ = stdin.flush();
    });
    let mut stdout = child.stdout.take().ok_or("no stdout")?;
    let cap = cfg.max_output_bytes;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut stdout).take(cap as u64 + 1).read_to_end(&mut buf);
        buf
    });
    let deadline = Instant::now() + Duration::from_secs(cfg.timeout_secs);
    let fail = |child: &mut std::process::Child, msg: String| -> Result<Vec<u8>, String> {
        let _ = child.kill();
        let _ = child.wait();
        Err(msg)
    };
    while !reader.is_finished() {
        if Instant::now() >= deadline {
            return fail(&mut child, format!("the external command timed out after {}s and was killed", cfg.timeout_secs));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let out = reader.join().map_err(|_| "reader thread failed".to_string())?;
    if out.len() > cap {
        return fail(&mut child, format!("the external command printed more than {cap} bytes and was killed"));
    }
    loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                return if st.success() { Ok(out) } else { Err(format!("the external command exited with {st}")) };
            }
            Ok(None) if Instant::now() >= deadline => return fail(&mut child, format!("the external command did not exit within {}s and was killed", cfg.timeout_secs)),
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => return Err(format!("cannot wait for the external command: {e}")),
        }
    }
}

/// Parse `{"reply": "...", "tool_calls": [{"tool": "...", "args": {...}}]}`.
pub fn parse_reply(bytes: &[u8]) -> Result<ChatReply, String> {
    let v: Value = serde_json::from_slice(bytes).map_err(|e| format!("the reply is not valid JSON: {e}"))?;
    let obj = v.as_object().ok_or("the reply must be a JSON object")?;
    let reply = match obj.get("reply") {
        Some(Value::String(s)) => safety::trunc(s, MAX_REPLY_BYTES).to_string(),
        None | Some(Value::Null) => String::new(),
        Some(_) => return Err("'reply' must be a string".into()),
    };
    let mut calls = Vec::new();
    match obj.get("tool_calls") {
        None | Some(Value::Null) => {}
        Some(Value::Array(a)) => {
            for c in a {
                let tool = c.get("tool").and_then(|t| t.as_str()).ok_or("each tool call needs a string 'tool'")?;
                let args = match c.get("args") {
                    None | Some(Value::Null) => Value::Object(Default::default()),
                    Some(a) => a.clone(),
                };
                calls.push(ToolCall { tool: tool.to_string(), args });
            }
        }
        Some(_) => return Err("'tool_calls' must be an array".into()),
    }
    if reply.trim().is_empty() && calls.is_empty() {
        return Err("the reply has neither text nor tool calls".into());
    }
    let _ = MAX_TOOL_CALLS;
    Ok(ChatReply { reply, tool_calls: calls, raw: None })
}

impl ChatThinker for CommandThinker {
    fn id(&self) -> String {
        let name = self.cfg.command.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        format!("command:{}", safety::trunc(&name, 60))
    }

    fn think(&self, ctx: &ChatContext) -> Result<ChatReply, String> {
        let out = self.run(&ctx.request_json())?;
        let raw = String::from_utf8_lossy(&out).to_string();
        let mut r = parse_reply(&out).map_err(|e| format!("{e} (raw reply, cut: {})", safety::trunc(&raw, 500)))?;
        r.raw = Some(raw);
        Ok(r)
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_parsing() {
        let abs = if cfg!(windows) { "C:\\tools\\m.exe" } else { "/usr/bin/m" };
        let ok = parse_config(&format!("# c\ncommand = {abs}\narg = --a b\narg=c\ntimeout_secs = 5\nenv_allow = KEY_1\n")).unwrap();
        assert_eq!(ok.args, vec!["--a b", "c"]);
        assert_eq!((ok.timeout_secs, ok.max_output_bytes), (5, 65536));
        assert_eq!(ok.env_allow, vec!["KEY_1"]);
        for bad in [
            "".to_string(),
            "command = relative/m".to_string(),
            "command = m".to_string(),
            format!("command = {abs}\ncommand = {abs}"),
            format!("command = {abs}\nshell = yes"),
            format!("command = {abs}\ntimeout_secs = 0"),
            format!("command = {abs}\ntimeout_secs = 601"),
            format!("command = {abs}\nmax_output_bytes = 5"),
            format!("command = {abs}\nenv_allow = A B"),
            format!("command = {abs}\narg ="),
            format!("command = {abs}\njust words"),
        ] {
            assert!(parse_config(&bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn reply_parsing() {
        let r = parse_reply(br#"{"reply":"hi","tool_calls":[{"tool":"self_sql","args":{"sql":"SELECT 1"}}]}"#).unwrap();
        assert_eq!((r.reply.as_str(), r.tool_calls.len()), ("hi", 1));
        assert!(parse_reply(b"not json").is_err());
        assert!(parse_reply(b"[]").is_err());
        assert!(parse_reply(br#"{"reply": 3}"#).is_err());
        assert!(parse_reply(br#"{"reply":"x","tool_calls":[{"args":{}}]}"#).is_err());
        assert!(parse_reply(br#"{}"#).is_err());
        assert_eq!(parse_reply(br#"{"tool_calls":[{"tool":"being_status"}]}"#).unwrap().tool_calls[0].args, serde_json::json!({}));
    }

    #[test]
    fn env_is_minimal() {
        std::env::set_var("KORE_TEST_SECRET_X", "s3cret");
        let cfg = ThinkerConfig { command: PathBuf::from("/x"), args: vec![], timeout_secs: 1, max_output_bytes: 1024, env_allow: vec![] };
        let env = build_env(&cfg, Path::new("/scratch"));
        assert!(env.iter().all(|(k, _)| k != "KORE_TEST_SECRET_X"));
        let cfg2 = ThinkerConfig { env_allow: vec!["KORE_TEST_SECRET_X".into()], ..cfg };
        assert!(build_env(&cfg2, Path::new("/scratch")).iter().any(|(k, v)| k == "KORE_TEST_SECRET_X" && v == "s3cret"));
    }
}
