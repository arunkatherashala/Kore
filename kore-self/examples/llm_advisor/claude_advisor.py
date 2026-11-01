"""Advisor for the kore-self being (Track B4): asks the Claude CLI for advice on one menu.

Contract (see kore-self/src/chat/advisor_cmd.rs): ONE JSON request on stdin, ONE JSON reply on stdout.
The request holds only visible task features of the synthetic sandbox world (domain, difficulty,
listed cost and reward, optional sequence terms). No personal data is sent.

The reply is advice only; the being validates it strictly and never executes it.
"""
import json
import re
import subprocess
import sys

CLAUDE = r"C:\Program Files\Claude\claude.exe"
MODEL = "haiku"
TIMEOUT = 80  # below the 90 s timeout_secs of advisor.txt, so the wrapper cleans up its own child
DENY = "Bash,Edit,Write,Read,Glob,Grep,WebFetch,WebSearch,Agent,NotebookEdit,Task"


def main():
    req = json.load(sys.stdin)
    prompt = (
        "You advise a small learning agent in a simulated world. You get a menu of tasks. "
        "Choose ONE task (slot) and ONE strategy to maximise reward per energy. Reply with ONE JSON object "
        "and nothing else (no prose, no code fence).\n\n"
        + json.dumps(req)
    )
    p = subprocess.run(
        [CLAUDE, "-p", "--model", MODEL, "--no-session-persistence", "--output-format", "text",
         "--disallowedTools", DENY, "--max-turns", "1"],
        input=prompt, capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=TIMEOUT,
    )
    if p.returncode != 0:
        sys.exit(2)
    text = p.stdout.strip()
    m = re.search(r"\{.*\}", text, re.S)
    if not m:
        sys.exit(3)
    obj = json.loads(m.group(0))
    print(json.dumps(obj))


if __name__ == "__main__":
    main()
