use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Google Tasks の notes 末尾に埋め込んでいた旧メタデータ行のマーカー。
/// 現在はローカル保存に移行したため、書き込みには使わず、旧データの読み取り(移行)用に残す。
pub const META_MARKER: &str = "[todo-meta]";

fn default_score() -> u8 {
    5
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskMeta {
    #[serde(default = "default_score")]
    pub imp: u8, // 重要度 0-10
    #[serde(default = "default_score")]
    pub clau: u8, // clau度 0-10
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack: Option<StackCategory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack_note: Option<String>,
}

impl Default for TaskMeta {
    fn default() -> Self {
        Self {
            imp: 5,
            clau: 5,
            stack: None,
            stack_note: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StackCategory {
    #[serde(rename = "物")]
    Mono,
    #[serde(rename = "claude")]
    Claude,
    #[serde(rename = "人")]
    Hito,
    #[serde(rename = "その他")]
    Sonota,
}

impl StackCategory {
    pub const ALL: [StackCategory; 4] = [
        StackCategory::Mono,
        StackCategory::Claude,
        StackCategory::Hito,
        StackCategory::Sonota,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            StackCategory::Mono => "物",
            StackCategory::Claude => "claude",
            StackCategory::Hito => "人",
            StackCategory::Sonota => "その他",
        }
    }

    /// マトリックス上でラベル先頭に付ける種類マーク
    pub fn mark(&self) -> &'static str {
        match self {
            StackCategory::Mono => "(物)",
            StackCategory::Claude => "(C)",
            StackCategory::Hito => "(人)",
            StackCategory::Sonota => "(他)",
        }
    }
}

/// notes 全体を (ユーザー本文, メタ) に分解する。
/// `[todo-meta]` で始まる最後の行をメタとして採用し、パース失敗時は None。
pub fn parse_notes(notes: &str) -> (String, Option<TaskMeta>) {
    let mut meta = None;
    let mut body_lines: Vec<&str> = Vec::new();

    for line in notes.lines() {
        let trimmed = line.trim();
        if let Some(json) = trimmed.strip_prefix(META_MARKER) {
            if let Ok(m) = serde_json::from_str::<TaskMeta>(json.trim()) {
                meta = Some(m);
                continue; // メタ行は本文に含めない
            }
        }
        body_lines.push(line);
    }

    (body_lines.join("\n").trim_end().to_string(), meta)
}

/// ユーザー本文 + メタ -> notes 文字列。
/// メタはローカル保存に移行したため通常は使わない(テスト・旧フォーマット参照用)。
#[allow(dead_code)]
pub fn serialize_notes(user_body: &str, meta: &TaskMeta) -> String {
    let body = user_body.trim_end();
    let json = serde_json::to_string(meta).unwrap_or_else(|_| "{}".to_string());
    if body.is_empty() {
        format!("{} {}", META_MARKER, json)
    } else {
        format!("{}\n\n{} {}", body, META_MARKER, json)
    }
}

/// Option<String> の notes からメタを取り出す(無ければ None)。
/// 旧データ(notes 埋め込み)からローカルストアへ移行する際に使う。
pub fn meta_of(notes: Option<&str>) -> Option<TaskMeta> {
    notes.and_then(|n| parse_notes(n).1)
}

/// タスクのメタデータをローカル(`task_meta.json`)に保存するストア。
/// タスクID -> TaskMeta のマップを JSON で永続化し、Google Tasks には書き込まない。
#[derive(Debug, Default)]
pub struct MetaStore {
    map: HashMap<String, TaskMeta>,
    path: PathBuf,
}

impl MetaStore {
    /// 設定ディレクトリから読み込む。ファイルが無い・壊れている場合は空で開始する。
    pub fn load() -> Self {
        let path = crate::config::config_dir().join("task_meta.json");
        let map = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { map, path }
    }

    /// タスクのメタを取得(無ければデフォルト)。
    pub fn get(&self, task_id: &str) -> TaskMeta {
        self.map.get(task_id).cloned().unwrap_or_default()
    }

    pub fn contains(&self, task_id: &str) -> bool {
        self.map.contains_key(task_id)
    }

    pub fn set(&mut self, task_id: String, meta: TaskMeta) {
        self.map.insert(task_id, meta);
    }

    pub fn remove(&mut self, task_id: &str) {
        self.map.remove(task_id);
    }

    /// ローカルファイルへ書き出す。
    pub fn save(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.map)?;
        std::fs::write(&self.path, json)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_with_japanese_body() {
        let meta = TaskMeta {
            imp: 7,
            clau: 3,
            stack: Some(StackCategory::Claude),
            stack_note: Some("API仕様が不明で停止]中".to_string()),
        };
        let body = "これはメモです\n二行目もある";
        let notes = serialize_notes(body, &meta);
        let (parsed_body, parsed_meta) = parse_notes(&notes);
        assert_eq!(parsed_body, body);
        assert_eq!(parsed_meta, Some(meta));
    }

    #[test]
    fn roundtrip_empty_body() {
        let meta = TaskMeta::default();
        let notes = serialize_notes("", &meta);
        let (body, parsed) = parse_notes(&notes);
        assert_eq!(body, "");
        assert_eq!(parsed, Some(meta));
    }

    #[test]
    fn no_meta_returns_none() {
        let (body, meta) = parse_notes("ただのメモ");
        assert_eq!(body, "ただのメモ");
        assert_eq!(meta, None);
    }

    #[test]
    fn broken_json_degrades_to_body() {
        let notes = "メモ\n[todo-meta] {imp: 壊れてる";
        let (body, meta) = parse_notes(notes);
        assert_eq!(meta, None);
        assert!(body.contains("[todo-meta]"));
    }

    #[test]
    fn last_meta_line_wins() {
        let notes = "[todo-meta] {\"imp\":1,\"clau\":1}\n[todo-meta] {\"imp\":9,\"clau\":2}";
        let (_, meta) = parse_notes(notes);
        assert_eq!(meta.unwrap().imp, 9);
    }

    #[test]
    fn meta_update_preserves_body() {
        let notes = serialize_notes("本文", &TaskMeta::default());
        let (body, meta) = parse_notes(&notes);
        let mut meta = meta.unwrap();
        meta.stack = Some(StackCategory::Hito);
        let notes2 = serialize_notes(&body, &meta);
        let (body2, meta2) = parse_notes(&notes2);
        assert_eq!(body2, "本文");
        assert_eq!(meta2.unwrap().stack, Some(StackCategory::Hito));
    }
}
