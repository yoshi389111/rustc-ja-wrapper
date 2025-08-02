use std::env;
use std::io::{self, Read, Write};
use std::process::{Command, Stdio, exit};

/// 翻訳データの型定義
#[derive(Debug, Clone, serde::Deserialize)]
pub struct TranslateEntry {
    pub en: String,
    pub ja: String,
}

/// JSONの翻訳データ（可変部分は "{$name}" や "{$ty}" などのプレースホルダを含む）
static TRANSLATE_LIST: once_cell::sync::Lazy<Vec<TranslateEntry>> =
    once_cell::sync::Lazy::new(|| {
        let json_str = include_str!("../assets/translate.json");
        // 英語文字列の長いものを先、短いものを後に並べ替える
        let mut entries: Vec<TranslateEntry> = serde_json::from_str(json_str).unwrap_or_default();
        entries.sort_by(|a, b| b.en.len().cmp(&a.en.len()));
        entries
    });

fn main() {
    let mut args = env::args_os().skip(1);
    let cmd: std::ffi::OsString = match args.next() {
        Some(c) => c,
        None => {
            eprintln!("Usage: rustc-ja-wrapper <command> [args...]");
            exit(1);
        }
    };

    let args_for_cmd: Vec<std::ffi::OsString> = args.collect();

    let child = Command::new(&cmd)
        .args(&args_for_cmd)
        // .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to spawn command: {}", e);
            exit(1);
        }
    };

    let mut stderr_buf = Vec::new();

    if let Some(mut err) = child.stderr.take() {
        if let Err(e) = err.read_to_end(&mut stderr_buf) {
            eprintln!("Failed to read stderr: {}", e);
            exit(1);
        }
    }

    if let Ok(s) = std::str::from_utf8(&stderr_buf) {
        append_debug_log("RESPONSE");
        append_debug_log(s);
    }

    // "--error-format=json" が含まれているか判定
    let error_format_json = std::ffi::OsStr::new("--error-format=json");
    let has_json_error_format = args_for_cmd.iter().any(|a| a == error_format_json);

    // 標準エラー出力変換処理
    if has_json_error_format {
        stderr_buf = convert_json_error_format(stderr_buf);
    }

    // 標準エラー出力に書き出す
    if let Err(e) = io::stderr().write_all(&stderr_buf) {
        eprintln!("Failed to write to stderr: {}", e);
        exit(1);
    }
    io::stderr().flush().unwrap_or_else(|e| {
        eprintln!("Failed to flush stderr: {}", e);
        exit(1);
    });

    let status = match child.wait() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to wait for child: {}", e);
            exit(1);
        }
    };

    exit(status.code().unwrap_or(1));
}

// 標準エラーの JSONL を変換する
fn convert_json_error_format(data: Vec<u8>) -> Vec<u8> {
    // UTF-8として解釈できなければそのまま返す
    let s = match std::str::from_utf8(&data) {
        Ok(s) => s,
        Err(_) => return data,
    };

    let mut out_lines = Vec::new();
    for line in s.lines() {
        // 各行をJSONとしてパース
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(json) => {
                // 変換処理関数を呼び出す
                let converted = convert_json_error_line(json);

                // 変換後をJSON文字列化
                match serde_json::to_string(&converted) {
                    Ok(s) => out_lines.push(s),
                    Err(_) => return data, // 失敗したら何もしない
                };
            }
            Err(_) => return data, // パース失敗時は何もしない
        }
    }
    // 改行区切りで連結してバイト列に戻す
    out_lines.join("\n").into_bytes()
}

// コンパイルエラーのJSONであれば、各種フィールドを日本語に翻訳する
fn convert_json_error_line(json: serde_json::Value) -> serde_json::Value {
    if let serde_json::Value::Object(ref obj) = json {
        if let Some(mt) = obj.get("$message_type") {
            if mt == "diagnostic" {
                return translate_json_message(&json, &TRANSLATE_LIST);
            }
        }
    }
    json
}

// JSON内のメッセージを日本語に翻訳する
// 翻訳対象は以下のフィールド（値が null の場合には何もしない）
// - "message"
// - "spans[].label"
// - "children[].message"
// - "children[].spans[].label"
// また "rendered" フィールドの中身について、各メッセージの翻訳前と同じ文字列が含まれている場合には、翻訳後文字列に置き換える
// JSONフォーマットの形式は以下を参照
// - <https://doc.rust-lang.org/rustc/json.html>
pub fn translate_json_message(
    json: &serde_json::Value,
    translations: &[TranslateEntry],
) -> serde_json::Value {
    let mut new_json = json.clone();
    let mut replaced = Vec::new();

    // message
    if let Some(message) = json.get("message").and_then(|m| m.as_str()) {
        let translated = translate_message(message, translations);
        if translated != message {
            new_json["message"] = serde_json::Value::String(translated.clone());
            replaced.push((message.to_string(), translated));
        }
    }

    // spans[].label
    if let Some(spans) = json.get("spans").and_then(|s| s.as_array()) {
        let mut new_spans = spans.clone();
        for (i, span) in spans.iter().enumerate() {
            if let Some(label) = span.get("label").and_then(|l| l.as_str()) {
                let translated = translate_message(label, translations);
                if translated != label {
                    let mut new_span = span.clone();
                    new_span["label"] = serde_json::Value::String(translated.clone());
                    new_spans[i] = new_span;
                    replaced.push((label.to_string(), translated));
                }
            }
        }
        new_json["spans"] = serde_json::Value::Array(new_spans);
    }

    // children[].message, children[].spans[].label
    if let Some(children) = json.get("children").and_then(|c| c.as_array()) {
        let mut new_children = children.clone();
        for (i, child) in children.iter().enumerate() {
            let mut new_child = child.clone();
            // children[].message
            if let Some(child_msg) = child.get("message").and_then(|m| m.as_str()) {
                let translated = translate_message(child_msg, translations);
                if translated != child_msg {
                    new_child["message"] = serde_json::Value::String(translated.clone());
                    replaced.push((child_msg.to_string(), translated));
                }
            }
            // children[].spans[].label
            if let Some(child_spans) = child.get("spans").and_then(|s| s.as_array()) {
                let mut new_child_spans = child_spans.clone();
                for (j, span) in child_spans.iter().enumerate() {
                    if let Some(label) = span.get("label").and_then(|l| l.as_str()) {
                        let translated = translate_message(label, translations);
                        if translated != label {
                            let mut new_span = span.clone();
                            new_span["label"] = serde_json::Value::String(translated.clone());
                            new_child_spans[j] = new_span;
                            replaced.push((label.to_string(), translated));
                        }
                    }
                }
                new_child["spans"] = serde_json::Value::Array(new_child_spans);
            }
            new_children[i] = new_child;
        }
        new_json["children"] = serde_json::Value::Array(new_children);
    }

    // rendered の置換
    if let Some(rendered) = new_json.get("rendered").and_then(|r| r.as_str()) {
        let mut new_rendered = rendered.to_string();
        for (orig, trans) in &replaced {
            if !orig.is_empty() && orig != trans {
                new_rendered = new_rendered.replace(orig, trans);
            }
        }
        new_json["rendered"] = serde_json::Value::String(new_rendered);
    }

    // 1行JSONLとして返す
    match serde_json::to_string(&new_json) {
        Ok(s) => serde_json::from_str(&s).unwrap_or(new_json),
        Err(_) => new_json,
    }
}

/// メッセージを日本語に翻訳する
pub fn translate_message(message: &str, translations: &[TranslateEntry]) -> String {
    // プレースホルダ用の正規表現
    static PLACEHOLDER_RE: once_cell::sync::Lazy<regex::Regex> =
        once_cell::sync::Lazy::new(|| regex::Regex::new(r"\{\$(\w+)\}").unwrap());
    for trans in translations.iter() {
        let en_str = &trans.en;
        let ja_str = &trans.ja;

        // プレースホルダ以外の部分をエスケープしつつ、プレースホルダは名前付きグループに変換
        let mut re_str = String::new();
        let mut last = 0;
        for caps in PLACEHOLDER_RE.captures_iter(en_str) {
            let m = caps.get(0).unwrap();
            // プレースホルダ前の部分をエスケープ
            re_str.push_str(&regex::escape(&en_str[last..m.start()]));
            // プレースホルダ部分を名前付きグループに
            let name = &caps[1];
            re_str.push_str(&format!("(?P<{}>.+?)", name));
            last = m.end();
        }
        // 残りの部分をエスケープ
        re_str.push_str(&regex::escape(&en_str[last..]));

        // 末尾に「.*」を追加して先頭一致＋残り文字列取得
        let re = match regex::Regex::new(&format!("^{}(.*)$", re_str)) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if let Some(caps) = re.captures(message) {
            // ja側のプレースホルダをキャプチャ値で置換
            let mut result = ja_str.to_string();
            for name in re.capture_names().flatten() {
                if name.is_empty() || name == "0" || name == "1" {
                    continue;
                }
                if let Some(val) = caps.name(name) {
                    result = result.replace(&format!("{{${}}}", name), val.as_str());
                }
            }
            // 追加: パターン外の残り文字列を末尾に追加
            if let Some(extra) = caps.get(caps.len() - 1) {
                let extra_str = extra.as_str();
                if !extra_str.is_empty() {
                    result.push_str(extra_str);
                }
            }
            return result;
        }
    }
    message.to_string()
}

/// デバッグ用: /tmp/rustc-ja-wrapper-debug.log に追記書き込みする
pub fn append_debug_log(msg: &str) {
    use std::fs::OpenOptions;
    use std::io::Write;
    let path = "/tmp/rustc-ja-wrapper-debug.log";
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{}", msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_translate_message_simple() {
        // テスト用の翻訳データ
        let test_translate_entries: &[TranslateEntry] = &[
            TranslateEntry { en: "hello".to_string(), ja: "こんにちは".to_string() },
            TranslateEntry { en: "error: {$name}".to_string(), ja: "エラー: {$name}".to_string() },
            TranslateEntry { en: "borrow of moved value".to_string(), ja: "移動された値の借用".to_string() },
            TranslateEntry {
                en: "move occurs because `{$name}` has type `{$ty}`, which does not implement the `Copy` trait".to_string(),
                ja: "`{$ty}` 型の `{$name}` は `Copy` トレイトを実装していないので、移動します".to_string()
            },
        ];

        assert_eq!(
            translate_message("hello", test_translate_entries),
            "こんにちは"
        );
        assert_eq!(
            translate_message("error: foo", test_translate_entries),
            "エラー: foo"
        );
        assert_eq!(
            translate_message("not found", test_translate_entries),
            "not found"
        );
        assert_eq!(
            translate_message("borrow of moved value", test_translate_entries),
            "移動された値の借用"
        );
        assert_eq!(
            translate_message(
                "move occurs because `s1` has type `String`, which does not implement the `Copy` trait",
                test_translate_entries
            ),
            "`String` 型の `s1` は `Copy` トレイトを実装していないので、移動します"
        );
    }

    #[test]
    fn test_translate_json_message_message_field() {
        let test_translate_entries: &[TranslateEntry] = &[
            TranslateEntry {
                en: "borrow of moved value".to_string(),
                ja: "移動された値の借用".to_string(),
            },
            TranslateEntry {
                en: "value moved here".to_string(),
                ja: "ここで値を移動".to_string(),
            },
            TranslateEntry {
                en: "value borrowed here after move".to_string(),
                ja: "移動後の値をここで借用".to_string(),
            },
            TranslateEntry {
                en: "consider cloning the value if the performance cost is acceptable".to_string(),
                ja: "複製コストが許容できるなら、クローンすることを検討してください".to_string(),
            },
        ];
        let json = serde_json::json!({
            "message": "borrow of moved value: `s1`",
            "spans": [
                {
                    "label": "value moved here",
                },
                {
                    "label": "value borrowed here after move",
                },
            ],
            "children": [
                {
                    "message": "consider cloning the value if the performance cost is acceptable",
                    "spans": [
                        {
                            "label": "hello",
                        }
                    ],
                },
            ],
            "rendered": "borrow of moved value: `s1`\nvalue moved here\nvalue borrowed here after move\nconsider cloning the value if the performance cost is acceptable",
        });
        let translated = translate_json_message(&json, test_translate_entries);
        let expected_json = serde_json::json!({
            "message": "移動された値の借用: `s1`",
            "spans": [
                {
                    "label": "ここで値を移動",
                },
                {
                    "label": "移動後の値をここで借用",
                },
            ],
            "children": [
                {
                    "message": "複製コストが許容できるなら、クローンすることを検討してください",
                    "spans": [
                        {
                            "label": "hello",
                        }
                    ],
                },
            ],
            "rendered": "移動された値の借用: `s1`\nここで値を移動\n移動後の値をここで借用\n複製コストが許容できるなら、クローンすることを検討してください",
        });
        assert_eq!(translated.get("message"), expected_json.get("message"));
        assert_eq!(translated.get("spans"), expected_json.get("spans"));
        assert_eq!(translated.get("children"), expected_json.get("children"));
        assert_eq!(translated.get("rendered"), expected_json.get("rendered"));
    }
}
