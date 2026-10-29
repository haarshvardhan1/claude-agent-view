use anyhow::{bail, Result};
use av_core::{scan, Agent, AgentStatus, ScanResult, SystemProbe};
use av_core::subagents::SubagentStatus;
use chrono::Utc;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "av", about = "Agent Viewer: inspect Claude Code sessions")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print a table (or JSON) of agents
    List {
        #[arg(long)]
        json: bool,
        /// Include exited agents
        #[arg(long)]
        all: bool,
        #[arg(long)]
        claude_dir: Option<PathBuf>,
    },
    /// Live view, refreshed every second
    Watch {
        #[arg(long)]
        all: bool,
        #[arg(long)]
        claude_dir: Option<PathBuf>,
    },
}

fn resolve_dir(arg: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(p) = arg {
        return Ok(p);
    }
    match std::env::var_os("HOME") {
        Some(h) if !h.is_empty() => Ok(PathBuf::from(h).join(".claude")),
        _ => bail!("HOME is not set; pass --claude-dir <path>"),
    }
}

fn do_scan(dir: &std::path::Path) -> ScanResult {
    scan(dir, &SystemProbe, Utc::now())
}

fn fmt_tokens(n: u64) -> String {
    if n < 1000 {
        n.to_string()
    } else if n < 1_000_000 {
        format!("{}k", n / 1000)
    } else {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    }
}

fn fmt_age(started_ms: i64, now_ms: i64) -> String {
    let s = ((now_ms - started_ms) / 1000).max(0);
    match s {
        0..=59 => format!("{s}s"),
        60..=3599 => format!("{}m", s / 60),
        3600..=86399 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86400),
    }
}

/// Replace every control character (escape sequences, CR, tabs, newlines...)
/// with a space so untrusted text cannot drive the terminal.
fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

fn truncate(s: &str, max: usize) -> String {
    let s = sanitize(s);
    if s.chars().count() <= max {
        s
    } else {
        let mut t: String = s.chars().take(max - 1).collect();
        t.push('…');
        t
    }
}

fn sub_label(s: SubagentStatus) -> &'static str {
    match s {
        SubagentStatus::Running => "running",
        SubagentStatus::Done => "done",
        SubagentStatus::Stopped => "stopped",
    }
}

fn render(result: &ScanResult, all: bool, now_ms: i64) -> String {
    let agents: Vec<&Agent> = result
        .agents
        .iter()
        .filter(|a| all || a.status != AgentStatus::Exited)
        .collect();
    let header = ["STATUS", "NAME", "TASK", "PROJECT", "BRANCH", "KIND", "AGE", "TOKENS"];
    let rows: Vec<[String; 8]> = agents
        .iter()
        .map(|a| {
            let project = a.entry.cwd.trim_end_matches('/').rsplit('/').next().unwrap_or("");
            [
                a.status.label().to_string(),
                sanitize(&a.entry.name),
                truncate(&a.title, 50),
                sanitize(project),
                sanitize(a.summary.git_branch.as_deref().unwrap_or("-")),
                sanitize(&a.entry.kind),
                fmt_age(a.entry.started_at_ms, now_ms),
                fmt_tokens(a.summary.tokens.total()),
            ]
        })
        .collect();
    let mut widths: Vec<usize> = header.iter().map(|h| h.chars().count()).collect();
    for r in &rows {
        for (i, c) in r.iter().enumerate() {
            widths[i] = widths[i].max(c.chars().count());
        }
    }
    let line = |cells: &[String]| -> String {
        let mut out = String::new();
        for (i, c) in cells.iter().enumerate() {
            if i + 1 == cells.len() {
                out.push_str(c);
            } else {
                out.push_str(&format!("{:<w$}  ", c, w = widths[i]));
            }
        }
        out.trim_end().to_string() + "\n"
    };
    let mut out = line(&header.map(String::from));
    for (r, a) in rows.iter().zip(&agents) {
        out.push_str(&line(r));
        for s in &a.subagents {
            out.push_str(&format!(
                "  └ {} {}: {}\n",
                sub_label(s.status),
                sanitize(&s.agent_type),
                truncate(&s.description, 80)
            ));
        }
    }
    out
}

fn print_warnings(r: &ScanResult) {
    for w in &r.warnings {
        eprintln!("warning: {w}");
    }
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::List { json, all, claude_dir } => {
            let dir = resolve_dir(claude_dir)?;
            let mut result = do_scan(&dir);
            print_warnings(&result);
            if json {
                if !all {
                    result.agents.retain(|a| a.status != AgentStatus::Exited);
                }
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                print!("{}", render(&result, all, Utc::now().timestamp_millis()));
            }
        }
        Cmd::Watch { all, claude_dir } => {
            let dir = resolve_dir(claude_dir)?;
            loop {
                let result = do_scan(&dir);
                print!("\x1b[2J\x1b[H");
                print!("{}", render(&result, all, Utc::now().timestamp_millis()));
                print_warnings(&result);
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }
    Ok(())
}
