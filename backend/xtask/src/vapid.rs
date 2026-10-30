//! `cargo xtask vapid-keygen`: a new VAPID key pair for `[push]`.
//!
//! Prints the vault line `POSTIT__PUSH__VAPID_PRIVATE_KEY=<base64url>` for
//! `!ref/vault/<environment>/postit.env`, and the public key browsers subscribe with (served at
//! `GET /api/v1/push/vapid-public-key`, so it needs no configuration). The private key is
//! printed once and stored nowhere else.

use secrecy::ExposeSecret as _;

pub fn run() -> anyhow::Result<()> {
    let (private, public) = postit_notify::vapid::generate_key_pair()?;
    println!(
        "POSTIT__PUSH__VAPID_PRIVATE_KEY={}",
        private.expose_secret()
    );
    eprintln!("# public key (served by the API): {public}");
    eprintln!(
        "# put the line above in !ref/vault/<environment>/postit.env; changing the key \
         invalidates every browser subscription"
    );
    Ok(())
}
