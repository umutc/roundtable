use chrono::Local;
use clap::Parser;
use colored::*;
use serde::Deserialize;
use std::fmt::Write as FmtWrite;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::process::Command;

// ── Built-in configs ──

const BUILTIN_DEBATE: &str = include_str!("../examples/debate.toml");
const BUILTIN_CODE_REVIEW: &str = include_str!("../examples/code-review.toml");
const BUILTIN_MICROSERVICES: &str = include_str!("../examples/microservices.toml");

const DEFAULT_MODERATOR_ROLE: &str = "\
You are an experienced moderator. Your job:
1. Analyze the agents' arguments
2. Identify missing perspectives and redirect to the relevant agent
3. Identify and deepen unresolved conflicts
4. When convergence is reached, synthesize

IMPORTANT: End each turn with a JSON decision on the last line:
{\"action\": \"continue\", \"guidance\": \"Specific guidance here\"}
or
{\"action\": \"synthesize\"}";

// ── CLI ──

#[derive(Parser)]
#[command(name = "roundtable", about = "Multi-agent debate panel with moderator-driven convergence")]
struct Cli {
    /// TOML config file or built-in name (debate, code-review, microservices)
    config: Option<String>,

    /// Override max_rounds from config
    #[arg(short, long)]
    rounds: Option<usize>,

    /// Override model for all agents
    #[arg(short, long)]
    model: Option<String>,

    /// Override moderator model
    #[arg(long)]
    moderator_model: Option<String>,

    /// Fallback model (on overload)
    #[arg(long)]
    fallback_model: Option<String>,

    /// Max budget (USD) — total spending limit
    #[arg(long)]
    max_budget_usd: Option<f64>,

    /// Permission mode: default, acceptEdits, plan, auto, bypassPermissions
    #[arg(long)]
    permission_mode: Option<String>,

    /// Effort level: low, medium, high, max (Opus 4.6 only)
    #[arg(long)]
    effort: Option<String>,

    /// Transcript file path (empty = auto)
    #[arg(long)]
    transcript: Option<String>,

    /// Topic — override config topic (required in zero-config mode)
    #[arg(long)]
    topic: Option<String>,

    /// Show claude stderr output
    #[arg(short, long)]
    verbose: bool,

    // ── Project context ──

    /// Working directory for child processes (default: CWD)
    #[arg(long)]
    cwd: Option<String>,

    /// Additional working directories (claude --add-dir)
    #[arg(long)]
    add_dir: Vec<String>,

    /// Project context file — contents injected into every prompt
    #[arg(long)]
    context_file: Vec<String>,

    /// MCP config JSON file (claude --mcp-config)
    #[arg(long)]
    mcp_config: Option<String>,

    /// JSON-only output for parent session integration
    #[arg(long)]
    output_json: bool,
}

// ── Config ──

#[derive(Deserialize)]
struct Config {
    session: SessionConfig,
    moderator: ModeratorConfig,
    agents: Vec<AgentConfig>,
    #[serde(default)]
    project: ProjectConfig,
}

#[derive(Deserialize, Default)]
struct ProjectConfig {
    #[serde(default)]
    context_files: Vec<String>,
    #[serde(default)]
    add_dirs: Vec<String>,
    #[serde(default)]
    mcp_config: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Deserialize)]
struct SessionConfig {
    topic: String,
    #[serde(default = "default_max_rounds")]
    max_rounds: usize,
    #[serde(default = "default_model")]
    model: String,
    #[serde(default = "default_moderator_model")]
    moderator_model: String,
    #[serde(default)]
    fallback_model: Option<String>,
    #[serde(default)]
    max_budget_usd: Option<f64>,
    #[serde(default)]
    permission_mode: Option<String>,
    #[serde(default)]
    effort: Option<String>,
    #[serde(default = "default_max_turns")]
    max_turns: usize,
    #[serde(default)]
    extra_flags: Vec<String>,
}

fn default_max_rounds() -> usize { 10 }
fn default_model() -> String { "sonnet".into() }
fn default_moderator_model() -> String { "opus".into() }
fn default_max_turns() -> usize { 2 }

#[derive(Deserialize)]
struct ModeratorConfig {
    role: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    effort: Option<String>,
}

#[derive(Deserialize, Clone)]
struct AgentConfig {
    name: String,
    role: String,
    #[serde(default = "default_color")]
    color: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    effort: Option<String>,
    #[serde(default)]
    max_turns: Option<usize>,
    #[serde(default)]
    disallowed_tools: Vec<String>,
    #[serde(default)]
    allowed_tools: Vec<String>,
}

fn default_color() -> String { "white".into() }

// ── Claude call params ──

struct ClaudeCallParams<'a> {
    prompt: &'a str,
    system_prompt: &'a str,
    model: &'a str,
    session_id: Option<&'a str>,
    max_turns: usize,
    effort: Option<&'a str>,
    fallback_model: Option<&'a str>,
    permission_mode: Option<&'a str>,
    allowed_tools: &'a [String],
    disallowed_tools: &'a [String],
    extra_flags: &'a [String],
    verbose: bool,
    cwd: Option<&'a str>,
    add_dirs: &'a [String],
    mcp_config: Option<&'a str>,
}

// ── Claude response ──

#[derive(Deserialize)]
struct ClaudeResponse {
    result: Option<String>,
    session_id: Option<String>,
    #[serde(default)]
    is_error: bool,
    #[serde(default)]
    total_cost_usd: f64,
}

// ── Transcript ──

struct Message {
    agent: String,
    round: usize,
    text: String,
}

struct Panel {
    topic: String,
    max_turns: usize,
    model: String,
    moderator_model: String,
    moderator_role: String,
    moderator_effort: Option<String>,
    moderator_session: Option<String>,
    agents: Vec<PanelAgent>,
    transcript: Vec<Message>,
    transcript_path: String,
    fallback_model: Option<String>,
    permission_mode: Option<String>,
    effort: Option<String>,
    extra_flags: Vec<String>,
    verbose: bool,
    total_cost: f64,
    output_json: bool,
    cwd: Option<String>,
    add_dirs: Vec<String>,
    mcp_config: Option<String>,
    project_context: String,
}

struct PanelAgent {
    name: String,
    role: String,
    color: String,
    model: Option<String>,
    effort: Option<String>,
    max_turns: Option<usize>,
    allowed_tools: Vec<String>,
    disallowed_tools: Vec<String>,
    session_id: Option<String>,
}

// ── Moderator decision ──

#[derive(Deserialize)]
struct ModeratorDecision {
    action: String,
    #[serde(default)]
    guidance: String,
}

// ── Color ──

fn colorize(text: &str, color: &str) -> ColoredString {
    match color {
        "red" => text.red(),
        "blue" => text.blue(),
        "green" => text.green(),
        "yellow" => text.yellow(),
        "magenta" => text.magenta(),
        "cyan" => text.cyan(),
        _ => text.white(),
    }
}

// ── Config resolution ──

fn resolve_config(path: &str) -> String {
    match path {
        "debate" => BUILTIN_DEBATE.to_string(),
        "code-review" => BUILTIN_CODE_REVIEW.to_string(),
        "microservices" => BUILTIN_MICROSERVICES.to_string(),
        _ => fs::read_to_string(path)
            .unwrap_or_else(|e| { eprintln!("Failed to read config: {e}"); std::process::exit(1); })
    }
}

fn default_config(topic: &str) -> Config {
    Config {
        session: SessionConfig {
            topic: topic.to_string(),
            max_rounds: 5,
            model: "sonnet".into(),
            moderator_model: "sonnet".into(),
            fallback_model: None,
            max_budget_usd: None,
            permission_mode: None,
            effort: None,
            max_turns: 2,
            extra_flags: Vec::new(),
        },
        moderator: ModeratorConfig {
            role: DEFAULT_MODERATOR_ROLE.to_string(),
            model: None,
            effort: None,
        },
        agents: vec![
            AgentConfig {
                name: "Advocate".into(),
                role: "You argue in favor of the topic. Provide concrete examples and data. Acknowledge strong counterpoints but defend your position.".into(),
                color: "green".into(),
                model: None, effort: None, max_turns: None,
                disallowed_tools: Vec::new(), allowed_tools: Vec::new(),
            },
            AgentConfig {
                name: "Critic".into(),
                role: "You take a skeptical position. Highlight practical issues, trade-offs, and hidden costs. Acknowledge strong points but stay critical.".into(),
                color: "red".into(),
                model: None, effort: None, max_turns: None,
                disallowed_tools: Vec::new(), allowed_tools: Vec::new(),
            },
        ],
        project: ProjectConfig::default(),
    }
}

// ── Claude call ──

fn call_claude(params: &ClaudeCallParams) -> Result<(String, String, f64), String> {
    let mut cmd = Command::new("claude");
    cmd.args(["-p", params.prompt]);
    cmd.args(["--append-system-prompt", params.system_prompt]);
    cmd.args(["--output-format", "json"]);
    cmd.args(["--max-turns", &params.max_turns.to_string()]);
    cmd.args(["--model", params.model]);

    if let Some(cwd) = params.cwd {
        cmd.current_dir(cwd);
    }
    for dir in params.add_dirs {
        cmd.args(["--add-dir", dir]);
    }
    if let Some(mcp) = params.mcp_config {
        cmd.args(["--mcp-config", mcp]);
    }

    if let Some(sid) = params.session_id {
        cmd.args(["--resume", sid]);
    }
    if let Some(effort) = params.effort {
        cmd.args(["--effort", effort]);
    }
    if let Some(fallback) = params.fallback_model {
        cmd.args(["--fallback-model", fallback]);
    }
    if let Some(perm) = params.permission_mode {
        cmd.args(["--permission-mode", perm]);
    }
    for tool in params.allowed_tools {
        cmd.args(["--allowedTools", tool]);
    }
    for tool in params.disallowed_tools {
        cmd.args(["--disallowedTools", tool]);
    }
    for flag in params.extra_flags {
        cmd.arg(flag);
    }

    let output = cmd.output().map_err(|e| format!("failed to run claude: {e}"))?;

    if params.verbose {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.is_empty() {
            eprintln!("{}", stderr.dimmed());
        }
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("claude produced no output: {stderr}"));
    }

    let resp = parse_claude_json(&stdout)?;
    if resp.is_error {
        return Err(format!("claude error: {}", resp.result.as_deref().unwrap_or("?")));
    }

    let text = resp.result.ok_or("empty response")?;
    let sid = resp.session_id.unwrap_or_default();
    let cost = resp.total_cost_usd;
    Ok((text, sid, cost))
}

fn parse_claude_json(raw: &str) -> Result<ClaudeResponse, String> {
    if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(raw) {
        for obj in &arr {
            if obj.get("type").and_then(|v| v.as_str()) == Some("result") {
                if let Ok(r) = serde_json::from_value::<ClaudeResponse>(obj.clone()) {
                    return Ok(r);
                }
            }
        }
    }
    if let Ok(r) = serde_json::from_str::<ClaudeResponse>(raw) {
        return Ok(r);
    }
    Err(format!("JSON parse error: {}", &raw[..raw.len().min(200)]))
}

// ── Panel methods ──

impl Panel {
    fn format_transcript_for_prompt(&self) -> String {
        let mut buf = String::new();
        for msg in &self.transcript {
            let _ = writeln!(buf, "[{} -- round {}]\n{}\n", msg.agent, msg.round, msg.text);
        }
        buf
    }

    fn format_last_round(&self, round: usize) -> String {
        let mut buf = String::new();
        for msg in &self.transcript {
            if msg.round == round {
                let _ = writeln!(buf, "[{}]\n{}\n", msg.agent, msg.text);
            }
        }
        buf
    }

    fn append_file(&self, agent: &str, round: usize, text: &str) {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&self.transcript_path) {
            let entry = serde_json::json!({
                "timestamp": Local::now().to_rfc3339(),
                "agent": agent,
                "round": round,
                "message": text,
            });
            let _ = writeln!(f, "{entry}");
        }
    }

    fn agent_model<'a>(&'a self, agent: &'a PanelAgent) -> &'a str {
        agent.model.as_deref().unwrap_or(&self.model)
    }

    fn agent_effort<'a>(&'a self, agent: &'a PanelAgent) -> Option<&'a str> {
        agent.effort.as_deref().or(self.effort.as_deref())
    }

    fn agent_max_turns(&self, agent: &PanelAgent) -> usize {
        agent.max_turns.unwrap_or(self.max_turns)
    }

    fn make_params<'a>(
        &'a self,
        prompt: &'a str,
        system_prompt: &'a str,
        model: &'a str,
        effort: Option<&'a str>,
        max_turns: usize,
        session_id: Option<&'a str>,
        allowed_tools: &'a [String],
        disallowed_tools: &'a [String],
    ) -> ClaudeCallParams<'a> {
        ClaudeCallParams {
            prompt, system_prompt, model, session_id, max_turns, effort,
            fallback_model: self.fallback_model.as_deref(),
            permission_mode: self.permission_mode.as_deref(),
            allowed_tools, disallowed_tools,
            extra_flags: &self.extra_flags,
            verbose: self.verbose,
            cwd: self.cwd.as_deref(),
            add_dirs: &self.add_dirs,
            mcp_config: self.mcp_config.as_deref(),
        }
    }

    fn enrich_prompt(&self, prompt: &str) -> String {
        if self.project_context.is_empty() {
            return prompt.to_string();
        }
        format!("{prompt}\n\n---\nPROJECT CONTEXT:\n{}", self.project_context)
    }

    fn track_cost(&mut self, cost: f64, _agent_name: &str) {
        self.total_cost += cost;
        if cost > 0.0 {
            eprintln!("{}", format!("  |- ${cost:.4} (total: ${:.4})", self.total_cost).dimmed());
        }
    }
}

// ── Main ──

fn main() {
    let cli = Cli::parse();

    let config: Config = match (&cli.config, &cli.topic) {
        (Some(path), _) => {
            let config_str = resolve_config(path);
            toml::from_str(&config_str)
                .unwrap_or_else(|e| { eprintln!("Config parse error: {e}"); std::process::exit(1); })
        }
        (None, Some(topic)) => default_config(topic),
        (None, None) => {
            eprintln!("Error: provide a TOML config or --topic \"Your question\"\n");
            eprintln!("Quick start:");
            eprintln!("  roundtable debate -m haiku -r 2");
            eprintln!("  roundtable --topic \"Should we use microservices?\" -m haiku");
            eprintln!("  roundtable my-config.toml");
            std::process::exit(1);
        }
    };

    let max_rounds = cli.rounds.unwrap_or(config.session.max_rounds);

    let transcript_path = cli.transcript.unwrap_or_else(|| {
        let dir = std::env::temp_dir().join("roundtable");
        fs::create_dir_all(&dir).ok();
        let ts = Local::now().format("%Y%m%d-%H%M%S");
        dir.join(format!("{ts}.jsonl")).to_string_lossy().to_string()
    });

    let model = cli.model.unwrap_or(config.session.model);
    let moderator_model = cli.moderator_model
        .or(config.moderator.model)
        .unwrap_or(config.session.moderator_model);
    let fallback_model = cli.fallback_model.or(config.session.fallback_model);
    let permission_mode = cli.permission_mode.or(config.session.permission_mode);
    let effort = cli.effort.or(config.session.effort);
    let topic = cli.topic.unwrap_or(config.session.topic);
    let max_budget = cli.max_budget_usd.or(config.session.max_budget_usd);

    let agents: Vec<PanelAgent> = config.agents.iter().map(|a| PanelAgent {
        name: a.name.clone(), role: a.role.clone(), color: a.color.clone(),
        model: a.model.clone(), effort: a.effort.clone(), max_turns: a.max_turns,
        allowed_tools: a.allowed_tools.clone(), disallowed_tools: a.disallowed_tools.clone(),
        session_id: None,
    }).collect();

    let agent_names: Vec<String> = agents.iter().map(|a| a.name.clone()).collect();

    let cwd = cli.cwd.clone();
    let mut add_dirs: Vec<String> = cli.add_dir.clone();
    add_dirs.extend(config.project.add_dirs.clone());
    let mcp_config = cli.mcp_config.or(config.project.mcp_config);

    let mut project_context = String::new();
    if let Some(ref desc) = config.project.description {
        let _ = writeln!(project_context, "Project: {desc}\n");
    }
    let mut ctx_files = cli.context_file.clone();
    ctx_files.extend(config.project.context_files.clone());
    for path in &ctx_files {
        match fs::read_to_string(path) {
            Ok(content) => {
                let _ = writeln!(project_context, "-- {} --\n{}\n", path, content.trim());
            }
            Err(e) => eprintln!("{}", format!("WARNING: {path} could not be read: {e}").yellow()),
        }
    }

    let mut panel = Panel {
        topic: topic.clone(), max_turns: config.session.max_turns, model, moderator_model,
        moderator_role: config.moderator.role,
        moderator_effort: config.moderator.effort.or(effort.clone()),
        moderator_session: None, agents, transcript: Vec::new(), transcript_path,
        fallback_model, permission_mode, effort,
        extra_flags: config.session.extra_flags,
        verbose: cli.verbose, total_cost: 0.0, output_json: cli.output_json,
        cwd, add_dirs, mcp_config, project_context,
    };

    // Banner
    if !cli.output_json {
        println!("{}", "=".repeat(60).green());
        println!("{}", format!("  Roundtable -- {} agents + moderator", panel.agents.len()).green());
        println!("{}", format!("  Topic: {}", topic).green());
        print!("{}", "  Agents: ".green());
        for (i, name) in agent_names.iter().enumerate() {
            print!("{}", colorize(name, &panel.agents[i].color));
            if i < agent_names.len() - 1 { print!(", "); }
        }
        println!();
        println!("{}", format!("  Model: {} | Moderator: {}", panel.model, panel.moderator_model).green());
        if let Some(ref fb) = panel.fallback_model {
            println!("{}", format!("  Fallback: {fb}").green());
        }
        if let Some(ref eff) = panel.effort {
            println!("{}", format!("  Effort: {eff}").green());
        }
        if let Some(budget) = max_budget {
            println!("{}", format!("  Budget limit: ${budget:.2}").green());
        }
        if let Some(ref cwd) = panel.cwd {
            println!("{}", format!("  CWD: {cwd}").green());
        }
        if !panel.add_dirs.is_empty() {
            println!("{}", format!("  Extra dirs: {}", panel.add_dirs.join(", ")).green());
        }
        if !ctx_files.is_empty() {
            println!("{}", format!("  Context: {} files loaded", ctx_files.len()).green());
        }
        println!("{}", format!("  Max rounds: {max_rounds}").green());
        println!("{}", format!("  Transcript: {}", panel.transcript_path).green());
        println!("{}", "=".repeat(60).green());
        println!();
    }

    // ── Opening round ──
    if !cli.output_json {
        println!("{}", "-- Opening Round --".dimmed());
        println!();
    }

    let empty_tools: Vec<String> = Vec::new();

    for i in 0..panel.agents.len() {
        let raw_prompt = format!(
            "Discussion topic: {}\n\n\
             Other panel agents: {}\n\n\
             From your area of expertise, explain your view on this topic in 2-3 paragraphs. \
             Provide concrete arguments. Highlight points where you might disagree with other disciplines.",
            panel.topic,
            agent_names.join(", "),
        );
        let prompt = panel.enrich_prompt(&raw_prompt);

        let ts = Local::now().format("%H:%M:%S");
        let name = panel.agents[i].name.clone();
        let color = panel.agents[i].color.clone();
        let agent_model = panel.agent_model(&panel.agents[i]).to_string();
        let agent_effort = panel.agent_effort(&panel.agents[i]).map(|s| s.to_string());
        let agent_max_turns = panel.agent_max_turns(&panel.agents[i]);
        eprintln!("{}", format!("[{ts}] {name} thinking... ({agent_model})").dimmed());

        let params = panel.make_params(
            &prompt, &panel.agents[i].role, &agent_model, agent_effort.as_deref(),
            agent_max_turns, panel.agents[i].session_id.as_deref(),
            &panel.agents[i].allowed_tools, &panel.agents[i].disallowed_tools,
        );

        match call_claude(&params) {
            Ok((text, sid, cost)) => {
                panel.agents[i].session_id = Some(sid);
                panel.track_cost(cost, &name);

                if let Some(budget) = max_budget {
                    if panel.total_cost >= budget {
                        eprintln!("{}", format!("BUDGET LIMIT: ${budget:.2} exceeded!").red().bold());
                        print_message(&name, &text, "opening", &color);
                        panel.append_file(&name, 0, &text);
                        panel.transcript.push(Message { agent: name, round: 0, text });
                        print_final(&panel, None);
                        return;
                    }
                }

                print_message(&name, &text, "opening", &color);
                panel.append_file(&name, 0, &text);
                panel.transcript.push(Message { agent: name, round: 0, text });
            }
            Err(e) => {
                eprintln!("{}", format!("ERROR ({name}): {e}").red().bold());
                std::process::exit(1);
            }
        }
    }

    // ── Discussion loop ──
    for round in 1..=max_rounds {
        if !cli.output_json {
            println!("{}", format!("-- Round {round}/{max_rounds} --").dimmed());
            println!();
        }

        let mod_prompt = format!(
            "Discussion topic: {topic}\n\n\
             Panel agents: {names}\n\n\
             Discussion so far:\n{transcript}\n\n\
             ---\n\
             Analyze the discussion:\n\
             1. Which points are settled?\n\
             2. Which conflicts remain unresolved?\n\
             3. Which perspectives are missing?\n\n\
             Make your decision. If the discussion is mature enough, synthesize.\n\
             If it should continue, provide specific questions and guidance.\n\n\
             RESPOND LIKE THIS:\n\
             First write your analysis.\n\
             Then on the last line give ONLY JSON:\n\
             {{\"action\": \"continue\", \"guidance\": \"Specific guidance here\"}} or\n\
             {{\"action\": \"synthesize\"}}",
            topic = panel.topic,
            names = agent_names.join(", "),
            transcript = panel.format_transcript_for_prompt(),
        );

        let ts = Local::now().format("%H:%M:%S");
        eprintln!("{}", format!("[{ts}] Moderator analyzing... ({})", panel.moderator_model).dimmed());

        let mod_effort = panel.moderator_effort.clone();
        let params = panel.make_params(
            &mod_prompt, &panel.moderator_role, &panel.moderator_model,
            mod_effort.as_deref(), panel.max_turns, panel.moderator_session.as_deref(),
            &empty_tools, &empty_tools,
        );

        let (mod_text, mod_sid, mod_cost) = match call_claude(&params) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("{}", format!("ERROR (Moderator): {e}").red().bold());
                break;
            }
        };
        panel.moderator_session = Some(mod_sid);
        panel.track_cost(mod_cost, "Moderator");

        if !cli.output_json {
            println!("{}", "|- Moderator -".green().bold());
            println!("{mod_text}");
            println!("{}", "|------------------------------------------------------".green().bold());
            println!();
        }

        panel.append_file("Moderator", round, &mod_text);
        let mod_text_clone = mod_text.clone();
        panel.transcript.push(Message { agent: "Moderator".into(), round, text: mod_text });

        let decision = parse_moderator_decision(&mod_text_clone);

        match decision.action.as_str() {
            "synthesize" => {
                let synth = do_synthesis(&mut panel, round);
                print_final(&panel, synth.as_deref());
                return;
            }
            _ => {
                if let Some(budget) = max_budget {
                    if panel.total_cost >= budget {
                        eprintln!("{}", format!("BUDGET LIMIT: ${budget:.2} exceeded! Synthesizing.").yellow().bold());
                        let synth = do_synthesis(&mut panel, round);
                        print_final(&panel, synth.as_deref());
                        return;
                    }
                }

                let guidance = decision.guidance.clone();

                for i in 0..panel.agents.len() {
                    let last_round = panel.format_last_round(round);
                    let prompt = format!(
                        "Discussion topic: {topic}\n\n\
                         Messages from this round:\n{last_round}\n\n\
                         Moderator guidance: {guidance}\n\n\
                         Respond considering what other agents said.\n\
                         Say something new -- don't repeat. State your agreements/disagreements concretely.\n\
                         Keep it concise, 2-3 paragraphs.",
                        topic = panel.topic,
                        last_round = last_round,
                        guidance = guidance,
                    );

                    let name = panel.agents[i].name.clone();
                    let color = panel.agents[i].color.clone();
                    let agent_model = panel.agent_model(&panel.agents[i]).to_string();
                    let agent_effort = panel.agent_effort(&panel.agents[i]).map(|s| s.to_string());
                    let agent_max_turns = panel.agent_max_turns(&panel.agents[i]);
                    let ts = Local::now().format("%H:%M:%S");
                    eprintln!("{}", format!("[{ts}] {name} thinking... ({agent_model})").dimmed());

                    let params = panel.make_params(
                        &prompt, &panel.agents[i].role, &agent_model, agent_effort.as_deref(),
                        agent_max_turns, panel.agents[i].session_id.as_deref(),
                        &panel.agents[i].allowed_tools, &panel.agents[i].disallowed_tools,
                    );

                    match call_claude(&params) {
                        Ok((text, sid, cost)) => {
                            panel.agents[i].session_id = Some(sid);
                            panel.track_cost(cost, &name);
                            print_message(&name, &text, &format!("round {round}"), &color);
                            panel.append_file(&name, round, &text);
                            panel.transcript.push(Message { agent: name, round, text });
                        }
                        Err(e) => {
                            eprintln!("{}", format!("ERROR ({name}): {e}").red().bold());
                        }
                    }

                    if let Some(budget) = max_budget {
                        if panel.total_cost >= budget {
                            eprintln!("{}", format!("BUDGET LIMIT: ${budget:.2} exceeded!").yellow().bold());
                            break;
                        }
                    }
                }
            }
        }
    }

    print_final(&panel, None);
}

fn do_synthesis(panel: &mut Panel, round: usize) -> Option<String> {
    if !panel.output_json {
        println!("{}", "-- Final Synthesis --".green().bold());
        println!();
    }

    let synth_prompt = format!(
        "Discussion topic: {topic}\n\n\
         Full discussion:\n{transcript}\n\n\
         ---\n\
         TASK: Write the final synthesis of this discussion.\n\n\
         Format:\n\
         ## Conclusion\n\
         Main takeaway (2-3 sentences)\n\n\
         ## Consensus Points\n\
         Points all agents agreed on\n\n\
         ## Unresolved Tensions\n\
         Remaining disagreements and why they matter\n\n\
         ## Proposed Action Plan\n\
         Concrete, step-by-step plan\n\n\
         ## Risks and Mitigation\n\
         Risks identified by each discipline and proposed mitigations",
        topic = panel.topic,
        transcript = panel.format_transcript_for_prompt(),
    );

    let ts = Local::now().format("%H:%M:%S");
    eprintln!("{}", format!("[{ts}] Moderator writing synthesis...").dimmed());

    let empty_tools: Vec<String> = Vec::new();
    let mod_effort = panel.moderator_effort.clone();
    let params = panel.make_params(
        &synth_prompt, &panel.moderator_role, &panel.moderator_model,
        mod_effort.as_deref(), panel.max_turns, panel.moderator_session.as_deref(),
        &empty_tools, &empty_tools,
    );

    match call_claude(&params) {
        Ok((synth, _, cost)) => {
            panel.track_cost(cost, "Synthesis");
            if !panel.output_json {
                println!("{}", "+======================================================+".green().bold());
                println!("{}", "|              SYNTHESIS REPORT                         |".green().bold());
                println!("{}", "+======================================================+".green().bold());
                println!();
                println!("{synth}");
                println!();
            }
            panel.append_file("Synthesis", round, &synth);
            Some(synth)
        }
        Err(e) => {
            eprintln!("{}", format!("ERROR (Synthesis): {e}").red().bold());
            None
        }
    }
}

fn print_message(name: &str, text: &str, round_label: &str, color: &str) {
    let header = format!("|- {} -- {} -", name, round_label);
    let footer = "|------------------------------------------------------";
    println!("{}", colorize(&header, color));
    println!("{text}");
    println!("{}", colorize(footer, color));
    println!();
}

fn print_final(panel: &Panel, synthesis: Option<&str>) {
    if panel.output_json {
        let output = serde_json::json!({
            "topic": panel.topic,
            "total_cost_usd": panel.total_cost,
            "transcript_path": panel.transcript_path,
            "rounds": panel.transcript.len(),
            "synthesis": synthesis.unwrap_or(""),
            "agents": panel.agents.iter().map(|a| &a.name).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        println!("{}", "=".repeat(60).green());
        println!("{}", "  Panel complete.".green());
        println!("{}", format!("  Total cost: ${:.4}", panel.total_cost).green());
        println!("{}", format!("  Transcript: {}", panel.transcript_path).green());
        println!("{}", "=".repeat(60).green());
    }
}

fn parse_moderator_decision(text: &str) -> ModeratorDecision {
    for line in text.lines().rev() {
        let trimmed = line.trim().trim_start_matches("```json").trim_start_matches("```").trim();
        if trimmed.starts_with('{') && trimmed.contains("action") {
            if let Ok(d) = serde_json::from_str::<ModeratorDecision>(trimmed) {
                return d;
            }
        }
    }
    // Keyword fallback (keep both languages for backward compat)
    if text.to_lowercase().contains("synthesize") || text.to_lowercase().contains("sentez") {
        return ModeratorDecision { action: "synthesize".into(), guidance: String::new() };
    }
    ModeratorDecision {
        action: "continue".into(),
        guidance: "Continue the discussion, deepen previous arguments.".into(),
    }
}
