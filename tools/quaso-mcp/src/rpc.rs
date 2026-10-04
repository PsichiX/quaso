use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    io::{self, Write},
    sync::{Arc, Mutex},
};

pub const PARSE_ERROR: i64 = -32700;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;

#[derive(Debug, Deserialize)]
pub struct Request {
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

// stdout carries the protocol, so nothing else may write to it. Every log line
// in this crate goes to stderr. The lock keeps two threads from interleaving
// half a message, because the connection thread sends notifications while the
// main thread answers requests.
#[derive(Clone)]
pub struct Output {
    stdout: Arc<Mutex<io::Stdout>>,
}

impl Default for Output {
    fn default() -> Self {
        Self {
            stdout: Arc::new(Mutex::new(io::stdout())),
        }
    }
}

impl Output {
    pub fn send(&self, message: &Value) {
        let Ok(mut stdout) = self.stdout.lock() else {
            return;
        };
        let Ok(text) = serde_json::to_string(message) else {
            return;
        };
        let _ = stdout.write_all(text.as_bytes());
        let _ = stdout.write_all(b"\n");
        let _ = stdout.flush();
    }

    pub fn result(&self, id: Value, result: Value) {
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    }

    pub fn error(&self, id: Value, code: i64, message: impl AsRef<str>) {
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message.as_ref() },
        }));
    }

    pub fn notification(&self, method: &str, params: Value) {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }
}

pub fn log(message: impl AsRef<str>) {
    eprintln!("[quaso-mcp] {}", message.as_ref());
}
