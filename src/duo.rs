use chrono::Local;
use clap::Parser;
use colored::*;
use serde::Deserialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::process::Command;

#[derive(Parser)]
#[command(name = "roundtable-chat", about = "Two Claude Code agents debate a topic")]
struct Cli {
    /// Discussion topic
    topic: String,

    /// Number of rounds (each round: B responds, A responds)
    #[arg(short, long, default_value_t = 3)]
    rounds: usize,

    /// Agent A role
    #[arg(long, default_value = "You are a pragmatic senior engineer. Be concise and direct. Listen to the other side, agree or disagree. Only state your own view, don't moderate.")]
    role_a: String,

    /// Agent B role
    #[arg(long, default_value = "You are a visionary software architect. Be concise and direct. Listen to the other side, agree or disagree. Only state your own view, don't moderate.")]
    role_b: String,

    /// Agent A name
    #[arg(long, default_value = "Pragmatist")]
    name_a: String,

    /// Agent B name
    #[arg(long, default_value = "Architect")]
    name_b: String,

    /// Model for both agents
    #[arg(short, long)]
    model: Option<String>,

    /// Max turns per agent call
    #[arg(long, default_value_t = 2)]
    max_turns: usize,

    /// Transcript file path (empty = auto)
    #[arg(long)]
    transcript: Option<String>,
}

#[derive(Deserialize)]
struct ClaudeResponse {
    result: Option<String>,
    session_id: Option<String>,
    #[serde(default)]
    is_error: bool,
}

struct Agent {
    name: String,
    role: String,
    session_id: Option<String>,
    color: fn(&str) -> ColoredString,
}

impl Agent {
    fn new(name: String, role: String, color: fn(&str) -> ColoredString) -> Self {
        Self { name, role, session_id: None, color }
    }
}

fn color_red(s: &str) -> ColoredString { s.red() }
fn color_blue(s: &str) -> ColoredString { s.blue() }

fn send_message(agent: &mut Agent, prompt: &str, max_turns: usize, model: Option<&str>) -> Result<String, String> {
    let mut cmd = Command::new("claude");
    cmd.args(["-p", prompt]);
    cmd.args(["--append-system-prompt", &agent.role]);
    cmd.args(["--output-format", "json"]);
    cmd.args(["--max-turns", &max_turns.to_string()]);

    if let Some(m) = model {
        cmd.args(["--model", m]);
    }

    if let Some(ref sid) = agent.session_id {
        cmd.args(["--resume", sid]);
    }

    let ts = Local::now().format("%H:%M:%S");
    eprintln!("{}", format!("[{ts}] {} thinking...", agent.name).dimmed());

    let output = cmd.output().map_err(|e| format!("failed to run claude: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("claude produced no output ({}): {stderr}", output.status));
    }

    let response = parse_claude_json(&stdout)?;

    if response.is_error {
        return Err(format!(
            "claude error: {}",
            response.result.as_deref().unwrap_or("unknown error")
        ));
    }

    if let Some(ref sid) = response.session_id {
        agent.session_id = Some(sid.clone());
    }

    response.result.ok_or_else(|| "empty response".to_string())
}

fn parse_claude_json(raw: &str) -> Result<ClaudeResponse, String> {
    // Output is JSON array: [{type:"system",...},{type:"result",...}]
    if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(raw) {
        for obj in &arr {
            if obj.get("type").and_then(|v| v.as_str()) == Some("result") {
                if let Ok(r) = serde_json::from_value::<ClaudeResponse>(obj.clone()) {
                    return Ok(r);
                }
            }
        }
        if let Some(last) = arr.last() {
            if let Ok(r) = serde_json::from_value::<ClaudeResponse>(last.clone()) {
                return Ok(r);
            }
        }
    }
    if let Ok(r) = serde_json::from_str::<ClaudeResponse>(raw) {
        return Ok(r);
    }
    Err(format!("JSON parse error: {}", &raw[..raw.len().min(300)]))
}

fn print_message(agent: &Agent, text: &str, round_label: &str) {
    let color = agent.color;
    let header = format!("|- {} -- {} -", agent.name, round_label);
    let footer = "|------------------------------------------------";
    println!("{}", color(&header));
    println!("{text}");
    println!("{}", color(footer));
    println!();
}

fn append_transcript(path: &str, agent_name: &str, round: usize, text: &str) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let entry = serde_json::json!({
            "timestamp": Local::now().to_rfc3339(),
            "agent": agent_name,
            "round": round,
            "message": text,
        });
        let _ = writeln!(f, "{entry}");
    }
}

fn main() {
    let cli = Cli::parse();
    let model = cli.model.as_deref();

    let transcript_path = cli.transcript.unwrap_or_else(|| {
        let dir = std::env::temp_dir().join("roundtable-chat");
        fs::create_dir_all(&dir).ok();
        let ts = Local::now().format("%Y%m%d-%H%M%S");
        dir.join(format!("{ts}.jsonl")).to_string_lossy().to_string()
    });

    let mut agent_a = Agent::new(cli.name_a, cli.role_a, color_red);
    let mut agent_b = Agent::new(cli.name_b, cli.role_b, color_blue);

    // Banner
    println!("{}", "=".repeat(55).green());
    println!("{}", format!(
        "  Roundtable Chat -- {} rounds, {} vs {}",
        cli.rounds, agent_a.name, agent_b.name
    ).green());
    println!("{}", format!("  Topic: {}", cli.topic).green());
    if let Some(m) = model {
        println!("{}", format!("  Model: {m}").green());
    }
    println!("{}", format!("  Transcript: {transcript_path}").green());
    println!("{}", "=".repeat(55).green());
    println!();

    // Opening: Agent A
    let opening_prompt = format!(
        "Discussion topic: {}\n\n\
         Explain your view on this topic in 2-3 paragraphs. Provide concrete arguments.",
        cli.topic
    );

    let msg_a = match send_message(&mut agent_a, &opening_prompt, cli.max_turns, model) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{}", format!("ERROR: {e}").red().bold());
            std::process::exit(1);
        }
    };

    print_message(&agent_a, &msg_a, "opening");
    append_transcript(&transcript_path, &agent_a.name, 0, &msg_a);

    let mut last_a = msg_a;

    for round in 1..=cli.rounds {
        let round_label = format!("round {round}/{}", cli.rounds);

        // Agent B responds
        let b_prompt = format!(
            "Discussion topic: {topic}\n\n\
             The other side ({a_name}) said:\n---\n{msg}\n---\n\n\
             Respond to this. State your agreements and disagreements. Keep it concise, 2-3 paragraphs.",
            topic = cli.topic, a_name = agent_a.name, msg = last_a,
        );

        let msg_b = match send_message(&mut agent_b, &b_prompt, cli.max_turns, model) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("{}", format!("ERROR (Agent B): {e}").red().bold());
                break;
            }
        };

        print_message(&agent_b, &msg_b, &round_label);
        append_transcript(&transcript_path, &agent_b.name, round, &msg_b);

        // Agent A responds
        let is_last = round == cli.rounds;
        let a_suffix = if is_last {
            "This is the final round. Summarize the common ground and remaining disagreements."
        } else {
            "Respond to this. State your agreements and disagreements. Keep it concise, 2-3 paragraphs."
        };

        let a_prompt = format!(
            "Discussion topic: {topic}\n\n\
             The other side ({b_name}) said:\n---\n{msg}\n---\n\n\
             {suffix}",
            topic = cli.topic, b_name = agent_b.name, msg = msg_b, suffix = a_suffix,
        );

        let msg_a = match send_message(&mut agent_a, &a_prompt, cli.max_turns, model) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("{}", format!("ERROR (Agent A): {e}").red().bold());
                break;
            }
        };

        print_message(&agent_a, &msg_a, &round_label);
        append_transcript(&transcript_path, &agent_a.name, round, &msg_a);

        last_a = msg_a;
    }

    println!("{}", "=".repeat(55).green());
    println!("{}", format!("  Debate complete. Transcript: {transcript_path}").green());
    println!("{}", "=".repeat(55).green());
}
