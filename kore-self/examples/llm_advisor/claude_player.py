"""LLM player for Track V1 (kore-self, measure_vs_world llm): asks the Claude CLI for one move in a World v2 menu.

Contract: ONE JSON request on stdin (built by kore-self/src/being/vs_world/llm.rs: synthetic task features of the
sandbox world and the player's own told outcomes only; its key sets are fixed by a test), ONE line on stdout: the
model's reply text, cut to the first JSON object. The harness validates the reply strictly; nothing is executed.
Same CLI settings as the B4b advisor wrapper (claude_advisor.py): haiku alias, tools denied, one turn, no session.
"""
import json
import re
import subprocess
import sys

CLAUDE = r"C:\Program Files\Claude\claude.exe"
MODEL = "haiku"
TIMEOUT = 80  # below the harness timeout of 90 s, so the wrapper cleans up its own child
DENY = "Bash,Edit,Write,Read,Glob,Grep,WebFetch,WebSearch,Agent,NotebookEdit,Task"


def main():
    req = json.load(sys.stdin)
    instruction = req.pop("instruction", "")
    prompt = instruction + "\n\n" + json.dumps(req)
    p = subprocess.run(
        [CLAUDE, "-p", "--model", MODEL, "--no-session-persistence", "--output-format", "text",
         "--disallowedTools", DENY, "--max-turns", "1"],
        input=prompt, capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=TIMEOUT,
    )
    if p.returncode != 0:
        sys.exit(2)
    text = p.stdout.strip()
    m = re.search(r"\{.*?\}", text, re.S)
    if not m:
        sys.exit(3)
    print(m.group(0))


if __name__ == "__main__":
    main()
