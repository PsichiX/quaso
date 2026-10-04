mod link;
mod rpc;
mod tools;

use crate::{
    link::{DEFAULT_PORT, GameLink, LinkEvent},
    rpc::{INVALID_PARAMS, METHOD_NOT_FOUND, Output, PARSE_ERROR, Request, log},
    tools::Catalog,
};
use serde_json::{Value, json};
use std::{
    io::{self, BufRead},
    process::ExitCode,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

// Echoed back to a client that asks for a version this adapter has no opinion
// about. The surface here is initialize, tools/list and tools/call, which every
// MCP revision so far spells the same way.
const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";

const USAGE: &str = "\
quaso-mcp - lets an agent drive a running quaso game over MCP.

Usage: quaso-mcp [--port <port>]

The adapter speaks MCP on stdin and stdout, and talks to the game on
127.0.0.1:<port>. It starts with no game running, it waits for one, and it
survives a game restart. Start the game with the `agent` feature to open the
port.

Options:
  --port <port>  Port the game listens on. Defaults to 17771.
  --help         Show this text.
";

fn main() -> ExitCode {
    let port = match read_port() {
        Ok(Some(port)) => port,
        Ok(None) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("{error}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    Server::new(port).run();
    ExitCode::SUCCESS
}

fn read_port() -> Result<Option<u16>, String> {
    let mut arguments = std::env::args().skip(1);
    let mut port = DEFAULT_PORT;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--help" | "-h" => return Ok(None),
            "--port" => {
                let value = arguments
                    .next()
                    .ok_or_else(|| "`--port` needs a port number".to_owned())?;
                port = value
                    .parse()
                    .map_err(|_| format!("`{value}` is not a port number"))?;
            }
            other => return Err(format!("`{other}` is not an option this adapter knows")),
        }
    }
    Ok(Some(port))
}

struct Server {
    output: Output,
    link: GameLink,
    catalog: Mutex<Catalog>,
    stale: Arc<AtomicBool>,
    initialized: Arc<AtomicBool>,
}

impl Server {
    fn new(port: u16) -> Self {
        let output = Output::default();
        let stale = Arc::new(AtomicBool::new(true));
        let initialized = Arc::new(AtomicBool::new(false));
        let link = GameLink::start(port, {
            let output = output.clone();
            let stale = stale.clone();
            let initialized = initialized.clone();
            // This runs on the link thread, which is the thread that answers
            // calls, so it must not call the game. It only marks the list stale,
            // and the client fetches the new one when it acts on the
            // notification.
            move |event| {
                stale.store(true, Ordering::Relaxed);
                log(match event {
                    LinkEvent::Connected => "a game connected, the tool list changed",
                    LinkEvent::Disconnected => "the game went away, the tool list is now stale",
                });
                if initialized.load(Ordering::Relaxed) {
                    output.notification("notifications/tools/list_changed", json!({}));
                }
            }
        });
        Self {
            output,
            link,
            catalog: Mutex::new(Catalog::default()),
            stale,
            initialized,
        }
    }

    fn run(&self) {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else {
                break;
            };
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Request>(&line) {
                Ok(request) => self.dispatch(request),
                Err(error) => {
                    // A message with no readable id can only be answered with a
                    // null id, which is what JSON-RPC asks for on a parse error.
                    self.output.error(
                        Value::Null,
                        PARSE_ERROR,
                        format!("Could not read the message: {error}"),
                    );
                }
            }
        }
    }

    fn dispatch(&self, request: Request) {
        let Some(id) = request.id else {
            // A notification takes no answer, whatever it says.
            if request.method == "notifications/initialized" {
                self.initialized.store(true, Ordering::Relaxed);
            }
            return;
        };
        match request.method.as_str() {
            "initialize" => {
                let version = request
                    .params
                    .get("protocolVersion")
                    .and_then(|value| value.as_str())
                    .unwrap_or(DEFAULT_PROTOCOL_VERSION);
                self.output.result(
                    id,
                    json!({
                        "protocolVersion": version,
                        "capabilities": { "tools": { "listChanged": true } },
                        "serverInfo": {
                            "name": "quaso-mcp",
                            "version": env!("CARGO_PKG_VERSION"),
                        },
                    }),
                );
                self.initialized.store(true, Ordering::Relaxed);
            }
            "ping" => self.output.result(id, json!({})),
            "tools/list" => {
                self.refresh();
                let tools = match self.catalog.lock() {
                    Ok(catalog) => catalog.tools().to_vec(),
                    Err(_) => Vec::new(),
                };
                self.output.result(id, json!({ "tools": tools }));
            }
            "tools/call" => self.call(id, request.params),
            other => self.output.error(
                id,
                METHOD_NOT_FOUND,
                format!("`{other}` is not a method this adapter answers"),
            ),
        }
    }

    // Only a connected game can say what its tools are. While nothing is
    // connected the last live list stays, because it still names the tools the
    // game will have when it comes back.
    fn refresh(&self) {
        if !self.link.is_connected() || !self.stale.swap(false, Ordering::Relaxed) {
            return;
        }
        match self
            .link
            .call("commands.list", json!({}))
            .and_then(|answer| Catalog::from_game(&answer))
        {
            Ok(catalog) => {
                if let Ok(mut held) = self.catalog.lock() {
                    *held = catalog;
                }
            }
            Err(error) => {
                log(format!("could not read the tool list: {error}"));
                self.stale.store(true, Ordering::Relaxed);
            }
        }
    }

    fn call(&self, id: Value, params: Value) {
        let Some(name) = params.get("name").and_then(|value| value.as_str()) else {
            self.output
                .error(id, INVALID_PARAMS, "`name` must name a tool");
            return;
        };
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
        let Some(command) = self.command_of(name) else {
            self.output.error(
                id,
                INVALID_PARAMS,
                format!("`{name}` is not a tool this game has"),
            );
            return;
        };
        // Past this point the failure belongs to the tool, not to the protocol.
        // An agent has to see it as a tool result it can act on, not as a
        // transport error, so it goes out as `isError` and not as a JSON-RPC
        // error.
        match self.link.call(&command, arguments) {
            Ok(result) => self.output.result(id, success_result(&result)),
            Err(error) => self.output.result(id, failure_result(&error)),
        }
    }

    fn command_of(&self, tool: &str) -> Option<String> {
        if let Ok(catalog) = self.catalog.lock()
            && let Some(command) = catalog.command(tool)
        {
            return Some(command.to_owned());
        }
        // A game can register a tool after the client last listed. Rather than
        // refuse a name that does exist, ask the game once more.
        self.stale.store(true, Ordering::Relaxed);
        self.refresh();
        let catalog = self.catalog.lock().ok()?;
        catalog.command(tool).map(|command| command.to_owned())
    }
}

// A captured frame is the one result worth sending as something other than
// text. As an image block the agent can actually look at it, and the base64
// never reaches the transcript as text.
fn image_content(result: &Value) -> Option<Value> {
    let mime_type = result.get("mime_type").and_then(|value| value.as_str())?;
    let data = result.get("data").and_then(|value| value.as_str())?;
    if !mime_type.starts_with("image/") {
        return None;
    }
    Some(json!({ "type": "image", "data": data, "mimeType": mime_type }))
}

fn success_result(result: &Value) -> Value {
    if let Some(image) = image_content(result) {
        let size = match (result.get("width"), result.get("height")) {
            (Some(width), Some(height)) => format!("{width}x{height} pixels"),
            _ => "a captured frame".to_owned(),
        };
        return json!({
            "content": [image, { "type": "text", "text": size }],
            "isError": false,
        });
    }
    let text = serde_json::to_string_pretty(result).unwrap_or_else(|_| result.to_string());
    let mut answer = json!({
        "content": [{ "type": "text", "text": text }],
        "isError": false,
    });
    if result.is_object()
        && let Some(object) = answer.as_object_mut()
    {
        object.insert("structuredContent".to_owned(), result.clone());
    }
    answer
}

fn failure_result(error: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": error }],
        "isError": true,
    })
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_PROTOCOL_VERSION, failure_result, image_content, success_result};
    use serde_json::json;

    #[test]
    fn test_a_captured_frame_goes_out_as_an_image_the_agent_can_see() {
        let answer = success_result(&json!({
            "mime_type": "image/png",
            "data": "aGVsbG8=",
            "width": 320,
            "height": 180,
        }));

        assert_eq!(answer["isError"], json!(false));
        assert_eq!(answer["content"][0]["type"], json!("image"));
        assert_eq!(answer["content"][0]["mimeType"], json!("image/png"));
        assert_eq!(answer["content"][0]["data"], json!("aGVsbG8="));
        assert_eq!(answer["content"][1]["text"], json!("320x180 pixels"));
        // The base64 must not be repeated as structured data.
        assert!(answer.get("structuredContent").is_none());
    }

    #[test]
    fn test_an_ordinary_result_goes_out_as_text_and_as_structured_data() {
        let answer = success_result(&json!({ "paused": true }));

        assert_eq!(answer["isError"], json!(false));
        assert_eq!(answer["content"][0]["type"], json!("text"));
        assert!(
            answer["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("paused")
        );
        assert_eq!(answer["structuredContent"], json!({ "paused": true }));
    }

    #[test]
    fn test_a_result_that_is_not_an_object_carries_no_structured_data() {
        let answer = success_result(&json!("pong"));

        assert_eq!(answer["content"][0]["type"], json!("text"));
        assert!(answer.get("structuredContent").is_none());
    }

    #[test]
    fn test_a_refused_command_is_a_tool_error_not_a_protocol_error() {
        let answer = failure_result("There is no command named: game.nope");

        assert_eq!(answer["isError"], json!(true));
        assert_eq!(
            answer["content"][0]["text"],
            json!("There is no command named: game.nope")
        );
    }

    #[test]
    fn test_only_an_image_mime_type_becomes_an_image_block() {
        assert!(image_content(&json!({ "mime_type": "image/png", "data": "x" })).is_some());
        assert!(image_content(&json!({ "mime_type": "text/plain", "data": "x" })).is_none());
        assert!(image_content(&json!({ "data": "x" })).is_none());
        assert!(image_content(&json!({ "mime_type": "image/png" })).is_none());
    }

    #[test]
    fn test_the_default_protocol_version_is_a_dated_revision() {
        assert!(DEFAULT_PROTOCOL_VERSION.starts_with("20"));
    }
}
