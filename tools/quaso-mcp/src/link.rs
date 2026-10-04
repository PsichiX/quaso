use crate::rpc::log;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, ErrorKind, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, Sender, channel},
    },
    thread,
    time::Duration,
};

pub const PROTOCOL_VERSION: u32 = 1;
pub const DEFAULT_PORT: u16 = 17771;

const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
const RETRY_PAUSE: Duration = Duration::from_millis(500);
const IDLE_POLL: Duration = Duration::from_millis(200);
const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkEvent {
    Connected,
    Disconnected,
}

struct Call {
    name: String,
    arguments: Value,
    reply: Sender<Result<Value, String>>,
}

pub struct GameLink {
    calls: Sender<Call>,
    connected: Arc<AtomicBool>,
}

impl GameLink {
    pub fn start(port: u16, on_event: impl Fn(LinkEvent) + Send + 'static) -> Self {
        let (calls, requests) = channel();
        let connected = Arc::new(AtomicBool::new(false));
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let flag = connected.clone();
        thread::spawn(move || run(address, requests, flag, on_event));
        Self { calls, connected }
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub fn call(&self, name: &str, arguments: Value) -> Result<Value, String> {
        let (reply, answer) = channel();
        self.calls
            .send(Call {
                name: name.to_owned(),
                arguments,
                reply,
            })
            .map_err(|_| "The link to the game stopped".to_owned())?;
        answer
            .recv()
            .map_err(|_| "The link to the game stopped".to_owned())?
    }
}

struct Connection {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

impl Connection {
    fn open(address: SocketAddr) -> Result<Self, String> {
        let stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)
            .map_err(|error| error.to_string())?;
        let _ = stream.set_nodelay(true);
        let writer = stream.try_clone().map_err(|error| error.to_string())?;
        let mut result = Self {
            reader: BufReader::new(stream),
            writer,
        };
        let banner = result.read_line(ANSWER_TIMEOUT)?;
        let banner = serde_json::from_str::<Value>(&banner)
            .map_err(|error| format!("the banner is not JSON: {error}"))?;
        match banner.get("protocol").and_then(|value| value.as_u64()) {
            Some(version) if version == PROTOCOL_VERSION as u64 => {}
            Some(version) => {
                return Err(format!(
                    "the game speaks protocol {version}, this adapter speaks {PROTOCOL_VERSION}"
                ));
            }
            None => return Err("whatever answered is not a quaso game".to_owned()),
        }
        log(format!(
            "connected to quaso {}",
            banner
                .get("quaso")
                .and_then(|value| value.as_str())
                .unwrap_or("of an unknown version")
        ));
        Ok(result)
    }

    fn read_line(&mut self, timeout: Duration) -> Result<String, String> {
        let _ = self.reader.get_ref().set_read_timeout(Some(timeout));
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => Err("the game closed the connection".to_owned()),
            Ok(_) => Ok(line),
            Err(error) => Err(error.to_string()),
        }
    }

    fn call(&mut self, name: &str, arguments: Value) -> Result<Value, String> {
        let request = json!({ "id": 1, "name": name, "arguments": arguments });
        let text = serde_json::to_string(&request).map_err(|error| error.to_string())?;
        self.writer
            .write_all(text.as_bytes())
            .and_then(|_| self.writer.write_all(b"\n"))
            .and_then(|_| self.writer.flush())
            .map_err(|error| error.to_string())?;
        let answer = self.read_line(ANSWER_TIMEOUT)?;
        serde_json::from_str::<Value>(&answer).map_err(|error| error.to_string())
    }

    // Reads nothing, and only asks whether the socket is still there. A game
    // that was closed is otherwise noticed on the next call, which can be many
    // minutes later, and the tool list would stay wrong until then.
    fn is_alive(&mut self) -> bool {
        let _ = self.reader.get_ref().set_read_timeout(Some(IDLE_POLL));
        match self.reader.fill_buf() {
            Ok(buffer) => !buffer.is_empty(),
            Err(error) => matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut),
        }
    }
}

fn run(
    address: SocketAddr,
    requests: Receiver<Call>,
    connected: Arc<AtomicBool>,
    on_event: impl Fn(LinkEvent),
) {
    let mut connection = None;
    let mut reported_failure = false;
    loop {
        let Some(open) = connection.as_mut() else {
            match Connection::open(address) {
                Ok(open) => {
                    connection = Some(open);
                    connected.store(true, Ordering::Relaxed);
                    reported_failure = false;
                    on_event(LinkEvent::Connected);
                }
                Err(error) => {
                    // Only the first failure of a run is worth a line. The game
                    // is usually just not started yet, and a line every half
                    // second would bury everything else.
                    if !reported_failure {
                        log(format!("waiting for a game on {address}: {error}"));
                        reported_failure = true;
                    }
                    match requests.recv_timeout(RETRY_PAUSE) {
                        Ok(call) => {
                            let _ = call.reply.send(Err(format!(
                                "No quaso game is listening on {address}. Start the game with the `agent` feature, then try again"
                            )));
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
            }
            continue;
        };

        match requests.recv_timeout(IDLE_POLL) {
            Ok(call) => match open.call(&call.name, call.arguments) {
                Ok(answer) => {
                    let _ = call.reply.send(read_answer(answer));
                }
                Err(error) => {
                    let _ = call
                        .reply
                        .send(Err(format!("The game stopped answering: {error}")));
                    connection = None;
                    connected.store(false, Ordering::Relaxed);
                    log(format!("lost the game: {error}"));
                    on_event(LinkEvent::Disconnected);
                }
            },
            Err(RecvTimeoutError::Timeout) => {
                if !open.is_alive() {
                    connection = None;
                    connected.store(false, Ordering::Relaxed);
                    log("the game closed the connection");
                    on_event(LinkEvent::Disconnected);
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn read_answer(answer: Value) -> Result<Value, String> {
    if answer.get("ok").and_then(|value| value.as_bool()) == Some(true) {
        return Ok(answer.get("result").cloned().unwrap_or(Value::Null));
    }
    Err(answer
        .get("error")
        .and_then(|value| value.as_str())
        .unwrap_or("The game refused the command and said nothing about why")
        .to_owned())
}

#[cfg(test)]
mod tests {
    use super::read_answer;
    use serde_json::json;

    #[test]
    fn test_an_ok_answer_becomes_the_result() {
        let answer = read_answer(json!({ "ok": true, "result": { "paused": true } }));

        assert_eq!(answer.unwrap(), json!({ "paused": true }));
    }

    #[test]
    fn test_a_refused_answer_becomes_the_error_text() {
        let answer = read_answer(json!({ "ok": false, "error": "no such command" }));

        assert_eq!(answer.unwrap_err(), "no such command");
    }

    #[test]
    fn test_an_answer_with_no_reason_still_reads_as_an_error() {
        assert!(read_answer(json!({ "ok": false })).is_err());
        assert!(read_answer(json!({})).is_err());
    }
}
