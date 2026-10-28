//! `CommandAdvisor`: the being's advisor is a program the OWNER configures (Track B4).
//!
//! It reuses the external-command mechanism of the chat thinker (`thinker_cmd.rs`) with the same safety
//! rules: the owner writes `<owner dir>/advisor.txt` by hand (reserved: no tool can read or write it), same
//! format as `thinker.txt` (`command`, `arg`, `timeout_secs`, `max_output_bytes`, `env_allow`). The program
//! is run WITHOUT a shell, with an absolute path, working directory `<owner dir>/scratch`, a cleared
//! environment, no stderr, a wall-clock timeout and an output cap. It receives ONE JSON request on stdin (the
//! visible task features only, see `being/advice.rs`; the text is passed through the redactor first) and must
//! print ONE JSON reply (`slot`+`strategy`, optional `p_success`, optional `rule`, optional `note`); the reply is
//! validated strictly against the request. What it can NOT be stopped from doing: using the network, the disk
//! or your account on its own (it runs with your rights; this crate has no sandbox). Its advice is never
//! executed: it is one weighted input to the being's planner, and a proposed rule must pass the verified search.
//!
//! The prompt and the reply go into the log as part of the advice event (source `advisor`), redacted. After
//! `MAX_CONSECUTIVE_FAILURES` failures in a row (timeout, bad output, non-zero exit) the advisor is not asked
//! again during the same run. This crate contains no network code and no model: this adapter is tested only
//! against the helper example program, never a real model.

use super::thinker_cmd::{load_config_file, run_program, ThinkerConfig};
use crate::being::advice::{Advice, AdviceRequest, Advisor};
use crate::redact::Redactor;
use crate::safety;
use std::cell::Cell;
use std::path::PathBuf;

pub const ADVISOR_FILE: &str = "advisor.txt";
pub const MAX_CONSECUTIVE_FAILURES: u32 = 3;
const MAX_LOGGED_REPLY: usize = 4000;

/// Load `<owner dir>/advisor.txt`; the file is required when a being was created with advisor command.
pub fn load_config(owner: &str) -> Result<ThinkerConfig, String> {
    match load_config_file(owner, ADVISOR_FILE)? {
        Some(c) => Ok(c),
        None => Err(format!("{ADVISOR_FILE} does not exist in the owner directory: create it by hand (see README, Track B4) to use advisor command")),
    }
}

pub struct CommandAdvisor {
    cfg: ThinkerConfig,
    scratch: PathBuf,
    redactor: Redactor,
    failures: Cell<u32>,
}

impl CommandAdvisor {
    pub fn new(cfg: ThinkerConfig, owner: &str) -> Result<CommandAdvisor, String> {
        let dir = safety::owner_dir(owner);
        let scratch = dir.join("scratch");
        std::fs::create_dir_all(&scratch).map_err(|e| format!("cannot create scratch dir: {e}"))?;
        Ok(CommandAdvisor { cfg, scratch, redactor: Redactor::load(&dir), failures: Cell::new(0) })
    }

    fn fail(&self, msg: String, transcript: Option<(String, String)>) -> Advice {
        self.failures.set(self.failures.get() + 1);
        Advice { error: Some(msg), transcript, ..Default::default() }
    }
}

impl Advisor for CommandAdvisor {
    fn id(&self) -> String {
        let name = self.cfg.command.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let safe: String = name.chars().take(40).map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' { c } else { '_' }).collect();
        format!("command:{safe}")
    }

    fn advise(&self, req: &AdviceRequest) -> Advice {
        if self.failures.get() >= MAX_CONSECUTIVE_FAILURES {
            return Advice { error: Some(format!("not asked: the advisor command failed {MAX_CONSECUTIVE_FAILURES} times in a row during this run")), ..Default::default() };
        }
        let prompt = self.redactor.redact(&req.to_json().to_string()).text;
        match run_program(&self.cfg, &self.scratch, &prompt) {
            Err(e) => self.fail(e, Some((prompt, String::new()))),
            Ok(bytes) => {
                let raw = String::from_utf8_lossy(&bytes).to_string();
                let reply = self.redactor.redact(safety::trunc(&raw, MAX_LOGGED_REPLY)).text;
                match Advice::parse_reply(&bytes, req) {
                    Ok(mut a) => {
                        self.failures.set(0);
                        a.note = self.redactor.redact(&a.note).text;
                        a.transcript = Some((prompt, reply));
                        a
                    }
                    Err(e) => self.fail(format!("{e} (reply, cut: {})", safety::trunc(&reply, 300)), Some((prompt, reply))),
                }
            }
        }
    }
}
