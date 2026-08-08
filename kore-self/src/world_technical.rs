//! Technical world knowledge — programming languages, shells, Linux/Unix, DevOps.

use crate::world_types::WorldAnswer;

#[derive(Debug, Clone, Copy)]
pub struct ProgrammingLanguage {
    pub name: &'static str,
    pub category: &'static str,
    pub paradigm: &'static str,
    pub year: u16,
    pub wiki_slug: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct ShellMeta {
    pub name: &'static str,
    pub platform: &'static str,
    pub wiki_slug: &'static str,
}

/// Major programming languages KORE-self indexes (human + technical stack).
pub const PROGRAMMING_LANGUAGES: &[ProgrammingLanguage] = &[
    ProgrammingLanguage { name: "Rust", category: "systems", paradigm: "multi-paradigm", year: 2010, wiki_slug: "Rust_(programming_language)" },
    ProgrammingLanguage { name: "Python", category: "general", paradigm: "multi-paradigm", year: 1991, wiki_slug: "Python_(programming_language)" },
    ProgrammingLanguage { name: "JavaScript", category: "web", paradigm: "multi-paradigm", year: 1995, wiki_slug: "JavaScript" },
    ProgrammingLanguage { name: "TypeScript", category: "web", paradigm: "multi-paradigm", year: 2012, wiki_slug: "TypeScript" },
    ProgrammingLanguage { name: "Go", category: "systems", paradigm: "imperative", year: 2009, wiki_slug: "Go_(programming_language)" },
    ProgrammingLanguage { name: "Java", category: "enterprise", paradigm: "OOP", year: 1995, wiki_slug: "Java_(programming_language)" },
    ProgrammingLanguage { name: "C", category: "systems", paradigm: "imperative", year: 1972, wiki_slug: "C_(programming_language)" },
    ProgrammingLanguage { name: "C++", category: "systems", paradigm: "multi-paradigm", year: 1985, wiki_slug: "C++" },
    ProgrammingLanguage { name: "C#", category: "enterprise", paradigm: "multi-paradigm", year: 2000, wiki_slug: "C_Sharp_(programming_language)" },
    ProgrammingLanguage { name: "F#", category: "functional", paradigm: "functional", year: 2005, wiki_slug: "F_Sharp_(programming_language)" },
    ProgrammingLanguage { name: "Ruby", category: "general", paradigm: "OOP", year: 1995, wiki_slug: "Ruby_(programming_language)" },
    ProgrammingLanguage { name: "PHP", category: "web", paradigm: "imperative", year: 1995, wiki_slug: "PHP" },
    ProgrammingLanguage { name: "Swift", category: "mobile", paradigm: "multi-paradigm", year: 2014, wiki_slug: "Swift_(programming_language)" },
    ProgrammingLanguage { name: "Kotlin", category: "mobile", paradigm: "multi-paradigm", year: 2011, wiki_slug: "Kotlin_(programming_language)" },
    ProgrammingLanguage { name: "Scala", category: "JVM", paradigm: "functional/OOP", year: 2004, wiki_slug: "Scala_(programming_language)" },
    ProgrammingLanguage { name: "Haskell", category: "functional", paradigm: "pure functional", year: 1990, wiki_slug: "Haskell" },
    ProgrammingLanguage { name: "Erlang", category: "concurrent", paradigm: "functional", year: 1986, wiki_slug: "Erlang_(programming_language)" },
    ProgrammingLanguage { name: "Elixir", category: "concurrent", paradigm: "functional", year: 2011, wiki_slug: "Elixir_(programming_language)" },
    ProgrammingLanguage { name: "Lua", category: "embedded", paradigm: "multi-paradigm", year: 1993, wiki_slug: "Lua" },
    ProgrammingLanguage { name: "R", category: "data", paradigm: "multi-paradigm", year: 1993, wiki_slug: "R_(programming_language)" },
    ProgrammingLanguage { name: "Julia", category: "data", paradigm: "multi-paradigm", year: 2012, wiki_slug: "Julia_(programming_language)" },
    ProgrammingLanguage { name: "MATLAB", category: "data", paradigm: "matrix", year: 1984, wiki_slug: "MATLAB" },
    ProgrammingLanguage { name: "Perl", category: "scripting", paradigm: "multi-paradigm", year: 1987, wiki_slug: "Perl" },
    ProgrammingLanguage { name: "Objective-C", category: "mobile", paradigm: "OOP", year: 1984, wiki_slug: "Objective-C" },
    ProgrammingLanguage { name: "Dart", category: "web/mobile", paradigm: "OOP", year: 2011, wiki_slug: "Dart_(programming_language)" },
    ProgrammingLanguage { name: "Zig", category: "systems", paradigm: "imperative", year: 2016, wiki_slug: "Zig_(programming_language)" },
    ProgrammingLanguage { name: "Nim", category: "systems", paradigm: "multi-paradigm", year: 2008, wiki_slug: "Nim_(programming_language)" },
    ProgrammingLanguage { name: "Crystal", category: "systems", paradigm: "OOP", year: 2014, wiki_slug: "Crystal_(programming_language)" },
    ProgrammingLanguage { name: "Clojure", category: "JVM", paradigm: "functional", year: 2007, wiki_slug: "Clojure" },
    ProgrammingLanguage { name: "Groovy", category: "JVM", paradigm: "OOP", year: 2003, wiki_slug: "Groovy_(programming_language)" },
    ProgrammingLanguage { name: "Fortran", category: "scientific", paradigm: "imperative", year: 1957, wiki_slug: "Fortran" },
    ProgrammingLanguage { name: "COBOL", category: "enterprise", paradigm: "imperative", year: 1959, wiki_slug: "COBOL" },
    ProgrammingLanguage { name: "Ada", category: "systems", paradigm: "imperative", year: 1980, wiki_slug: "Ada_(programming_language)" },
    ProgrammingLanguage { name: "Assembly", category: "systems", paradigm: "low-level", year: 1947, wiki_slug: "Assembly_language" },
    ProgrammingLanguage { name: "SQL", category: "data", paradigm: "declarative", year: 1974, wiki_slug: "SQL" },
    ProgrammingLanguage { name: "Solidity", category: "blockchain", paradigm: "imperative", year: 2014, wiki_slug: "Solidity" },
    ProgrammingLanguage { name: "Prolog", category: "logic", paradigm: "logic", year: 1972, wiki_slug: "Prolog" },
    ProgrammingLanguage { name: "Lisp", category: "functional", paradigm: "functional", year: 1958, wiki_slug: "Lisp_(programming_language)" },
    ProgrammingLanguage { name: "Scheme", category: "functional", paradigm: "functional", year: 1975, wiki_slug: "Scheme_(programming_language)" },
    ProgrammingLanguage { name: "OCaml", category: "functional", paradigm: "functional", year: 1996, wiki_slug: "OCaml" },
    ProgrammingLanguage { name: "Pascal", category: "general", paradigm: "imperative", year: 1970, wiki_slug: "Pascal_(programming_language)" },
    ProgrammingLanguage { name: "Visual Basic", category: "general", paradigm: "OOP", year: 1991, wiki_slug: "Visual_Basic" },
    ProgrammingLanguage { name: "GraphQL", category: "web", paradigm: "declarative", year: 2015, wiki_slug: "GraphQL" },
    ProgrammingLanguage { name: "HTML", category: "markup", paradigm: "declarative", year: 1993, wiki_slug: "HTML" },
    ProgrammingLanguage { name: "CSS", category: "markup", paradigm: "declarative", year: 1996, wiki_slug: "CSS" },
    ProgrammingLanguage { name: "YAML", category: "config", paradigm: "data", year: 2001, wiki_slug: "YAML" },
    ProgrammingLanguage { name: "JSON", category: "config", paradigm: "data", year: 2001, wiki_slug: "JSON" },
    ProgrammingLanguage { name: "TOML", category: "config", paradigm: "data", year: 2013, wiki_slug: "TOML" },
    ProgrammingLanguage { name: "XML", category: "markup", paradigm: "declarative", year: 1998, wiki_slug: "XML" },
    ProgrammingLanguage { name: "Markdown", category: "markup", paradigm: "declarative", year: 2004, wiki_slug: "Markdown" },
    ProgrammingLanguage { name: "V", category: "systems", paradigm: "imperative", year: 2019, wiki_slug: "V_(programming_language)" },
    ProgrammingLanguage { name: "Odin", category: "systems", paradigm: "imperative", year: 2016, wiki_slug: "Odin_(programming_language)" },
    ProgrammingLanguage { name: "WebAssembly", category: "systems", paradigm: "stack machine", year: 2015, wiki_slug: "WebAssembly" },
];

pub const SHELLS: &[ShellMeta] = &[
    ShellMeta { name: "Bash", platform: "Linux/macOS/WSL", wiki_slug: "Bash_(Unix_shell)" },
    ShellMeta { name: "sh", platform: "POSIX Unix", wiki_slug: "Unix_shell" },
    ShellMeta { name: "Zsh", platform: "macOS/Linux", wiki_slug: "Z_shell" },
    ShellMeta { name: "Fish", platform: "Linux/macOS", wiki_slug: "Friendly_interactive_shell" },
    ShellMeta { name: "PowerShell", platform: "Windows/cross", wiki_slug: "PowerShell" },
    ShellMeta { name: "cmd", platform: "Windows", wiki_slug: "Cmd.exe" },
    ShellMeta { name: "Tcsh", platform: "BSD/Unix", wiki_slug: "Tcsh" },
    ShellMeta { name: "Ksh", platform: "Unix", wiki_slug: "Korn_shell" },
];

/// Wikipedia topics for technical gap-fill (heartbeat / self_fill_self).
pub const PRIORITY_TECH_TOPICS: &[(&str, &str)] = &[
    ("Rust_(programming_language)", "Rust"),
    ("Python_(programming_language)", "Python"),
    ("Bash_(Unix_shell)", "Bash"),
    ("Linux", "Linux"),
    ("Unix", "Unix"),
    ("Shell_script", "Shell script"),
    ("GNU", "GNU"),
    ("Git", "Git"),
    ("Docker_(software)", "Docker"),
    ("Kubernetes", "Kubernetes"),
    ("JavaScript", "JavaScript"),
    ("Go_(programming_language)", "Go"),
    ("C_(programming_language)", "C"),
    ("C++", "C++"),
    ("Java_(programming_language)", "Java"),
    ("SQL", "SQL"),
    ("DevOps", "DevOps"),
    ("Operating_system", "Operating system"),
    ("Command-line_interface", "Command line"),
    ("Systemd", "systemd"),
];

/// Common Linux commands KORE can explain inline.
pub const LINUX_COMMANDS: &[(&str, &str)] = &[
    ("ls", "List directory contents. ls -la shows all files with permissions and sizes."),
    ("cd", "Change directory. cd .. goes up one level; cd ~ goes home."),
    ("pwd", "Print working directory — shows your current path."),
    ("cp", "Copy files/directories. cp -r for recursive copy."),
    ("mv", "Move or rename files/directories."),
    ("rm", "Remove files. rm -rf removes directories recursively (dangerous)."),
    ("mkdir", "Create directory. mkdir -p creates parent paths too."),
    ("chmod", "Change file permissions. chmod +x file makes it executable; chmod 755 dir sets rwxr-xr-x."),
    ("chown", "Change file owner/group. Requires root/sudo."),
    ("grep", "Search text patterns. grep -r pattern . searches recursively."),
    ("find", "Find files. find . -name '*.rs' finds Rust files from current dir."),
    ("sed", "Stream editor — substitute/delete text in files or pipes."),
    ("awk", "Pattern scanning and text processing language for columns/fields."),
    ("cat", "Concatenate and print file contents to stdout."),
    ("head", "Print first N lines (default 10). head -n 20 file"),
    ("tail", "Print last N lines. tail -f follows log files live."),
    ("less", "Pager for long output — scroll with arrows, q to quit."),
    ("man", "Manual pages. man grep shows grep documentation."),
    ("sudo", "Run command as superuser (admin). Use carefully."),
    ("apt", "Debian/Ubuntu package manager. apt install pkg, apt update."),
    ("yum", "RHEL/CentOS package manager (legacy). dnf is the modern replacement."),
    ("dnf", "Fedora/RHEL package manager. dnf install pkg."),
    ("pacman", "Arch Linux package manager. pacman -S pkg."),
    ("systemctl", "Control systemd services. systemctl start/stop/status nginx."),
    ("journalctl", "View systemd logs. journalctl -u nginx -f follows service logs."),
    ("ssh", "Secure shell remote login. ssh user@host -p 22."),
    ("scp", "Secure copy over SSH. scp file user@host:/path/."),
    ("curl", "Transfer data from URLs. curl -O downloads a file."),
    ("wget", "Download files from the web."),
    ("tar", "Archive files. tar -xzf file.tar.gz extracts gzip tarball."),
    ("gzip", "Compress/decompress with gzip. gzip file → file.gz."),
    ("ps", "List running processes. ps aux shows all processes."),
    ("top", "Interactive process monitor (CPU/memory). htop is friendlier."),
    ("kill", "Send signal to process. kill -9 PID force-kills."),
    ("df", "Disk free space. df -h human-readable."),
    ("du", "Disk usage. du -sh * sizes each item in directory."),
    ("ln", "Create links. ln -s target linkname for symbolic link."),
    ("which", "Show path to executable. which bash."),
    ("env", "Print environment variables or run command with modified env."),
    ("export", "Set shell environment variable. export PATH=$PATH:/new/path."),
    ("source", "Execute script in current shell. source ~/.bashrc reloads bash config."),
    ("pipe", "Redirect stdout to another command. cmd1 | cmd2."),
];

pub fn catalog_summary() -> String {
    format!(
        "KORE technical catalog: {} programming languages, {} shells, {} Linux commands indexed.\n\
         Categories: systems, web, functional, data, mobile, enterprise, markup, config.\n\
         Use self_world_catalog action=programming | shells | linux | technical.",
        PROGRAMMING_LANGUAGES.len(),
        SHELLS.len(),
        LINUX_COMMANDS.len()
    )
}

pub fn full_programming_list() -> String {
    let mut out = format!(
        "Programming languages in KORE-self ({} indexed):\n\n",
        PROGRAMMING_LANGUAGES.len()
    );
    let mut last_cat = "";
    for lang in PROGRAMMING_LANGUAGES {
        if lang.category != last_cat {
            out.push_str(&format!("\n[{}]\n", lang.category));
            last_cat = lang.category;
        }
        out.push_str(&format!(
            "  • {} — {} (since {})\n",
            lang.name, lang.paradigm, lang.year
        ));
    }
    out.push_str("\nDeep articles: self_fetch source=wikipedia topic=Rust_(programming_language)");
    out
}

pub fn full_shell_list() -> String {
    let mut out = String::from("Shells & scripting environments:\n\n");
    for s in SHELLS {
        out.push_str(&format!("  • {} — {}\n", s.name, s.platform));
    }
    out.push_str(
        "\nBash basics: shebang #!/bin/bash, variables $VAR, $(command), pipes |, redirects > >> <, \
         if/for/while, functions, exit codes ($?).\n\
         Linux default shell on most distros: Bash (sh often links to dash or bash).",
    );
    out
}

pub fn full_linux_catalog() -> String {
    let mut out = String::from("Linux / Unix command reference (built-in):\n\n");
    for (cmd, desc) in LINUX_COMMANDS {
        out.push_str(&format!("  {cmd:12} — {desc}\n"));
    }
    out.push_str(
        "\nLinux FHS: /bin (binaries), /etc (config), /home (users), /var (logs/data), \
         /tmp (temp), /usr (user programs), /opt (optional apps).\n\
         Permissions: rwx for owner/group/others (e.g. 755 = rwxr-xr-x).",
    );
    out
}

pub fn full_technical_overview() -> String {
    format!(
        "{}\n\n{}\n\n{}",
        catalog_summary(),
        full_shell_list(),
        full_linux_catalog()
    )
}

pub fn find_language(name: &str) -> Option<&'static ProgrammingLanguage> {
    let lower = name.to_lowercase();
    PROGRAMMING_LANGUAGES.iter().find(|l| {
        l.name.to_lowercase() == lower
            || l.wiki_slug.to_lowercase().contains(&lower)
            || lower.contains(&l.name.to_lowercase())
    })
}

pub fn try_technical(problem: &str, lower: &str, steps: &mut Vec<String>) -> Option<WorldAnswer> {
    if let Some(a) = try_programming_catalog(lower, steps) {
        return Some(a);
    }
    if let Some(a) = try_language_lookup(lower, steps) {
        return Some(a);
    }
    if let Some(a) = try_shell_query(lower, steps) {
        return Some(a);
    }
    if let Some(a) = try_linux_query(lower, steps) {
        return Some(a);
    }
    if let Some(a) = try_devops_query(lower, steps) {
        return Some(a);
    }
    if let Some(a) = try_technical_classify(problem, lower, steps) {
        return Some(a);
    }
    None
}

fn try_programming_catalog(lower: &str, steps: &mut Vec<String>) -> Option<WorldAnswer> {
    let wants_list = (lower.contains("programming") || lower.contains("coding"))
        && (lower.contains("language") || lower.contains("languages"))
        && (lower.contains("list") || lower.contains("how many") || lower.contains("all"));
    if wants_list {
        steps.push("Technical: programming language catalog.".into());
        return Some(WorldAnswer::new(
            "tech_programming_list",
            full_programming_list(),
            0.96,
        ));
    }
    if lower.contains("how many") && lower.contains("programming") {
        return Some(WorldAnswer::new(
            "tech_programming_count",
            format!(
                "KORE indexes {} major programming languages (systems, web, functional, data, mobile, …).\n\
                 ~700+ languages exist historically; KORE focuses on practical + systems stack.\n\n{}",
                PROGRAMMING_LANGUAGES.len(),
                catalog_summary()
            ),
            0.94,
        ));
    }
    None
}

fn try_language_lookup(lower: &str, steps: &mut Vec<String>) -> Option<WorldAnswer> {
    let lookup = lower.contains("what is")
        || lower.contains("what's")
        || lower.contains("tell me about")
        || lower.contains("explain");
    if !lookup {
        return None;
    }
    for lang in PROGRAMMING_LANGUAGES {
        let name_l = lang.name.to_lowercase();
        if !language_name_in_text(&name_l, lower) {
            continue;
        }
        steps.push(format!("Technical: {} language lookup.", lang.name));
        return Some(WorldAnswer::new(
            "tech_language_info",
            format!(
                "{} — category: {}, paradigm: {}, first appeared ~{}.\n\
                 Wikipedia: self_fetch source=wikipedia topic={}\n\
                 KORE-self itself is written in Rust.",
                lang.name, lang.category, lang.paradigm, lang.year, lang.wiki_slug
            ),
            0.88,
        ));
    }
    None
}

fn language_name_in_text(name: &str, lower: &str) -> bool {
    if name == "c++" || name == "c#" {
        return lower.contains(name) || lower.contains(&name.replace('+', "plus"));
    }
    let tokens: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric() && c != '+' && c != '#')
        .filter(|t| !t.is_empty())
        .collect();
    if name.len() <= 2 {
        return tokens.iter().any(|t| t.eq_ignore_ascii_case(name));
    }
    lower.contains(name) || tokens.iter().any(|t| t.eq_ignore_ascii_case(name))
}

fn try_shell_query(lower: &str, steps: &mut Vec<String>) -> Option<WorldAnswer> {
    if lower.contains("bash") && (lower.contains(" vs ") || lower.contains(" versus ") || lower.contains("difference")) {
        if lower.contains("sh") || lower.contains("shell") {
            steps.push("Bash vs sh comparison.".into());
            return Some(WorldAnswer::new(
                "tech_bash_vs_sh",
                "Bash (Bourne Again Shell): superset of POSIX sh — arrays, [[ ]], brace expansion, \
                 process substitution, better scripting. Default on most Linux distros.\n\
                 sh: POSIX minimal shell — portable, faster startup; dash on Debian/Ubuntu.\n\
                 Shebang: #!/bin/bash (bash-specific) vs #!/bin/sh (portable).",
                0.91,
            ));
        }
    }
    if (lower.contains("list") || lower.contains("which")) && lower.contains("shell") {
        steps.push("Shell catalog.".into());
        return Some(WorldAnswer::new("tech_shell_list", full_shell_list(), 0.95));
    }
    for shell in SHELLS {
        let shell_l = shell.name.to_lowercase();
        if !language_name_in_text(&shell_l, lower) {
            continue;
        }
        if lower.contains("what is") || lower.contains("explain") {
            steps.push(format!("Shell: {}", shell.name));
                return Some(WorldAnswer::new(
                    "tech_shell_info",
                    format!(
                        "{} — platform: {}.\n\
                         Wikipedia: self_fetch source=wikipedia topic={}\n\n{}",
                        shell.name,
                        shell.platform,
                        shell.wiki_slug,
                        if shell.name == "Bash" {
                            "Example: #!/bin/bash\nfor f in *.txt; do echo \"$f\"; done"
                        } else {
                            ""
                        },
                    ),
                    0.87,
                ));
        }
    }
    if lower.contains("shebang") || lower.contains("#!/bin") {
        return Some(WorldAnswer::new(
            "tech_shebang",
            "Shebang: first line #!/path/to/interpreter tells OS which program runs the script.\n\
             Examples: #!/bin/bash, #!/usr/bin/env python3, #!/bin/sh.\n\
             Make executable: chmod +x script.sh then ./script.sh",
            0.92,
        ));
    }
    None
}

fn try_linux_query(lower: &str, steps: &mut Vec<String>) -> Option<WorldAnswer> {
    if lower.contains("linux") && (lower.contains("what is") || lower.contains("explain")) {
        steps.push("Linux overview.".into());
        return Some(WorldAnswer::new(
            "tech_linux_overview",
            "Linux: open-source Unix-like kernel (Linus Torvalds, 1991). Distros bundle kernel + GNU tools + package manager.\n\
             Major distros: Ubuntu, Debian, Fedora, Arch, CentOS/RHEL, openSUSE, Alpine.\n\
             KORE runs on Linux, macOS, and Windows (WSL/PowerShell).\n\
             Deep: self_fetch source=wikipedia topic=Linux",
            0.9,
        ));
    }
    if lower.contains("file permission") || lower.contains("chmod") || lower.contains("rwx") {
        return Some(WorldAnswer::new(
            "tech_linux_permissions",
            "Linux permissions: owner / group / others × read(r=4) write(w=2) execute(x=1).\n\
             chmod 755 file → rwxr-xr-x. chmod +x script.sh adds execute.\n\
             ls -l shows permissions. chown user:group file changes owner.",
            0.93,
        ));
    }
    if lower.contains("systemd") {
        return Some(WorldAnswer::new(
            "tech_systemd",
            "systemd: init system + service manager on most Linux distros.\n\
             systemctl start|stop|restart|status SERVICE\n\
             systemctl enable SERVICE — start on boot\n\
             journalctl -u SERVICE -f — follow logs",
            0.91,
        ));
    }
    if lower.contains("linux command") || lower.contains("unix command") {
        steps.push("Linux command catalog.".into());
        return Some(WorldAnswer::new("tech_linux_commands", full_linux_catalog(), 0.94));
    }
    for (cmd, desc) in LINUX_COMMANDS {
        if lower.contains(&format!("what is {cmd}"))
            || lower.contains(&format!("explain {cmd}"))
            || lower.contains(&format!("{cmd} command"))
        {
            steps.push(format!("Linux command: {cmd}"));
            return Some(WorldAnswer::new(
                "tech_linux_cmd",
                format!("{cmd}: {desc}"),
                0.9,
            ));
        }
    }
    None
}

fn try_devops_query(lower: &str, steps: &mut Vec<String>) -> Option<WorldAnswer> {
    if lower.contains("git") && (lower.contains("what is") || lower.contains("explain")) {
        return Some(WorldAnswer::new(
            "tech_git",
            "Git: distributed version control (Linus Torvalds, 2005).\n\
             git init, clone, add, commit, push, pull, branch, merge, rebase, status, log.\n\
             Remote hosting: GitHub, GitLab, Bitbucket.",
            0.9,
        ));
    }
    if lower.contains("docker") && (lower.contains("what is") || lower.contains("explain")) {
        return Some(WorldAnswer::new(
            "tech_docker",
            "Docker: container platform — packages app + dependencies in isolated containers.\n\
             docker build, run, ps, exec, compose. Images from Dockerfile; orchestration often Kubernetes.",
            0.88,
        ));
    }
    if lower.contains("kubernetes") || lower.contains("k8s") {
        return Some(WorldAnswer::new(
            "tech_kubernetes",
            "Kubernetes (K8s): container orchestration — deploy, scale, heal pods across nodes.\n\
             Concepts: Pod, Deployment, Service, Ingress, Namespace, kubectl.",
            0.87,
        ));
    }
    None
}

fn try_technical_classify(
    problem: &str,
    lower: &str,
    steps: &mut Vec<String>,
) -> Option<WorldAnswer> {
    let tech_keywords = [
        "programming", "coding", "developer", "software", "linux", "unix", "bash",
        "shell", "script", "terminal", "cli", "devops", "rust", "python", "javascript",
        "compiler", "interpreter", "algorithm", "database", "api", "server",
    ];
    if !tech_keywords.iter().any(|k| lower.contains(k)) {
        return None;
    }
    if let Some(lang) = PROGRAMMING_LANGUAGES.iter().find(|l| {
        language_name_in_text(&l.name.to_lowercase(), lower)
    }) {
        steps.push(format!("Matched programming language: {}", lang.name));
        return Some(WorldAnswer::new(
            "tech_language_match",
            format!(
                "Detected programming language: {} ({} / {}).\n\
                 Full catalog: self_world_catalog action=programming\n\
                 Wikipedia depth: self_fetch source=wikipedia topic={}",
                lang.name, lang.category, lang.paradigm, lang.wiki_slug
            ),
            0.75,
        ));
    }
    if lower.contains("linux") || lower.contains("bash") || lower.contains("shell") {
        steps.push("Technical domain: Linux/shell.".into());
        return Some(WorldAnswer::new(
            "tech_linux_pointer",
            format!(
                "Technical query detected: \"{}\"\n\n{}\n\n\
                 Try: self_world_catalog action=linux | shells | programming",
                truncate(problem, 80),
                catalog_summary()
            ),
            0.65,
        ));
    }
    None
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    format!("{}…", &s[..max])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_major_languages() {
        assert!(find_language("rust").is_some());
        assert!(find_language("python").is_some());
        assert!(PROGRAMMING_LANGUAGES.len() >= 50);
    }

    #[test]
    fn solves_programming_list_query() {
        let mut steps = vec![];
        let r = try_technical(
            "list all programming languages",
            "list all programming languages",
            &mut steps,
        )
        .unwrap();
        assert!(r.answer.contains("Rust"));
        assert!(r.confidence > 0.9);
    }

    #[test]
    fn solves_bash_vs_sh() {
        let mut steps = vec![];
        let r = try_technical(
            "bash vs sh difference",
            "bash vs sh difference",
            &mut steps,
        )
        .unwrap();
        assert!(r.answer.contains("POSIX"));
    }

    #[test]
    fn linux_chmod_answer() {
        let mut steps = vec![];
        let r = try_technical(
            "explain chmod file permissions",
            "explain chmod file permissions",
            &mut steps,
        )
        .unwrap();
        assert!(r.answer.contains("755"));
    }
}
