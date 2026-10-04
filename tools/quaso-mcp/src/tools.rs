use serde_json::{Value, json};
use std::collections::HashMap;

// MCP tool names travel through clients that only accept letters, digits,
// underscores and dashes, and a quaso command name is dotted. The dot becomes an
// underscore on the way out, and the catalog remembers the way back.
pub fn to_tool_name(command: &str) -> String {
    command.replace('.', "_")
}

// Used only until a game connects, so an agent sees what this game engine can do
// even when nothing runs. The live list from a connected game replaces this
// wholesale, schemas and all, so a description here that has drifted is
// corrected the moment a game starts.
const BUILT_INS: &[(&str, &str, bool)] = &[
    (
        "commands.list",
        "List every command the running game answers.",
        true,
    ),
    (
        "render.capture",
        "Capture a rendered frame as a PNG image.",
        true,
    ),
    (
        "time.status",
        "Report the clock: paused state, time scale and step counters.",
        true,
    ),
    ("time.pause", "Stop game time.", false),
    ("time.resume", "Let game time run again.", false),
    (
        "time.step",
        "Run a set number of fixed update steps, which works while paused.",
        false,
    ),
    (
        "time.set_scale",
        "Set how fast game time runs. 1 is normal speed.",
        false,
    ),
    (
        "ui.tree",
        "Report the GUI widget tree with the screen rect of every widget.",
        true,
    ),
    ("ui.text", "Report every text the GUI draws.", true),
    (
        "input.mappings",
        "Report the input mapping stack and what each mapping binds.",
        true,
    ),
    (
        "input.status",
        "Report which injected actions and axes are still held.",
        true,
    ),
    (
        "input.press",
        "Hold an input action, as if the player pressed it.",
        false,
    ),
    ("input.release", "Release an input action.", false),
    ("input.axis", "Set an input axis to a value.", false),
    (
        "input.clear",
        "Drop every injected action and axis at once.",
        false,
    ),
    ("editor.status", "Report whether the editor edits.", true),
    (
        "editor.set_editing",
        "Enter or leave editor edit mode.",
        false,
    ),
    ("app.close", "Ask the window to close.", false),
];

pub struct Catalog {
    tools: Vec<Value>,
    commands: HashMap<String, String>,
}

impl Default for Catalog {
    fn default() -> Self {
        let mut result = Self {
            tools: Default::default(),
            commands: Default::default(),
        };
        for (command, description, read_only) in BUILT_INS {
            let name = to_tool_name(command);
            result.commands.insert(name.clone(), (*command).to_owned());
            result.tools.push(json!({
                "name": name,
                "description": format!("{description} (No game is running, so this description may be out of date and the arguments are not checked.)"),
                "inputSchema": { "type": "object" },
                "annotations": { "readOnlyHint": read_only, "openWorldHint": false },
            }));
        }
        result
    }
}

impl Catalog {
    pub fn from_game(answer: &Value) -> Result<Self, String> {
        let commands = answer
            .get("commands")
            .and_then(|value| value.as_array())
            .ok_or_else(|| "The game did not report a command list".to_owned())?;
        let mut result = Self {
            tools: Vec::with_capacity(commands.len()),
            commands: HashMap::with_capacity(commands.len()),
        };
        for command in commands {
            let Some(source) = command.get("name").and_then(|value| value.as_str()) else {
                continue;
            };
            let name = to_tool_name(source);
            // A game that registers both `a.b` and `a_b` would map both onto one
            // tool name. The first one keeps the name, because silently pointing
            // a tool at the wrong command is worse than one missing tool.
            if result.commands.contains_key(&name) {
                crate::rpc::log(format!(
                    "two commands map onto the tool name `{name}`, so `{source}` is not exposed"
                ));
                continue;
            }
            let mut tool = command.clone();
            if let Some(object) = tool.as_object_mut() {
                object.insert("name".to_owned(), json!(name));
            }
            result.commands.insert(name, source.to_owned());
            result.tools.push(tool);
        }
        Ok(result)
    }

    pub fn tools(&self) -> &[Value] {
        &self.tools
    }

    pub fn command(&self, tool: &str) -> Option<&str> {
        self.commands.get(tool).map(|name| name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::{Catalog, to_tool_name};
    use serde_json::json;

    #[test]
    fn test_a_dotted_command_becomes_a_legal_tool_name() {
        assert_eq!(to_tool_name("time.set_scale"), "time_set_scale");
        assert_eq!(to_tool_name("app.close"), "app_close");
        assert_eq!(to_tool_name("plain"), "plain");
    }

    #[test]
    fn test_every_fallback_tool_name_is_legal_for_mcp() {
        let catalog = Catalog::default();

        assert!(!catalog.tools().is_empty());
        for tool in catalog.tools() {
            let name = tool["name"].as_str().unwrap();
            assert!(
                name.chars()
                    .all(|character| character.is_ascii_alphanumeric()
                        || character == '_'
                        || character == '-'),
                "`{name}` is not a legal MCP tool name"
            );
            assert!(name.len() <= 64);
        }
    }

    #[test]
    fn test_the_fallback_maps_back_to_the_dotted_command() {
        let catalog = Catalog::default();

        assert_eq!(catalog.command("time_pause"), Some("time.pause"));
        assert_eq!(catalog.command("time.pause"), None);
    }

    #[test]
    fn test_a_live_list_keeps_the_schema_and_renames_the_tool() {
        let catalog = Catalog::from_game(&json!({
            "commands": [{
                "name": "game.player",
                "description": "Read the player.",
                "inputSchema": { "type": "object", "additionalProperties": false },
                "annotations": { "readOnlyHint": true },
            }],
        }))
        .unwrap();

        assert_eq!(catalog.tools().len(), 1);
        assert_eq!(catalog.tools()[0]["name"], json!("game_player"));
        assert_eq!(
            catalog.tools()[0]["inputSchema"]["additionalProperties"],
            json!(false)
        );
        assert_eq!(catalog.command("game_player"), Some("game.player"));
    }

    #[test]
    fn test_two_commands_that_collide_expose_only_the_first() {
        let catalog = Catalog::from_game(&json!({
            "commands": [
                { "name": "a.b", "description": "first" },
                { "name": "a_b", "description": "second" },
            ],
        }))
        .unwrap();

        assert_eq!(catalog.tools().len(), 1);
        assert_eq!(catalog.command("a_b"), Some("a.b"));
    }

    #[test]
    fn test_an_answer_without_a_command_list_is_refused() {
        assert!(Catalog::from_game(&json!({})).is_err());
        assert!(Catalog::from_game(&json!({ "commands": 7 })).is_err());
    }
}
