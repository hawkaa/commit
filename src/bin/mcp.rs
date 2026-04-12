use std::io::{self, BufRead, Write};
use std::sync::{Arc, Mutex};

use commit_backend::AppState;
use commit_backend::models::{CommitScore, ScoreBreakdown, SubjectKind};
use commit_backend::services::db::Database;
use commit_backend::services::github::GitHubClient;
use commit_backend::services::score::{build_signals, score_github_repo};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::main]
async fn main() {
    let github_token = std::env::var("GITHUB_TOKEN").ok();
    let db_path = std::env::var("DATABASE_PATH").unwrap_or_else(|_| "commit.db".to_string());
    let db = Database::open(&db_path).expect("Failed to open database");
    let github = GitHubClient::new(github_token);

    let notary_public_key = std::env::var("NOTARY_PUBLIC_KEY").ok();

    let state = AppState {
        db: Arc::new(Mutex::new(db)),
        github: Arc::new(github),
        notary_public_key,
    };

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };

        if line.trim().is_empty() {
            continue;
        }

        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let err = json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {
                        "code": -32700,
                        "message": format!("Parse error: {e}")
                    }
                });
                writeln!(stdout, "{err}").ok();
                stdout.flush().ok();
                continue;
            }
        };

        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");

        let response = match method {
            "initialize" => handle_initialize(&id),
            "notifications/initialized" => continue, // no response needed
            "tools/list" => handle_tools_list(&id),
            "tools/call" => handle_tools_call(&id, &request, &state).await,
            _ => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": format!("Method not found: {method}")
                }
            }),
        };

        writeln!(stdout, "{response}").ok();
        stdout.flush().ok();
    }
}

fn handle_initialize(id: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "commit-mcp",
                "version": "0.1.0"
            }
        }
    })
}

fn handle_tools_list(id: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "tools": [
                {
                    "name": "get_commit_score",
                    "description": "Get the Commit Score (0-100) for a subject. The score measures long-term commitment based on public signals like project age, maintenance activity, community size, and financial health.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "kind": {
                                "type": "string",
                                "description": "Subject type: github, npm, crates, business, service",
                                "enum": ["github", "npm", "crates", "business", "service"]
                            },
                            "id": {
                                "type": "string",
                                "description": "Subject identifier. For GitHub repos: owner/repo. For npm: package-name. For businesses: org number."
                            }
                        },
                        "required": ["kind", "id"]
                    }
                },
                {
                    "name": "get_trust_card",
                    "description": "Get a full trust card with all commitment signals and score breakdown for a subject. Returns individual signals (age, maintenance, contributors, etc.) plus the composite Commit Score.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "kind": {
                                "type": "string",
                                "description": "Subject type: github, npm, crates, business, service",
                                "enum": ["github", "npm", "crates", "business", "service"]
                            },
                            "id": {
                                "type": "string",
                                "description": "Subject identifier. For GitHub repos: owner/repo. For npm: package-name. For businesses: org number."
                            }
                        },
                        "required": ["kind", "id"]
                    }
                }
            ]
        }
    })
}

async fn handle_tools_call(id: &Value, request: &Value, state: &AppState) -> Value {
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    let tool_name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or(Value::Object(serde_json::Map::new()));

    let kind_str = arguments.get("kind").and_then(Value::as_str).unwrap_or("");
    let id_str = arguments.get("id").and_then(Value::as_str).unwrap_or("");

    let Some(kind) = SubjectKind::parse(kind_str) else {
        return tool_error(
            id,
            &format!(
                "Unknown subject kind: {kind_str}. Use: github, npm, crates, business, service"
            ),
        );
    };

    match (tool_name, &kind) {
        ("get_commit_score", SubjectKind::GithubRepo) => get_github_score(id, state, id_str).await,
        ("get_trust_card", SubjectKind::GithubRepo) => {
            get_github_trust_card(id, state, id_str).await
        }
        (_, SubjectKind::GithubRepo) => tool_error(id, &format!("Unknown tool: {tool_name}")),
        _ => tool_error(
            id,
            &format!(
                "{kind_str} subjects are not yet supported. Only github is available in Phase 1."
            ),
        ),
    }
}

async fn get_github_score(id: &Value, state: &AppState, identifier: &str) -> Value {
    match fetch_github_data(state, identifier).await {
        Ok((_, _, score)) => {
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "content": [{
                        "type": "text",
                        "text": serde_json::to_string_pretty(&json!({
                            "score": score.score,
                            "layer1_only": score.layer1_only,
                            "breakdown": score.breakdown
                        })).unwrap_or_default()
                    }]
                }
            })
        }
        Err(e) => tool_error(id, &e),
    }
}

async fn get_github_trust_card(id: &Value, state: &AppState, identifier: &str) -> Value {
    match fetch_github_data(state, identifier).await {
        Ok((subject, signals, score)) => {
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "content": [{
                        "type": "text",
                        "text": serde_json::to_string_pretty(&json!({
                            "subject": {
                                "kind": subject.kind,
                                "identifier": subject.identifier,
                                "display_name": subject.display_name,
                            },
                            "score": score,
                            "signals": signals,
                        })).unwrap_or_default()
                    }]
                }
            })
        }
        Err(e) => tool_error(id, &e),
    }
}

async fn fetch_github_data(
    state: &AppState,
    identifier: &str,
) -> Result<
    (
        commit_backend::models::Subject,
        Vec<commit_backend::models::CommitmentSignal>,
        CommitScore,
    ),
    String,
> {
    let parts: Vec<&str> = identifier.splitn(2, '/').collect();
    if parts.len() != 2 {
        return Err(format!(
            "Invalid GitHub identifier: {identifier}. Expected format: owner/repo"
        ));
    }
    let (owner, repo_name) = (parts[0], parts[1]);

    // Check cache
    {
        let db = state.db.lock().map_err(|e| format!("DB lock error: {e}"))?;
        if let Ok(Some(subject)) = db.find_subject(&SubjectKind::GithubRepo, identifier)
            && let Ok(Some((signals_json, score_json))) = db.get_cached_signals(&subject.id)
        {
            let signals = serde_json::from_str(&signals_json).unwrap_or_default();
            let score: CommitScore = serde_json::from_str(&score_json).unwrap_or(CommitScore {
                score: None,
                breakdown: ScoreBreakdown::default(),
                layer1_only: true,
            });
            return Ok((subject, signals, score));
        }
    }

    // Fetch from GitHub
    let gh_repo = state
        .github
        .get_repo(owner, repo_name)
        .await
        .map_err(|e| format!("GitHub API error: {e}"))?;

    let contributor_count = state
        .github
        .get_contributor_count(owner, repo_name)
        .await
        .unwrap_or(0);

    let score = score_github_repo(&gh_repo, contributor_count);
    let signals = build_signals(&gh_repo, contributor_count);

    let candidate = commit_backend::models::Subject {
        id: Uuid::new_v4(),
        kind: SubjectKind::GithubRepo,
        identifier: identifier.to_string(),
        display_name: gh_repo.full_name.clone(),
        endorsement_count: 0,
    };

    let db = state.db.lock().map_err(|e| format!("DB lock error: {e}"))?;
    let _ = db.upsert_subject(&candidate);
    let subject = db
        .find_subject(&SubjectKind::GithubRepo, identifier)
        .map_err(|e| format!("DB error: {e}"))?
        .ok_or("Subject not found after upsert")?;
    let _ = db.cache_signals(
        &subject.id,
        &serde_json::to_string(&signals).unwrap_or_default(),
        &serde_json::to_string(&score).unwrap_or_default(),
    );

    Ok((subject, signals, score))
}

fn tool_error(id: &Value, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "content": [{
                "type": "text",
                "text": message
            }],
            "isError": true
        }
    })
}
