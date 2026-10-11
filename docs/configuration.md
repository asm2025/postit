# Configuration reference

postit is configured by layered files and environment variables. This page lists every setting under the environment variable that sets it. A test (`server/crates/config/tests/reference_doc.rs`) fails when a setting exists that is not listed here, so the tables below cannot silently fall behind the code.

## How configuration is loaded

Settings are merged from these layers, in this order. A later layer overrides an earlier one:

1. `config/default.toml`, the baseline shared by every environment.
2. `config/{environment}.toml`, the overlay for `development`, `qa` or `production`.
3. `config/local.toml`, read in development only. It is git-ignored and holds developer-machine overrides.
4. The dotenv file named by `POSTIT_SECRETS_FILE`, if that variable is set. A native `cargo run` uses it to read the same vault file a container receives through `env_file:`.
5. `POSTIT__SECTION__KEY` environment variables.
6. `POSTIT__SECTION__KEY_FILE` variables, which read the value from a file (see Secrets below). These win over every other layer.

An unknown key in any layer is an error, so a misspelled variable stops startup instead of being ignored.

### Process variables

Three variables select how a process starts. They are not settings and are not part of the settings tree:

| Variable | Meaning |
| --- | --- |
| `POSTIT_ENV` | The environment: `development`, `qa` or `production`. A debug build defaults to `development` when it is unset. A release build, including every container image, refuses to start without it. |
| `POSTIT_ROLE` | What the process runs: `all` (API and worker together), `api`, or `worker`. It defaults to `all` in development. Outside development it is required. |
| `POSTIT_SECRETS_FILE` | Path to a dotenv file of `POSTIT__SECTION__KEY=value` lines, read before the environment. Real environment variables override it. |

### Naming a variable

A setting `section.sub_section.key` is set by `POSTIT__SECTION__SUB_SECTION__KEY`: upper case, with a double underscore between levels. A single underscore stays part of a name, so `server.api_port` is `POSTIT__SERVER__API_PORT`.

### Value syntax

- **Text, numbers and booleans** are written as they are: `44310`, `true`, `postit-app`.
- **Lists** are written in brackets, with the whole value in single quotes so the shell leaves the double quotes alone: `'["https://app.example.com"]'`. Items are always kept as text. Lists are split on commas, so an item cannot itself contain a comma. List syntax works only in real environment variables: values read from the `POSTIT_SECRETS_FILE` dotenv file are never parsed as lists.
- **Durations** use humantime syntax, such as `"30s"`, `"5min"`, `"1h"` or `"30days"`. Every duration must be greater than zero and at most 100 years, except `POSTIT__AUTH__OIDC__LEEWAY`, which may be `"0s"`.
- **Optional settings** are simply left unset. A blank value for the two bootstrap settings counts as unset.
- **Values containing `$`** should be quoted with single quotes, so the shell and dotenv parsers do not expand them.

## Secrets

Five settings are secrets:

- `POSTIT__DATABASE__USERNAME`
- `POSTIT__DATABASE__PASSWORD`
- `POSTIT__AUDIT__PSEUDONYM_KEY`
- `POSTIT__MAIL__SMTP__USERNAME`
- `POSTIT__MAIL__SMTP__PASSWORD`

All five come from the vault and are secrets by policy, and none of them appears in the committed TOML files. Masking in logs and admin dumps is narrower: only the password and key settings (`POSTIT__DATABASE__PASSWORD`, `POSTIT__AUDIT__PSEUDONYM_KEY`, `POSTIT__MAIL__SMTP__PASSWORD`) print as `[redacted]`. The two usernames are plain strings and appear in clear text in the startup configuration dump. Each one can be supplied in either of two ways:

- As a plain variable, for example `POSTIT__DATABASE__PASSWORD=...`.
- As a `_FILE` variable holding the path of a file whose contents are the value, for example `POSTIT__DATABASE__PASSWORD_FILE=/run/secrets/db_password`. Leading and trailing whitespace in the file is trimmed. `_FILE` variables are honoured only as real process environment variables; a `..._FILE` line inside the `POSTIT_SECRETS_FILE` dotenv file is ignored. This matches how Docker secrets, Kubernetes secrets, and the secret mounts of ECS and Azure Container Apps deliver values.

`POSTIT__DATABASE__URL` must never carry credentials. Put only the host, port and database name in it, and supply the user and password through the two database secrets. A URL with a username or password is rejected at startup.

## Required outside development

The `qa` and `production` environments refuse to start unless all of the following hold. The shipped `qa.toml` fills in some of these for QA; production leaves them to the deployment.

| Requirement | Setting |
| --- | --- |
| The OIDC issuer is an `https` URL. | `POSTIT__AUTH__OIDC__ISSUER` |
| At least one accepted audience is set. This applies in every environment. | `POSTIT__AUTH__OIDC__AUDIENCES` |
| The database password is not blank. | `POSTIT__DATABASE__PASSWORD` |
| The SMTP host is not blank. | `POSTIT__MAIL__SMTP__HOST` |
| The mail sender is not blank. | `POSTIT__MAIL__FROM_ADDRESS` |
| The SMTP connection uses TLS, so `none` is not allowed. | `POSTIT__MAIL__SMTP__TLS` |
| The audit pseudonymisation key is not blank. This applies in every environment. | `POSTIT__AUDIT__PSEUDONYM_KEY` |

Production has one further rule that depends on the database. At startup it must find at least one active admin, or else a bootstrap rule that will create one: `POSTIT__AUTH__BOOTSTRAP__ADMIN_EMAIL` or `POSTIT__AUTH__BOOTSTRAP__ADMIN_SUBJECT`, never both. Without either, production refuses to start.

### Production Compose variables

`docker/docker-compose.production.yml` maps a smaller set of `docker/.env` variables onto the settings above. The first six are required; Compose stops with a message if one is missing.

| Compose variable | Required | Sets |
| --- | --- | --- |
| `POSTIT_API_URL` | yes | `POSTIT__SERVER__PUBLIC_URL` |
| `POSTIT_APP_URL` | yes | `POSTIT__APP__PUBLIC_URL`, and the single entry of `POSTIT__CORS__ALLOWED_ORIGINS` |
| `POSTIT_OIDC_ISSUER` | yes | `POSTIT__AUTH__OIDC__ISSUER` |
| `POSTIT_OIDC_AUDIENCE` | yes | the single entry of `POSTIT__AUTH__OIDC__AUDIENCES` |
| `POSTIT_SMTP_HOST` | yes | `POSTIT__MAIL__SMTP__HOST` |
| `POSTIT_MAIL_FROM` | yes | `POSTIT__MAIL__FROM_ADDRESS` |
| `POSTIT_BOOTSTRAP_ADMIN_EMAIL` | no | `POSTIT__AUTH__BOOTSTRAP__ADMIN_EMAIL`; leave it empty once the first admin exists |
| `POSTIT_TRUSTED_PROXIES` | no, defaults to `172.30.0.1/32` | the entries of `POSTIT__SERVER__TRUSTED_PROXIES` |

## Reference

In the tables below, the Default column shows the value in `config/default.toml`. "Required" means `default.toml` has no value, so one must come from an environment overlay or from a variable. The shipped overlays already supply some of these:

- `POSTIT__DATABASE__URL` in `development.toml`, `qa.toml` and `production.toml`.
- `POSTIT__MAIL__SMTP__PORT` in all three overlays: 25 in development, 587 in qa and production.
- `POSTIT__SERVER__PUBLIC_URL`, `POSTIT__APP__PUBLIC_URL`, `POSTIT__AUTH__OIDC__ISSUER`, `POSTIT__AUTH__OIDC__AUDIENCES`, `POSTIT__MAIL__FROM_ADDRESS` and `POSTIT__MAIL__SMTP__HOST` in `development.toml` and `qa.toml`, but not in `production.toml`.

The username, password and pseudonym key are never in a TOML file and must always come from the vault or a variable. "Unset" means the setting is optional and off until you provide it. The Secret column marks secrets by policy; "yes (not masked)" means the value is a secret but is printed in clear text in configuration dumps, while plain "yes" values print as `[redacted]`.

### server

Listeners, proxies and request handling.

| Variable | Default | Secret | Meaning |
| --- | --- | --- | --- |
| `POSTIT__SERVER__HOST` | `0.0.0.0` | no | Interface address both listeners bind. |
| `POSTIT__SERVER__API_PORT` | `44310` | no | Port of the API listener. |
| `POSTIT__SERVER__WORKER_PORT` | `44311` | no | Port of the worker's health and metrics listener. |
| `POSTIT__SERVER__TLS__ENABLED` | `false` | no | Whether the API terminates TLS itself. |
| `POSTIT__SERVER__TLS__CERT_PATH` | unset | no | PEM certificate chain. Required when TLS is enabled. |
| `POSTIT__SERVER__TLS__KEY_PATH` | unset | no | PEM private key. Required when TLS is enabled. |
| `POSTIT__SERVER__TRUSTED_PROXIES` | empty list | no | Reverse-proxy addresses or CIDRs whose forwarded headers are trusted for the client IP. |
| `POSTIT__SERVER__PUBLIC_URL` | required | no | Externally visible base URL of the API. |
| `POSTIT__SERVER__SHUTDOWN_TIMEOUT` | `30s` | no | How long shutdown waits for in-flight work before exiting. |
| `POSTIT__SERVER__REQUEST_TIMEOUT` | `30s` | no | Per-request timeout applied to API routes. |
| `POSTIT__SERVER__BODY_LIMIT` | `1048576` | no | Maximum request body size in bytes. Must be at least 1. |
| `POSTIT__SERVER__WEB__ENABLED` | `false` | no | Whether the server also serves the web client. |
| `POSTIT__SERVER__WEB__ROOT` | unset | no | Directory holding the built web client. Required when web serving is enabled. |
| `POSTIT__SERVER__WEB__BIND` | unset | no | Socket address of a dedicated web listener, for example `0.0.0.0:44315`. When unset, the web app is served on the API listener. Must be an IP socket address. |
| `POSTIT__SERVER__WEB__API_BASE_URL` | unset | no | Base URL the web client calls for the API. When unset, it falls back to `server.public_url`. |

### database

| Variable | Default | Secret | Meaning |
| --- | --- | --- | --- |
| `POSTIT__DATABASE__URL` | required | no | Where the database is, as `postgres://host:port/name`. It must not carry credentials. |
| `POSTIT__DATABASE__USERNAME` | required | yes (not masked) | Database user. |
| `POSTIT__DATABASE__PASSWORD` | required | yes | Database password. Must not be blank outside development. |
| `POSTIT__DATABASE__MAX_CONNECTIONS` | `10` | no | Upper bound on pooled connections. |

### auth

Sign-in through the OIDC provider, first-admin bootstrap, and account lifecycle.

| Variable | Default | Secret | Meaning |
| --- | --- | --- | --- |
| `POSTIT__AUTH__OIDC__ISSUER` | required | no | Issuer URL. The `iss` claim must match it, and discovery starts here. Must be `https` outside development. |
| `POSTIT__AUTH__OIDC__AUDIENCES` | required | no | Accepted `aud` values. A token must carry at least one. Must not be empty. |
| `POSTIT__AUTH__OIDC__CLIENT_ID` | `postit-app` | no | OAuth client ID of the web and mobile clients. |
| `POSTIT__AUTH__OIDC__SCOPES` | `openid`, `profile`, `email`, `offline_access` | no | Scopes the clients request at sign-in. |
| `POSTIT__AUTH__OIDC__ACCEPTED_ALGORITHMS` | `RS256` | no | JWT signature algorithms the verifier accepts. |
| `POSTIT__AUTH__OIDC__LEEWAY` | `1min` | no | Clock-skew tolerance when checking `exp` and `nbf`. May be zero. |
| `POSTIT__AUTH__OIDC__JWKS_REFRESH_INTERVAL` | `1h` | no | How often the signing keys are refreshed. |
| `POSTIT__AUTH__OIDC__CLAIM_NAMES__EMAIL` | `email` | no | Claim carrying the email address. |
| `POSTIT__AUTH__OIDC__CLAIM_NAMES__EMAIL_VERIFIED` | `email_verified` | no | Claim carrying whether the email address is verified. |
| `POSTIT__AUTH__OIDC__CLAIM_NAMES__NAME` | `name` | no | Claim carrying the display name. |
| `POSTIT__AUTH__OIDC__CLAIM_NAMES__PREFERRED_USERNAME` | `preferred_username` | no | Claim carrying the preferred username. |
| `POSTIT__AUTH__OIDC__USERINFO` | `fallback` | no | When the `userinfo` endpoint is consulted: `fallback` (only when the token lacks needed claims), `always`, or `never`. |
| `POSTIT__AUTH__OIDC__ACCOUNT_URL` | unset | no | Link to the provider's account-management page, shown to users when set. |
| `POSTIT__AUTH__BOOTSTRAP__ADMIN_EMAIL` | unset | no | Email address whose first sign-in becomes the initial admin. Mutually exclusive with the subject setting. A blank value counts as unset. |
| `POSTIT__AUTH__BOOTSTRAP__ADMIN_SUBJECT` | unset | no | Subject (`sub`) whose first sign-in becomes the initial admin. Mutually exclusive with the email setting. A blank value counts as unset. |
| `POSTIT__AUTH__PENDING_TTL` | `30days` | no | How long a user may stay pending before being purged. |
| `POSTIT__AUTH__PRINCIPAL_CACHE_TTL` | `5min` | no | How long a resolved principal is cached. |
| `POSTIT__AUTH__APPROVAL_EMAIL_INTERVAL` | `1h` | no | Minimum interval between approval-request emails to admins. |

### audit

| Variable | Default | Secret | Meaning |
| --- | --- | --- | --- |
| `POSTIT__AUDIT__RETENTION` | `365days` | no | How long audit events are kept. |
| `POSTIT__AUDIT__IP_RETENTION` | `90days` | no | How long the IP address on an audit event is kept before it is cleared. |
| `POSTIT__AUDIT__PSEUDONYM_KEY` | required | yes | HMAC key used to pseudonymise audit subjects when a user is deleted. Must not be blank in any environment. |

### retention

Retention windows for operational data other than the audit log. These four settings are reserved: they are validated at load, but no job reads them yet.

| Variable | Default | Secret | Meaning |
| --- | --- | --- | --- |
| `POSTIT__RETENTION__NOTIFICATIONS_READ_AFTER` | `90days` | no | Reserved: how long after being read a notification is kept. |
| `POSTIT__RETENTION__DELIVERY_ATTEMPTS_AFTER` | `180days` | no | Reserved: how long delivery attempt rows are kept. |
| `POSTIT__RETENTION__AI_GENERATION_PROMPT_AFTER` | `30days` | no | Reserved: how long an AI generation's prompt text is kept. |
| `POSTIT__RETENTION__AI_GENERATION_ROW_AFTER` | `365days` | no | Reserved: how long an AI generation row is kept. |

### ops

The periodic operational health check. Both settings are reserved: validated at load, not yet read by any job.

| Variable | Default | Secret | Meaning |
| --- | --- | --- | --- |
| `POSTIT__OPS__CHECK_INTERVAL` | `5min` | no | Reserved: how often the check runs. |
| `POSTIT__OPS__DUE_DELIVERY_OVERDUE_AFTER` | `5min` | no | Reserved: how late a due delivery may be before it is flagged as overdue. |

### rate_limit

Token-bucket limits applied by the API. Every rate and burst must be at least 1.

| Variable | Default | Secret | Meaning |
| --- | --- | --- | --- |
| `POSTIT__RATE_LIMIT__UNAUTHENTICATED__RATE_PER_MINUTE` | `60` | no | Sustained refill rate, in requests per minute, per client IP. |
| `POSTIT__RATE_LIMIT__UNAUTHENTICATED__BURST` | `20` | no | Bucket capacity, the largest burst allowed, per client IP. |
| `POSTIT__RATE_LIMIT__PROVISIONING__RATE_PER_MINUTE` | `10` | no | Sustained refill rate, in requests per minute, for first-sign-in provisioning, keyed by client IP. |
| `POSTIT__RATE_LIMIT__PROVISIONING__BURST` | `5` | no | Bucket capacity for first-sign-in provisioning requests. |
| `POSTIT__RATE_LIMIT__AUTHENTICATED__RATE_PER_MINUTE` | `300` | no | Sustained refill rate, in requests per minute, per authenticated user. |
| `POSTIT__RATE_LIMIT__AUTHENTICATED__BURST` | `60` | no | Bucket capacity per authenticated user. |

### mail

| Variable | Default | Secret | Meaning |
| --- | --- | --- | --- |
| `POSTIT__MAIL__TRANSPORT` | `smtp` | no | Delivery transport. `smtp` is the only one. |
| `POSTIT__MAIL__FROM_ADDRESS` | required | no | Address, or `Name <address>`, that mail is sent from. Must not be blank outside development. |
| `POSTIT__MAIL__SEND_EMAIL_MAX_ATTEMPTS` | `8` | no | Attempts made to send one email before the job fails for good. |
| `POSTIT__MAIL__SMTP__HOST` | required | no | SMTP server host. Must not be blank outside development. |
| `POSTIT__MAIL__SMTP__PORT` | required | no | SMTP server port. The shipped `qa.toml` and `production.toml` use 587. |
| `POSTIT__MAIL__SMTP__USERNAME` | unset | yes (not masked) | SMTP username, when the server requires authentication. |
| `POSTIT__MAIL__SMTP__PASSWORD` | unset | yes | SMTP password, when the server requires authentication. |
| `POSTIT__MAIL__SMTP__TLS` | `starttls` | no | TLS mode: `starttls` (upgrade a plain connection), `tls` (implicit TLS, usually port 465), or `none` (plain SMTP, development only). |

### jobs

The background job runtime. Concurrency is a map keyed by queue name, so each queue has its own variable.

| Variable | Default | Secret | Meaning |
| --- | --- | --- | --- |
| `POSTIT__JOBS__CONCURRENCY__MAIL` | `4` | no | Worker concurrency of the `mail` queue. |
| `POSTIT__JOBS__CONCURRENCY__MAINTENANCE` | `1` | no | Worker concurrency of the `maintenance` queue. |
| `POSTIT__JOBS__CONCURRENCY__DEFAULT` | `4` | no | Worker concurrency of the `default` queue. |
| `POSTIT__JOBS__OUTBOX_POLL_INTERVAL` | `5s` | no | How often the worker polls the job outbox. |
| `POSTIT__JOBS__HISTORY_RETENTION__SUCCEEDED` | `7days` | no | How long succeeded jobs are kept. |
| `POSTIT__JOBS__HISTORY_RETENTION__FAILED` | `30days` | no | How long failed jobs are kept. |
| `POSTIT__JOBS__SCHEDULES__JOB_HISTORY_PURGE` | `0 10 3 * * *` | no | Cron schedule of the job-history purge. |
| `POSTIT__JOBS__SCHEDULES__PURGE_PENDING_USERS` | `0 20 3 * * *` | no | Cron schedule of the pending-user purge. |
| `POSTIT__JOBS__SCHEDULES__AUDIT_RETENTION` | `0 30 3 * * *` | no | Cron schedule of the audit-log retention sweep. |
| `POSTIT__JOBS__SCHEDULES__DATA_RETENTION` | `0 40 3 * * *` | no | Cron schedule of the operational data retention sweep. |

Schedules use cron syntax with seconds first: second, minute, hour, day of month, month, day of week.

### http

The shared outbound HTTP client.

| Variable | Default | Secret | Meaning |
| --- | --- | --- | --- |
| `POSTIT__HTTP__CONNECT_TIMEOUT` | `5s` | no | TCP connect timeout. |
| `POSTIT__HTTP__REQUEST_TIMEOUT` | `30s` | no | Whole-request timeout. |
| `POSTIT__HTTP__USER_AGENT` | `postit/0.7.0` | no | `User-Agent` header sent on outbound requests. |
| `POSTIT__HTTP__EXTRA_CA_FILES` | empty list | no | Extra PEM CA bundles trusted in addition to the system roots. |

### app

| Variable | Default | Secret | Meaning |
| --- | --- | --- | --- |
| `POSTIT__APP__PUBLIC_URL` | required | no | Externally visible base URL of the app, used in emails and redirects. |

### cors

| Variable | Default | Secret | Meaning |
| --- | --- | --- | --- |
| `POSTIT__CORS__ALLOWED_ORIGINS` | empty list | no | Origins allowed to call the API from a browser. |
