use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

const MAX_MESSAGES: usize = 200;
const MAX_MESSAGE_CHARS: usize = 4_000;
const MAX_GRAPH_NODES: usize = 50_000;
const MAX_STORED_MESSAGES: usize = 1_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewRole {
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewMessage {
    pub role: PreviewRole,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PreviewDisplayLine {
    Role(PreviewRole),
    Text(String),
    Muted(String),
    Blank,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PreviewCache {
    pub width: u16,
    pub lines: Vec<PreviewDisplayLine>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionPreview {
    pub title: String,
    pub messages: Vec<PreviewMessage>,
    pub total_messages: usize,
    pub history_incomplete: bool,
    /// Wrapped rows away from the newest content.
    pub scroll_from_bottom: usize,
    pub(crate) cache: Option<PreviewCache>,
}

#[derive(Clone, Debug)]
struct RawMessage {
    role: PreviewRole,
    text: String,
    source_id: Option<String>,
}

#[derive(Clone, Debug)]
struct Node {
    parent: Option<String>,
    message: Option<RawMessage>,
    message_omitted: bool,
}

impl SessionPreview {
    pub fn load(title: String, path: &Path) -> io::Result<Self> {
        let file = File::open(path).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("failed to open session {}: {error}", path.display()),
            )
        })?;
        Self::from_reader(title, BufReader::new(file))
    }

    fn from_reader(title: String, reader: impl BufRead) -> io::Result<Self> {
        let mut nodes = HashMap::new();
        let mut node_order = VecDeque::new();
        let mut message_order = VecDeque::new();
        let mut chronological = VecDeque::new();
        let mut chronological_omitted = false;
        let mut active_leaf = None;

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };

            let event_type = value.get("type").and_then(Value::as_str);
            let sidechain = flag(&value, "isSidechain");
            let hidden = sidechain
                || flag(&value, "isMeta")
                || flag(&value, "isCompactSummary")
                || flag(&value, "isVisibleInTranscriptOnly");
            let role = match event_type {
                Some("user") => Some(PreviewRole::User),
                Some("assistant") => Some(PreviewRole::Assistant),
                _ => None,
            };
            let message = role
                .filter(|_| !hidden)
                .and_then(|role| raw_message(&value, role));
            if let Some(message) = &message {
                chronological_omitted |=
                    push_bounded(&mut chronological, message.clone(), MAX_STORED_MESSAGES);
            }

            let uuid = value.get("uuid").and_then(Value::as_str).map(str::to_owned);
            if !sidechain && role.is_some() {
                if let Some(uuid) = &uuid {
                    active_leaf = Some(uuid.clone());
                }
            }
            if let Some(uuid) = uuid {
                let parent = value
                    .get("parentUuid")
                    .and_then(Value::as_str)
                    .or_else(|| value.get("logicalParentUuid").and_then(Value::as_str))
                    .map(str::to_owned);
                store_node(
                    &mut nodes,
                    &mut node_order,
                    &mut message_order,
                    uuid,
                    Node {
                        parent,
                        message,
                        message_omitted: false,
                    },
                    MAX_GRAPH_NODES,
                    MAX_STORED_MESSAGES,
                );
            }
        }

        let (raw_messages, history_incomplete) = active_branch(
            &nodes,
            active_leaf.as_deref(),
            chronological,
            chronological_omitted,
        );
        let mut messages = merge_messages(raw_messages);
        let total_messages = messages.len();
        if messages.len() > MAX_MESSAGES {
            messages.drain(..messages.len() - MAX_MESSAGES);
        }

        Ok(Self {
            title,
            messages,
            total_messages,
            history_incomplete,
            scroll_from_bottom: 0,
            cache: None,
        })
    }
}

fn active_branch(
    nodes: &HashMap<String, Node>,
    active_leaf: Option<&str>,
    chronological: VecDeque<RawMessage>,
    chronological_omitted: bool,
) -> (Vec<RawMessage>, bool) {
    let Some(mut current) = active_leaf else {
        return (chronological.into(), chronological_omitted);
    };

    let mut branch = Vec::new();
    let mut visited = HashSet::new();
    let mut history_incomplete = false;

    loop {
        if !visited.insert(current.to_owned()) {
            history_incomplete = true;
            break;
        }
        let Some(node) = nodes.get(current) else {
            history_incomplete = true;
            break;
        };
        if let Some(message) = &node.message {
            branch.push(message.clone());
        }
        if node.message_omitted {
            history_incomplete = true;
        }
        let Some(parent) = node.parent.as_deref() else {
            break;
        };
        current = parent;
    }

    branch.reverse();
    if branch.is_empty() && !chronological.is_empty() {
        (
            chronological.into(),
            history_incomplete || chronological_omitted,
        )
    } else {
        (branch, history_incomplete)
    }
}

fn store_node(
    nodes: &mut HashMap<String, Node>,
    node_order: &mut VecDeque<String>,
    message_order: &mut VecDeque<String>,
    uuid: String,
    node: Node,
    max_nodes: usize,
    max_messages: usize,
) {
    if max_nodes == 0 {
        return;
    }
    if !nodes.contains_key(&uuid) {
        while nodes.len() >= max_nodes {
            let Some(oldest) = node_order.pop_front() else {
                break;
            };
            nodes.remove(&oldest);
        }
        node_order.push_back(uuid.clone());
    }

    let has_message = node.message.is_some();
    nodes.insert(uuid.clone(), node);
    if has_message && max_messages > 0 {
        message_order.push_back(uuid);
        while message_order.len() > max_messages {
            let Some(oldest) = message_order.pop_front() else {
                break;
            };
            if let Some(node) = nodes.get_mut(&oldest) {
                if node.message.take().is_some() {
                    node.message_omitted = true;
                }
            }
        }
    }
}

fn push_bounded<T>(items: &mut VecDeque<T>, item: T, limit: usize) -> bool {
    if limit == 0 {
        return true;
    }
    let omitted = items.len() == limit;
    if omitted {
        items.pop_front();
    }
    items.push_back(item);
    omitted
}

fn raw_message(value: &Value, role: PreviewRole) -> Option<RawMessage> {
    Some(RawMessage {
        role,
        text: message_text(value, role)?,
        source_id: value
            .pointer("/message/id")
            .and_then(Value::as_str)
            .or_else(|| value.get("uuid").and_then(Value::as_str))
            .map(str::to_owned),
    })
}

fn message_text(value: &Value, role: PreviewRole) -> Option<String> {
    let content = value.get("message")?.get("content")?;
    let parts = if let Some(text) = content.as_str() {
        filtered_text(text, role).into_iter().collect()
    } else {
        content
            .as_array()?
            .iter()
            .filter_map(|block| match block.get("type").and_then(Value::as_str) {
                Some("text") => block
                    .get("text")
                    .and_then(Value::as_str)
                    .and_then(|text| filtered_text(text, role)),
                Some("image") if role == PreviewRole::User => Some("[Image]".to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let text = parts.join("\n\n");
    (!text.is_empty()).then_some(truncate(&text, MAX_MESSAGE_CHARS))
}

fn filtered_text(text: &str, role: PreviewRole) -> Option<String> {
    let normalized = normalize_text(text);
    if normalized.is_empty() {
        return None;
    }
    if role == PreviewRole::User {
        const HIDDEN_PREFIXES: [&str; 5] = [
            "<local-command-caveat>",
            "<local-command-stdout>",
            "<system-reminder>",
            "<task-notification>",
            "<tool-result>",
        ];
        if HIDDEN_PREFIXES
            .iter()
            .any(|prefix| normalized.starts_with(prefix))
        {
            return None;
        }
        if let Some(command) = between(&normalized, "<command-name>", "</command-name>") {
            return Some(command.trim().to_string());
        }
    }
    Some(normalized)
}

fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\t', "    ")
        .chars()
        .filter(|character| !character.is_control() || *character == '\n')
        .collect::<String>()
        .trim()
        .to_string()
}

fn between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let start_index = text.find(start)? + start.len();
    let end_index = text[start_index..].find(end)? + start_index;
    Some(&text[start_index..end_index])
}

fn merge_messages(raw_messages: Vec<RawMessage>) -> Vec<PreviewMessage> {
    let mut messages: Vec<PreviewMessage> = Vec::new();
    let mut previous_source_id: Option<String> = None;

    for raw in raw_messages {
        let can_merge = raw.source_id.is_some()
            && raw.source_id == previous_source_id
            && messages
                .last()
                .is_some_and(|message| message.role == raw.role);
        if can_merge {
            if let Some(message) = messages.last_mut() {
                append_text(&mut message.text, &raw.text);
            }
        } else {
            messages.push(PreviewMessage {
                role: raw.role,
                text: raw.text,
            });
        }
        previous_source_id = raw.source_id;
    }
    messages
}

fn append_text(current: &mut String, addition: &str) {
    if current.chars().count() >= MAX_MESSAGE_CHARS {
        return;
    }
    if !current.is_empty() {
        current.push_str("\n\n");
    }
    current.push_str(addition);
    *current = truncate(current, MAX_MESSAGE_CHARS);
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_owned()
    } else {
        let mut truncated: String = text.chars().take(max_chars.saturating_sub(1)).collect();
        truncated.push('…');
        truncated
    }
}

fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn parse(lines: &[Value]) -> SessionPreview {
        let source = lines
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        SessionPreview::from_reader("Test session".to_string(), Cursor::new(source))
            .expect("preview should parse")
    }

    fn user(uuid: &str, parent: Option<&str>, text: &str) -> Value {
        serde_json::json!({
            "type": "user",
            "uuid": uuid,
            "parentUuid": parent,
            "message": {"content": text}
        })
    }

    fn assistant(uuid: &str, parent: Option<&str>, message_id: &str, block: Value) -> Value {
        serde_json::json!({
            "type": "assistant",
            "uuid": uuid,
            "parentUuid": parent,
            "message": {"id": message_id, "content": [block]}
        })
    }

    #[test]
    fn extracts_visible_text_and_merges_assistant_blocks() {
        let preview = parse(&[
            user("u1", None, "Build the feature"),
            assistant(
                "a1",
                Some("u1"),
                "response-1",
                serde_json::json!({"type": "thinking", "thinking": "private"}),
            ),
            assistant(
                "a2",
                Some("a1"),
                "response-1",
                serde_json::json!({"type": "text", "text": "I’ll inspect it."}),
            ),
            assistant(
                "a3",
                Some("a2"),
                "response-1",
                serde_json::json!({"type": "tool_use", "name": "Read"}),
            ),
            assistant(
                "a4",
                Some("a3"),
                "response-1",
                serde_json::json!({"type": "text", "text": "The change is ready."}),
            ),
            serde_json::json!({
                "type": "user",
                "uuid": "tool-result",
                "parentUuid": "a4",
                "message": {"content": [{"type": "tool_result", "content": "secret output"}]}
            }),
        ]);

        assert_eq!(
            preview.messages,
            vec![
                PreviewMessage {
                    role: PreviewRole::User,
                    text: "Build the feature".to_string(),
                },
                PreviewMessage {
                    role: PreviewRole::Assistant,
                    text: "I’ll inspect it.\n\nThe change is ready.".to_string(),
                },
            ]
        );
    }

    #[test]
    fn follows_the_active_branch_and_excludes_rewound_history() {
        let preview = parse(&[
            user("root", None, "Start"),
            user("abandoned", Some("root"), "Old direction"),
            assistant(
                "old-answer",
                Some("abandoned"),
                "old",
                serde_json::json!({"type": "text", "text": "Old answer"}),
            ),
            user("active", Some("root"), "New direction"),
            assistant(
                "new-answer",
                Some("active"),
                "new",
                serde_json::json!({"type": "text", "text": "New answer"}),
            ),
        ]);

        let text = preview
            .messages
            .iter()
            .map(|message| message.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(text.contains("Start"));
        assert!(text.contains("New direction"));
        assert!(text.contains("New answer"));
        assert!(!text.contains("Old direction"));
        assert!(!text.contains("Old answer"));
    }

    #[test]
    fn logical_parent_reconnects_compacted_history() {
        let preview = parse(&[
            user("before", None, "Before compaction"),
            serde_json::json!({
                "type": "user",
                "uuid": "after",
                "parentUuid": null,
                "logicalParentUuid": "before",
                "isCompactSummary": true,
                "message": {"content": "hidden summary"}
            }),
            assistant(
                "answer",
                Some("after"),
                "answer",
                serde_json::json!({"type": "text", "text": "After compaction"}),
            ),
        ]);

        assert_eq!(
            preview
                .messages
                .iter()
                .map(|message| message.text.as_str())
                .collect::<Vec<_>>(),
            vec!["Before compaction", "After compaction"]
        );
    }

    #[test]
    fn hides_internal_records_but_keeps_commands_and_image_markers() {
        let preview = parse(&[
            user("u1", None, "<system-reminder>hidden</system-reminder>"),
            user(
                "u2",
                Some("u1"),
                "<command-name>/review</command-name><command-message>ignored</command-message>",
            ),
            serde_json::json!({
                "type": "user",
                "uuid": "u3",
                "parentUuid": "u2",
                "message": {"content": [
                    {"type": "text", "text": "Look at this"},
                    {"type": "image", "source": {"data": "not retained"}}
                ]}
            }),
        ]);

        assert_eq!(
            preview
                .messages
                .iter()
                .map(|message| message.text.as_str())
                .collect::<Vec<_>>(),
            vec!["/review", "Look at this\n\n[Image]"]
        );
    }

    #[test]
    fn falls_back_to_chronological_order_for_legacy_files() {
        let source = concat!(
            "not json\n",
            "{\"type\":\"user\",\"message\":{\"content\":\"first\"}}\n",
            "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"second\"}]}}\n"
        );
        let preview = SessionPreview::from_reader("Test".to_string(), Cursor::new(source)).unwrap();

        assert_eq!(
            preview
                .messages
                .iter()
                .map(|message| message.text.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );
    }

    #[test]
    fn marks_missing_or_cyclic_history_without_hanging() {
        let missing = parse(&[user("u1", Some("missing"), "visible tail")]);
        assert!(missing.history_incomplete);
        assert_eq!(missing.messages[0].text, "visible tail");

        let cyclic = parse(&[user("u1", Some("u2"), "one"), user("u2", Some("u1"), "two")]);
        assert!(cyclic.history_incomplete);
        assert_eq!(cyclic.messages.len(), 2);
    }

    #[test]
    fn retains_only_the_most_recent_messages() {
        let lines: Vec<Value> = (0..MAX_MESSAGES + 5)
            .map(|index| {
                user(
                    &format!("user-{index}"),
                    (index > 0)
                        .then(|| format!("user-{}", index - 1))
                        .as_deref(),
                    &format!("message {index}"),
                )
            })
            .collect();
        let preview = parse(&lines);

        assert_eq!(preview.messages.len(), MAX_MESSAGES);
        assert_eq!(preview.total_messages, MAX_MESSAGES + 5);
        assert_eq!(preview.messages.first().unwrap().text, "message 5");
    }

    #[test]
    fn caps_individual_message_size() {
        let preview = parse(&[user("u1", None, &"x".repeat(MAX_MESSAGE_CHARS + 100))]);

        assert_eq!(preview.messages[0].text.chars().count(), MAX_MESSAGE_CHARS);
        assert!(preview.messages[0].text.ends_with('…'));
    }

    #[test]
    fn bounds_graph_and_message_retention_without_losing_the_newest_branch() {
        let mut nodes = HashMap::new();
        let mut node_order = VecDeque::new();
        let mut message_order = VecDeque::new();

        for index in 0..4 {
            let uuid = format!("node-{index}");
            store_node(
                &mut nodes,
                &mut node_order,
                &mut message_order,
                uuid,
                Node {
                    parent: (index > 0).then(|| format!("node-{}", index - 1)),
                    message: Some(RawMessage {
                        role: PreviewRole::User,
                        text: format!("message {index}"),
                        source_id: Some(format!("source-{index}")),
                    }),
                    message_omitted: false,
                },
                3,
                2,
            );
        }

        assert_eq!(nodes.len(), 3);
        assert!(!nodes.contains_key("node-0"));
        assert!(nodes["node-1"].message.is_none());
        assert!(nodes["node-1"].message_omitted);

        let (branch, incomplete) = active_branch(&nodes, Some("node-3"), VecDeque::new(), false);
        assert!(incomplete);
        assert_eq!(
            branch
                .iter()
                .map(|message| message.text.as_str())
                .collect::<Vec<_>>(),
            vec!["message 2", "message 3"]
        );
    }
}
