use reqwest::Client;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct GitHubRepo {
    pub full_name: String,
    pub description: Option<String>,
    pub created_at: String,
    pub pushed_at: String,
    pub stargazers_count: u64,
    pub forks_count: u64,
    pub open_issues_count: u64,
}

#[derive(Deserialize)]
struct GitHubContributor {
    // Fields exist for serde deserialization; only the vec length matters.
    #[allow(dead_code)]
    login: String,
}

pub struct GitHubClient {
    client: Client,
    token: Option<String>,
}

impl GitHubClient {
    pub fn new(token: Option<String>) -> Self {
        Self {
            client: Client::new(),
            token,
        }
    }

    pub async fn get_repo(&self, owner: &str, repo: &str) -> Result<GitHubRepo, reqwest::Error> {
        let url = format!("https://api.github.com/repos/{owner}/{repo}");
        let mut req = self.client.get(&url).header("User-Agent", "commit-trust");
        if let Some(token) = &self.token {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
        req.send().await?.error_for_status()?.json().await
    }

    pub async fn get_contributor_count(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<usize, reqwest::Error> {
        let url = format!(
            "https://api.github.com/repos/{owner}/{repo}/contributors?per_page=1&anon=true"
        );
        let mut req = self.client.get(&url).header("User-Agent", "commit-trust");
        if let Some(token) = &self.token {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
        let resp = req.send().await?;

        // GitHub returns contributor count in the Link header's last page number
        if let Some(link) = resp.headers().get("link")
            && let Ok(link_str) = link.to_str()
            && let Some(last) = link_str.split("page=").last()
            && let Some(num) = last.split('>').next()
            && let Ok(count) = num.parse::<usize>()
        {
            return Ok(count);
        }

        // Fallback: count the response body
        let contributors: Vec<GitHubContributor> = resp.json().await?;
        Ok(contributors.len())
    }
}
