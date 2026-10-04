use crate::commands::{GameCommandResponse, GameCommandsHandle};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, BufWriter, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    thread,
    time::Duration,
};

// The wire protocol is deliberately not MCP. The adapter binary holds every
// MCP specific decision, so a change in the MCP spec never forces a quaso
// release. What crosses this socket is one game command and one answer.
pub const PROTOCOL_VERSION: u32 = 1;

// Threat model: this port injects input and mutates game state, so it binds to
// the loopback address only, it is behind a non default feature, and it refuses
// to start in a release build. It trusts whatever connects to it.
pub struct AgentServerConfig {
    pub port: u16,
    pub timeout: Duration,
    pub allow_in_release: bool,
}

impl Default for AgentServerConfig {
    fn default() -> Self {
        Self {
            port: Self::DEFAULT_PORT,
            timeout: Duration::from_secs(10),
            allow_in_release: false,
        }
    }
}

impl AgentServerConfig {
    pub const DEFAULT_PORT: u16 = 17771;

    pub fn with_port(mut self, value: u16) -> Self {
        self.port = value;
        self
    }

    pub fn with_timeout(mut self, value: Duration) -> Self {
        self.timeout = value;
        self
    }

    pub fn with_allow_in_release(mut self, value: bool) -> Self {
        self.allow_in_release = value;
        self
    }
}

pub struct AgentServer {
    local_addr: SocketAddr,
}

impl AgentServer {
    pub fn start(handle: GameCommandsHandle, config: AgentServerConfig) -> Result<Self, String> {
        if let Some(error) = release_refusal(config.allow_in_release, !cfg!(debug_assertions)) {
            return Err(error);
        }
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, config.port))
            .map_err(|error| format!("Could not bind the agent server: {error}"))?;
        let local_addr = listener
            .local_addr()
            .map_err(|error| format!("Could not read the agent server address: {error}"))?;
        let timeout = config.timeout;
        // quaso installs no tracing subscriber, so a game that set none up would
        // never show this. An open control port is worth a line the game author
        // cannot miss, so it goes to stderr as well.
        eprintln!(
            "* Agent control server listens on {local_addr}. It can inject input and change game state"
        );
        tracing::event!(
            target: "quaso::agent",
            tracing::Level::WARN,
            "Agent control server listens on {}. It can inject input and change game state",
            local_addr
        );
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    continue;
                };
                let handle = handle.clone();
                thread::spawn(move || serve(stream, handle, timeout));
            }
        });
        Ok(Self { local_addr })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn port(&self) -> u16 {
        self.local_addr.port()
    }
}

// Split out from `AgentServer::start` so the rule can be tested. A test cannot
// change the profile it runs under, so the profile has to be an argument.
fn release_refusal(allow_in_release: bool, is_release_build: bool) -> Option<String> {
    if allow_in_release || !is_release_build {
        return None;
    }
    Some(
        concat!(
            "The agent server refuses to start in a release build. A released game would carry ",
            "an open control port. Set `allow_in_release` if this build is a tool, not a game ",
            "you ship",
        )
        .to_owned(),
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentRequest {
    #[serde(default)]
    id: Option<Value>,
    name: String,
    #[serde(default)]
    arguments: Value,
}

fn serve(stream: TcpStream, handle: GameCommandsHandle, timeout: Duration) {
    let _ = stream.set_nodelay(true);
    let Ok(reading) = stream.try_clone() else {
        return;
    };
    let mut writer = BufWriter::new(stream);
    // The banner goes out before anything is read. A client that finds another
    // program on the port, or an older game, learns that from the first line
    // rather than from a command that answers nothing.
    let banner = json!({
        "quaso": env!("CARGO_PKG_VERSION"),
        "protocol": PROTOCOL_VERSION,
    });
    if write_line(&mut writer, &banner).is_err() {
        return;
    }
    for line in BufReader::new(reading).lines() {
        let Ok(line) = line else {
            break;
        };
        if line.trim().is_empty() {
            continue;
        }
        let answer = match serde_json::from_str::<AgentRequest>(&line) {
            Ok(request) => answer(&handle, request, timeout),
            Err(error) => json!({
                "id": Value::Null,
                "ok": false,
                "error": format!("Could not read the request: {error}"),
            }),
        };
        if write_line(&mut writer, &answer).is_err() {
            break;
        }
    }
}

fn answer(handle: &GameCommandsHandle, request: AgentRequest, timeout: Duration) -> Value {
    let id = request.id.unwrap_or(Value::Null);
    let name = request.name;
    let result = handle
        .call(&name, request.arguments)
        .and_then(|pending| pending.take_timeout(timeout))
        .map(|response: GameCommandResponse| response.result);
    match result {
        Ok(Ok(value)) => json!({ "id": id, "name": name, "ok": true, "result": value }),
        Ok(Err(error)) => json!({ "id": id, "name": name, "ok": false, "error": error }),
        Err(error) => json!({ "id": id, "name": name, "ok": false, "error": error }),
    }
}

fn write_line(writer: &mut BufWriter<TcpStream>, value: &Value) -> Result<(), std::io::Error> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::{AgentServer, AgentServerConfig, PROTOCOL_VERSION, release_refusal};
    use crate::commands::GameCommands;
    use serde_json::{Value, json};
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpStream,
        time::Duration,
    };

    struct Client {
        reader: BufReader<TcpStream>,
        writer: TcpStream,
    }

    impl Client {
        fn connect(server: &AgentServer) -> Self {
            let stream = TcpStream::connect(server.local_addr()).unwrap();
            Self {
                reader: BufReader::new(stream.try_clone().unwrap()),
                writer: stream,
            }
        }

        fn read(&mut self) -> Value {
            let mut line = String::new();
            self.reader.read_line(&mut line).unwrap();
            serde_json::from_str(&line).unwrap()
        }

        fn write(&mut self, value: Value) {
            writeln!(self.writer, "{value}").unwrap();
            self.writer.flush().unwrap();
        }
    }

    fn server(commands: &GameCommands) -> AgentServer {
        AgentServer::start(
            commands.handle(),
            AgentServerConfig::default()
                .with_port(0)
                .with_timeout(Duration::from_millis(250))
                .with_allow_in_release(true),
        )
        .unwrap()
    }

    #[test]
    fn test_a_release_build_refuses_to_open_the_port_unless_it_is_told_to() {
        assert!(release_refusal(false, true).is_some());
        assert!(release_refusal(true, true).is_none());
        assert!(release_refusal(false, false).is_none());
        assert!(release_refusal(true, false).is_none());
    }

    #[test]
    fn test_the_default_config_keeps_the_port_out_of_a_release_build() {
        let config = AgentServerConfig::default();

        assert!(!config.allow_in_release);
        assert_eq!(config.port, AgentServerConfig::DEFAULT_PORT);
    }

    #[test]
    fn test_the_server_binds_to_loopback_only() {
        let commands = GameCommands::default();
        let server = server(&commands);

        assert!(server.local_addr().ip().is_loopback());
        assert_ne!(server.port(), 0);
    }

    #[test]
    fn test_a_client_reads_the_banner_before_it_sends_anything() {
        let commands = GameCommands::default();
        let server = server(&commands);
        let mut client = Client::connect(&server);
        let banner = client.read();

        assert_eq!(banner["protocol"], json!(PROTOCOL_VERSION));
        assert_eq!(banner["quaso"], json!(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn test_a_request_reaches_the_command_queue() {
        let commands = GameCommands::default();
        let server = server(&commands);
        let mut client = Client::connect(&server);
        client.read();
        client.write(json!({ "id": 1, "name": "time.pause" }));

        // Nothing runs a frame in a test, so the command can only be seen
        // waiting in the queue.
        let mut pending = 0;
        for _ in 0..100 {
            pending = commands.pending();
            if pending > 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(pending, 1);
    }

    #[test]
    fn test_a_game_that_never_answers_gives_the_client_an_error_not_a_hang() {
        let commands = GameCommands::default();
        let server = server(&commands);
        let mut client = Client::connect(&server);
        client.read();
        client.write(json!({ "id": 7, "name": "time.pause" }));
        let answer = client.read();

        assert_eq!(answer["id"], json!(7));
        assert_eq!(answer["name"], json!("time.pause"));
        assert_eq!(answer["ok"], json!(false));
        assert!(answer["error"].as_str().unwrap().contains("in time"));
    }

    #[test]
    fn test_a_broken_line_is_answered_and_the_connection_stays_open() {
        let commands = GameCommands::default();
        let server = server(&commands);
        let mut client = Client::connect(&server);
        client.read();
        client.write(json!("this is not a request"));
        let answer = client.read();

        assert_eq!(answer["ok"], json!(false));
        assert_eq!(answer["id"], Value::Null);

        client.write(json!({ "name": "time.pause", "nonsense": true }));
        let answer = client.read();

        assert_eq!(answer["ok"], json!(false));
        assert!(answer["error"].as_str().unwrap().contains("nonsense"));
    }

    #[test]
    fn test_many_clients_share_one_game() {
        let commands = GameCommands::default();
        let server = server(&commands);
        let mut first = Client::connect(&server);
        let mut second = Client::connect(&server);
        first.read();
        second.read();
        first.write(json!({ "name": "time.pause" }));
        second.write(json!({ "name": "time.resume" }));

        let mut pending = 0;
        for _ in 0..100 {
            pending = commands.pending();
            if pending >= 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(pending, 2);
    }
}
