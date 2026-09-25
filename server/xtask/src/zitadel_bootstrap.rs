//! `cargo xtask zitadel-bootstrap`, run from `server/`: waits for the dev Zitadel
//! container, creates the `postit` project, the `postit-app` OIDC application, and the
//! `member@postit.local` user, then writes the real (generated) client id and audiences
//! into `config/local.toml`. Every step is idempotent, so re-running after `docker compose
//! down` and back `up` is safe. Machine-user auth uses the JWT profile (RFC 7523) against
//! the machine key Zitadel prints once during `FirstInstance` setup (see
//! `docker/zitadel/steps.yaml`), which this task captures from `docker compose logs zitadel`
//! on its first run and caches at `../docker/zitadel/machinekey/postit-bootstrap.json`.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use postit_config::HttpSettings;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const ISSUER: &str = "https://postit.local:44330";
const CA_PATH: &str = "../docker/shared/nginx/certs/postit-dev-ca.crt";
const MACHINE_KEY_PATH: &str = "../docker/zitadel/machinekey/postit-bootstrap.json";
const LOCAL_TOML_PATH: &str = "config/local.toml";
const PROJECT_NAME: &str = "postit";
const APP_NAME: &str = "postit-app";
const MEMBER_EMAIL: &str = "member@postit.local";
const MEMBER_PASSWORD: &str = "PostitDev1!";

#[derive(Deserialize)]
struct MachineKey {
    #[serde(rename = "userId")]
    user_id: String,
    #[serde(rename = "keyId")]
    key_id: String,
    key: String,
}

#[derive(Serialize)]
struct Claims {
    iss: String,
    sub: String,
    aud: String,
    iat: u64,
    exp: u64,
}

pub async fn run() -> Result<()> {
    let client = trusted_client()?;

    println!("Waiting for Zitadel at {ISSUER} ...");
    wait_for_discovery(&client).await?;

    let machine_key = load_or_capture_machine_key()?;
    let access_token = machine_bearer_token(&client, &machine_key).await?;

    let project_id = ensure_project(&client, &access_token).await?;
    let client_id = ensure_app(&client, &access_token, &project_id).await?;
    ensure_member_user(&client, &access_token).await?;

    write_local_toml(&client_id, &project_id)?;

    println!("Done. server/config/local.toml now has the real Zitadel client id.");
    println!("Sign in at {ISSUER}/ui/console as admin@postit.local / PostitDev1!");
    println!(
        "App users: admin@postit.local / PostitDev1!, member@postit.local / {MEMBER_PASSWORD}"
    );
    Ok(())
}

fn trusted_client() -> Result<reqwest::Client> {
    let settings = HttpSettings {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(15),
        user_agent: "postit-xtask/0".to_string(),
        extra_ca_files: vec![PathBuf::from(CA_PATH)],
    };
    postit_http::build_client(&settings).context("building a client trusting the dev CA")
}

async fn wait_for_discovery(client: &reqwest::Client) -> Result<()> {
    let url = format!("{ISSUER}/.well-known/openid-configuration");
    for attempt in 1..=60 {
        if let Ok(resp) = client.get(&url).send().await
            && resp.status().is_success()
        {
            return Ok(());
        }
        if attempt == 60 {
            bail!(
                "Zitadel never became reachable at {url}. Is `docker compose -f compose.yaml -f compose.dev.yaml up -d` running?"
            );
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    Ok(())
}

fn load_or_capture_machine_key() -> Result<MachineKey> {
    let path = Path::new(MACHINE_KEY_PATH);
    if !path.exists() {
        let raw = capture_machine_key_from_logs()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, &raw).with_context(|| format!("writing {}", path.display()))?;
    }
    let raw =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
}

fn capture_machine_key_from_logs() -> Result<String> {
    let output = std::process::Command::new("docker")
        .args([
            "compose",
            "-f",
            "compose.yaml",
            "-f",
            "compose.dev.yaml",
            "logs",
            "--no-color",
            "zitadel",
        ])
        .current_dir("..")
        .output()
        .context("running `docker compose logs zitadel`")?;
    let logs = String::from_utf8_lossy(&output.stdout);
    logs.lines()
        .rev()
        .find_map(|line| {
            let start = line.find(r#"{"type":"serviceaccount""#)?;
            Some(line[start..].to_string())
        })
        .context(
            "no machine key found in `docker compose logs zitadel`; Zitadel's FirstInstance \
             setup only prints it once, on the run that created the machine user",
        )
}

async fn machine_bearer_token(
    client: &reqwest::Client,
    machine_key: &MachineKey,
) -> Result<String> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let claims = Claims {
        iss: machine_key.user_id.clone(),
        sub: machine_key.user_id.clone(),
        aud: ISSUER.to_string(),
        iat: now,
        exp: now + 3600,
    };
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(machine_key.key_id.clone());
    let key = EncodingKey::from_rsa_pem(machine_key.key.as_bytes())
        .context("parsing the machine key's RSA private key")?;
    let assertion = encode(&header, &claims, &key).context("signing the JWT profile assertion")?;

    let resp = client
        .post(format!("{ISSUER}/oauth/v2/token"))
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", &assertion),
            ("scope", "openid urn:zitadel:iam:org:project:id:zitadel:aud"),
        ])
        .send()
        .await
        .context("requesting an access token for the bootstrap machine user")?;
    let body: Value = check(resp, "authenticating the bootstrap machine user").await?;
    body["access_token"]
        .as_str()
        .map(str::to_string)
        .context("token response had no access_token")
}

async fn ensure_project(client: &reqwest::Client, token: &str) -> Result<String> {
    let resp = client
        .post(format!("{ISSUER}/management/v1/projects"))
        .bearer_auth(token)
        .json(&json!({ "name": PROJECT_NAME }))
        .send()
        .await?;
    if resp.status() == reqwest::StatusCode::CONFLICT {
        let resp = client
            .post(format!("{ISSUER}/management/v1/projects/_search"))
            .bearer_auth(token)
            .json(&json!({
                "queries": [{ "nameQuery": { "name": PROJECT_NAME, "method": "TEXT_QUERY_METHOD_EQUALS" } }]
            }))
            .send()
            .await?;
        let body: Value = check(resp, "looking up the existing postit project").await?;
        return body["result"][0]["id"]
            .as_str()
            .map(str::to_string)
            .context("project search returned no result");
    }
    let body: Value = check(resp, "creating the postit project").await?;
    body["id"]
        .as_str()
        .map(str::to_string)
        .context("project creation returned no id")
}

async fn ensure_app(client: &reqwest::Client, token: &str, project_id: &str) -> Result<String> {
    let resp = client
        .post(format!(
            "{ISSUER}/management/v1/projects/{project_id}/apps/oidc"
        ))
        .bearer_auth(token)
        .json(&json!({
            "name": APP_NAME,
            "redirectUris": ["https://postit.local:44310/auth/callback"],
            "responseTypes": ["OIDC_RESPONSE_TYPE_CODE"],
            "grantTypes": ["OIDC_GRANT_TYPE_AUTHORIZATION_CODE", "OIDC_GRANT_TYPE_REFRESH_TOKEN"],
            "appType": "OIDC_APP_TYPE_USER_AGENT",
            "authMethodType": "OIDC_AUTH_METHOD_TYPE_NONE",
            "postLogoutRedirectUris": ["https://postit.local:44310/"],
            "devMode": true,
            "accessTokenType": "OIDC_TOKEN_TYPE_JWT",
        }))
        .send()
        .await?;
    if resp.status() == reqwest::StatusCode::CONFLICT {
        let resp = client
            .post(format!("{ISSUER}/management/v1/projects/{project_id}/apps/_search"))
            .bearer_auth(token)
            .json(&json!({
                "queries": [{ "nameQuery": { "name": APP_NAME, "method": "TEXT_QUERY_METHOD_EQUALS" } }]
            }))
            .send()
            .await?;
        let body: Value = check(resp, "looking up the existing postit-app application").await?;
        return body["result"][0]["oidcConfig"]["clientId"]
            .as_str()
            .map(str::to_string)
            .context("app search returned no clientId");
    }
    let body: Value = check(resp, "creating the postit-app OIDC application").await?;
    body["clientId"]
        .as_str()
        .map(str::to_string)
        .context("app creation returned no clientId")
}

async fn ensure_member_user(client: &reqwest::Client, token: &str) -> Result<()> {
    let resp = client
        .post(format!("{ISSUER}/management/v1/users/human/_import"))
        .bearer_auth(token)
        .json(&json!({
            "userName": MEMBER_EMAIL,
            "profile": { "firstName": "Postit", "lastName": "Member" },
            "email": { "email": MEMBER_EMAIL, "isEmailVerified": true },
            "password": MEMBER_PASSWORD,
            "passwordChangeRequired": false,
        }))
        .send()
        .await?;
    if resp.status() == reqwest::StatusCode::CONFLICT {
        return Ok(());
    }
    check::<Value>(resp, "creating the member@postit.local user").await?;
    Ok(())
}

async fn check<T: serde::de::DeserializeOwned>(resp: reqwest::Response, action: &str) -> Result<T> {
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("{action} failed: {status}: {text}");
    }
    serde_json::from_str(&text).with_context(|| format!("{action}: parsing response: {text}"))
}

fn write_local_toml(client_id: &str, project_id: &str) -> Result<()> {
    let contents = format!(
        r#"# Generated by `cargo xtask zitadel-bootstrap`. Safe to delete and regenerate;
# re-running the bootstrap task is idempotent. Git-ignored (see .gitignore).

[auth.oidc]
client_id = "{client_id}"
audiences = ["{client_id}", "{project_id}"]
"#
    );
    let path = Path::new(LOCAL_TOML_PATH);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents).with_context(|| format!("writing {}", path.display()))
}
