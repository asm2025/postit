//! A strict in-process SMTP server that records what it receives and refuses on request.
//!
//! It answers `503` to `MAIL` while a transaction is open, to `RCPT` before `MAIL`, and to
//! `DATA` with no accepted recipient, so a client that forgets `RSET` after a refusal fails.
//! Every refusal echoes the address it is about, as real servers often do, so tests can check
//! that no server text reaches a result.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

/// How long a stalled sink waits before answering: longer than any test runs.
const STALL: Duration = Duration::from_secs(600);

/// What the sink does. Change it with [`Sink::script`] before the client connects.
#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_excessive_bools)] // independent fault switches, not a state machine
pub struct Script {
    /// Never send the greeting.
    pub stall_greeting: bool,
    /// Never answer `MAIL`.
    pub stall_mail: bool,
    /// Require `AUTH PLAIN` with this username and password before `MAIL`.
    pub login: Option<(String, String)>,
    /// Refuse `RCPT` for an address with this code (such as `550 5.1.1`).
    pub rcpt: Vec<(String, String)>,
    /// Refuse the message after `DATA` for an address with this code.
    pub data: Vec<(String, String)>,
    /// Refuse every `MAIL` with this code once this many messages were accepted in total.
    pub mail_refusal: Option<(usize, String)>,
    /// Close the connection after this many accepted messages on it, for at most this many
    /// connections (`usize::MAX` for every one).
    pub drop_after: Option<(usize, usize)>,
    /// Close the connection after reading the whole message for an address, without a reply.
    pub drop_in_data: Vec<String>,
    /// Answer `AUTH` with this reply instead of checking the login.
    pub auth_reply: Option<String>,
    /// Leave `SMTPUTF8` out of the `EHLO` reply.
    pub hide_smtputf8: bool,
    /// Leave `8BITMIME` out of the `EHLO` reply.
    pub hide_8bitmime: bool,
    /// Offer `STARTTLS`, answer it with `220`, then send bytes that are not TLS.
    pub starttls_garbage: bool,
}

/// One accepted message.
#[derive(Debug, Clone)]
pub struct Received {
    /// Which connection carried it, from 1.
    pub connection: usize,
    /// The envelope sender.
    pub from: String,
    /// The envelope recipients.
    pub to: Vec<String>,
    /// `MAIL` parameters, such as `BODY=8BITMIME`.
    pub mail_params: String,
    /// The message, dot-unstuffed, with CRLF line ends.
    pub data: String,
}

impl Received {
    /// The value of header `name` (case-insensitive), unfolded, when present.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<String> {
        let head = self.data.split("\r\n\r\n").next().unwrap_or_default();
        let unfolded = head.replace("\r\n ", " ").replace("\r\n\t", " ");
        unfolded.split("\r\n").find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name)
                .then(|| value.trim().to_owned())
        })
    }
}

#[derive(Debug, Default)]
struct State {
    connections: usize,
    accepted: usize,
    drops: usize,
    received: Vec<Received>,
    commands: Vec<(usize, String)>,
    auths: usize,
    ehlo: Vec<String>,
    rcpts: Vec<(usize, String)>,
}

/// A running sink. It stops when the test's runtime ends.
#[derive(Clone)]
pub struct Sink {
    addr: SocketAddr,
    script: Arc<Mutex<Script>>,
    state: Arc<Mutex<State>>,
}

impl Sink {
    /// Starts a sink on a loopback port with `script`.
    ///
    /// # Panics
    ///
    /// When no loopback port can be bound.
    pub async fn start(script: Script) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let sink = Self {
            addr: listener.local_addr().unwrap(),
            script: Arc::new(Mutex::new(script)),
            state: Arc::new(Mutex::new(State::default())),
        };
        let accepting = sink.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let sink = accepting.clone();
                tokio::spawn(async move { sink.serve(stream).await });
            }
        });
        sink
    }

    /// The address it listens on.
    #[must_use]
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The port it listens on.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// Changes the script for connections from now on.
    pub fn script(&self, change: impl FnOnce(&mut Script)) {
        change(&mut self.script.lock().unwrap());
    }

    /// Every accepted message, in order.
    #[must_use]
    pub fn received(&self) -> Vec<Received> {
        self.state.lock().unwrap().received.clone()
    }

    /// How many connections were accepted.
    #[must_use]
    pub fn connections(&self) -> usize {
        self.state.lock().unwrap().connections
    }

    /// Every command verb received, with its connection, in order.
    #[must_use]
    pub fn commands(&self) -> Vec<(usize, String)> {
        self.state.lock().unwrap().commands.clone()
    }

    /// The name of every `EHLO`, in order.
    #[must_use]
    pub fn ehlo_names(&self) -> Vec<String> {
        self.state.lock().unwrap().ehlo.clone()
    }

    /// Every `RCPT` address, with its connection, in order, whatever the reply.
    #[must_use]
    pub fn rcpts(&self) -> Vec<(usize, String)> {
        self.state.lock().unwrap().rcpts.clone()
    }

    /// How many `AUTH` attempts succeeded.
    #[must_use]
    pub fn auths(&self) -> usize {
        self.state.lock().unwrap().auths
    }

    #[allow(clippy::too_many_lines)] // one SMTP dialogue, verb by verb
    async fn serve(&self, stream: TcpStream) {
        let connection = {
            let mut state = self.state.lock().unwrap();
            state.connections += 1;
            state.connections
        };
        let script = self.script.lock().unwrap().clone();
        if script.stall_greeting {
            tokio::time::sleep(STALL).await;
            return;
        }
        let (read, mut write) = stream.into_split();
        let mut lines = BufReader::new(read).lines();
        if write.write_all(b"220 sink ESMTP\r\n").await.is_err() {
            return;
        }
        let mut authed = script.login.is_none();
        let mut from: Option<(String, String)> = None;
        let mut to: Vec<String> = Vec::new();
        let mut on_this_connection = 0;
        while let Ok(Some(line)) = lines.next_line().await {
            let verb = line
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_ascii_uppercase();
            self.state
                .lock()
                .unwrap()
                .commands
                .push((connection, verb.clone()));
            let reply = match verb.as_str() {
                "EHLO" | "HELO" => {
                    let name = line.split_whitespace().nth(1).unwrap_or_default();
                    self.state.lock().unwrap().ehlo.push(name.to_owned());
                    let mut reply = String::from("250-sink\r\n");
                    if !script.hide_8bitmime {
                        reply.push_str("250-8BITMIME\r\n");
                    }
                    if !script.hide_smtputf8 {
                        reply.push_str("250-SMTPUTF8\r\n");
                    }
                    if script.starttls_garbage {
                        reply.push_str("250-STARTTLS\r\n");
                    }
                    if script.login.is_some() {
                        reply.push_str("250-AUTH PLAIN LOGIN\r\n");
                    }
                    reply.push_str("250 SIZE 10000000\r\n");
                    reply
                }
                "STARTTLS" if script.starttls_garbage => {
                    if write.write_all(b"220 2.0.0 ready\r\n").await.is_err() {
                        return;
                    }
                    // Wait for the client's hello, then answer with something that is not TLS.
                    let mut hello = [0_u8; 512];
                    let _ = lines.get_mut().read(&mut hello).await;
                    let _ = write.write_all(b"this is not a TLS record\r\n").await;
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    return;
                }
                "AUTH" if script.auth_reply.is_some() => {
                    format!("{}\r\n", script.auth_reply.clone().unwrap_or_default())
                }
                "AUTH" => {
                    let given = line.split_whitespace().nth(2).unwrap_or_default();
                    let decoded = base64::engine::general_purpose::STANDARD
                        .decode(given)
                        .unwrap_or_default();
                    let ok = script.login.as_ref().is_some_and(|(user, pass)| {
                        decoded == format!("\0{user}\0{pass}").into_bytes()
                    });
                    if ok {
                        authed = true;
                        self.state.lock().unwrap().auths += 1;
                        "235 2.7.0 accepted\r\n".to_owned()
                    } else {
                        "535 5.7.8 bad credentials\r\n".to_owned()
                    }
                }
                "MAIL" => {
                    if script.stall_mail {
                        tokio::time::sleep(STALL).await;
                        return;
                    }
                    let address = angle(&line);
                    let accepted = self.state.lock().unwrap().accepted;
                    if !authed {
                        "530 5.7.0 authentication required\r\n".to_owned()
                    } else if from.is_some() {
                        "503 5.5.1 nested MAIL command\r\n".to_owned()
                    } else if let Some((_, code)) = script
                        .mail_refusal
                        .as_ref()
                        .filter(|(after, _)| accepted >= *after)
                    {
                        format!("{code} sender <{address}> refused for {address}\r\n")
                    } else {
                        let params = line
                            .split_once('>')
                            .map(|(_, rest)| rest.trim().to_owned())
                            .unwrap_or_default();
                        from = Some((address, params));
                        "250 2.1.0 ok\r\n".to_owned()
                    }
                }
                "RCPT" => {
                    let address = angle(&line);
                    self.state
                        .lock()
                        .unwrap()
                        .rcpts
                        .push((connection, address.clone()));
                    if from.is_none() {
                        "503 5.5.1 need MAIL first\r\n".to_owned()
                    } else if let Some((_, code)) = script.rcpt.iter().find(|(a, _)| *a == address)
                    {
                        format!("{code} <{address}> refused: {address} unknown\r\n")
                    } else {
                        to.push(address);
                        "250 2.1.5 ok\r\n".to_owned()
                    }
                }
                "DATA" => {
                    if to.is_empty() {
                        "503 5.5.1 no valid recipients\r\n".to_owned()
                    } else {
                        if write.write_all(b"354 go ahead\r\n").await.is_err() {
                            return;
                        }
                        let mut data = String::new();
                        loop {
                            let Ok(Some(body)) = lines.next_line().await else {
                                return;
                            };
                            if body == "." {
                                break;
                            }
                            let body = body.strip_prefix('.').unwrap_or(&body);
                            data.push_str(body);
                            data.push_str("\r\n");
                        }
                        let (sender, mail_params) = from.take().unwrap_or_default();
                        let recipients = std::mem::take(&mut to);
                        if recipients.iter().any(|a| script.drop_in_data.contains(a)) {
                            let _ = write.shutdown().await;
                            return;
                        }
                        let refused = recipients.iter().find_map(|address| {
                            script
                                .data
                                .iter()
                                .find(|(a, _)| a == address)
                                .map(|(_, code)| (address.clone(), code.clone()))
                        });
                        if let Some((address, code)) = refused {
                            format!("{code} message for {address} refused\r\n")
                        } else {
                            on_this_connection += 1;
                            let drop_now = {
                                let mut state = self.state.lock().unwrap();
                                state.accepted += 1;
                                state.received.push(Received {
                                    connection,
                                    from: sender,
                                    to: recipients.clone(),
                                    mail_params,
                                    data,
                                });
                                let drop_now = script.drop_after.is_some_and(|(after, times)| {
                                    on_this_connection >= after && state.drops < times
                                });
                                if drop_now {
                                    state.drops += 1;
                                }
                                drop_now
                            };
                            if drop_now {
                                let _ = write.write_all(b"250 2.0.0 queued\r\n").await;
                                let _ = write.shutdown().await;
                                return;
                            }
                            format!("250 2.0.0 queued for {}\r\n", recipients.join(","))
                        }
                    }
                }
                "RSET" => {
                    from = None;
                    to.clear();
                    "250 2.0.0 reset\r\n".to_owned()
                }
                "NOOP" => "250 2.0.0 ok\r\n".to_owned(),
                "QUIT" => {
                    let _ = write.write_all(b"221 2.0.0 bye\r\n").await;
                    return;
                }
                _ => "502 5.5.2 unknown command\r\n".to_owned(),
            };
            if write.write_all(reply.as_bytes()).await.is_err() {
                return;
            }
        }
    }
}

/// The address between `<` and `>` in a `MAIL` or `RCPT` line.
fn angle(line: &str) -> String {
    line.split_once('<')
        .and_then(|(_, rest)| rest.split_once('>'))
        .map(|(address, _)| address.to_owned())
        .unwrap_or_default()
}

/// A loopback port nothing listens on.
///
/// # Panics
///
/// When no loopback port can be bound.
pub async fn closed_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}
