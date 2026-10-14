//! touch-server クライアント。`~/ssd/fb/touch/touch-server` デーモンへ Unix socket で
//! 接続し、フォーカス中ペイン内のタッチイベント(JSON Lines)を受け取って、
//! ローカル化＋ジェスチャ判定した結果を mpsc でメインループへ渡す。
//!
//! - サーバーは生のタッチ区切り(down/move/up)だけを送り、「タップ」の
//!   意味付けはこちら側で行う(vim-client / ssbrowse と同じ方針)。
//! - 接続できなければスレッドを静かに終了し、アプリはタッチ無しで通常動作する。
//! - tmux 外で起動した場合は `pane=None` となりサーバー側で配信先に選ばれない。

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::mpsc::Sender;
use std::thread;

/// アプリへ渡すジェスチャ。座標はペインローカルなセル(ratatui の `f.area()` と同系)。
#[derive(Debug, Clone)]
pub enum TouchInput {
    /// タップ。離した位置を返す。
    Tap { col: u16, row: u16 },
}

/// アクティブペインの配置(tmux セル単位)。座標のローカル化に使う。
#[derive(Deserialize, Debug, Clone)]
struct PaneCtx {
    win_w: u32,
    win_h: u32,
    left: u32,
    top: u32,
}

/// サーバー → クライアントのタッチイベント。最小実装では `up` のみ使う。
#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Event {
    Down {},
    Move {},
    /// 離した瞬間。終了位置(fx1,fy1)の frac。
    Up {
        fx1: f64,
        fy1: f64,
        ctx: Option<PaneCtx>,
    },
}

/// クライアント → サーバー(接続直後に1行だけ送る)。
#[derive(Serialize)]
struct Hello {
    hello: &'static str,
    pane: Option<String>,
}

/// ソケットパス。`TOUCH_SERVER_SOCK` → `$XDG_RUNTIME_DIR/touch-server.sock` → `/tmp/...`。
fn socket_path() -> String {
    if let Ok(p) = std::env::var("TOUCH_SERVER_SOCK") {
        if !p.is_empty() {
            return p;
        }
    }
    match std::env::var("XDG_RUNTIME_DIR") {
        Ok(d) if !d.is_empty() => format!("{d}/touch-server.sock"),
        _ => "/tmp/touch-server.sock".to_string(),
    }
}

/// グローバル frac (0..1) を ctx でペインローカルなセル座標へ変換する。
fn localize(fx: f64, fy: f64, ctx: &PaneCtx) -> Option<(u16, u16)> {
    if ctx.win_w == 0 || ctx.win_h == 0 {
        return None;
    }
    let gcol = ((fx * ctx.win_w as f64) as i64).clamp(0, ctx.win_w as i64 - 1);
    let grow = ((fy * ctx.win_h as f64) as i64).clamp(0, ctx.win_h as i64 - 1);
    let col = gcol - ctx.left as i64;
    let row = grow - ctx.top as i64;
    if col < 0 || row < 0 {
        return None;
    }
    Some((col as u16, row as u16))
}

/// 受信スレッドを起動する。接続失敗・切断時は静かに終了する。
pub fn spawn(tx: Sender<TouchInput>) {
    thread::spawn(move || {
        if let Err(e) = run(&tx) {
            eprintln!("[touch] {e}");
        }
    });
}

fn run(tx: &Sender<TouchInput>) -> std::io::Result<()> {
    let path = socket_path();
    let stream = UnixStream::connect(&path).map_err(|e| {
        std::io::Error::new(e.kind(), format!("touch-server に接続できません ({path}): {e}"))
    })?;

    let hello = Hello {
        hello: "calendar-tui",
        pane: std::env::var("TMUX_PANE").ok(),
    };
    let mut writer = stream.try_clone()?;
    let line = serde_json::to_string(&hello).unwrap();
    writeln!(writer, "{line}")?;

    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let ev = match serde_json::from_str::<Event>(&line) {
            Ok(ev) => ev,
            Err(_) => continue,
        };
        // 最小実装では up のみ意味付けする(down/move は無視)。
        let Event::Up {
            fx1,
            fy1,
            ctx: Some(ctx),
        } = ev
        else {
            continue;
        };
        let Some((col, row)) = localize(fx1, fy1, &ctx) else {
            continue;
        };
        let input = TouchInput::Tap { col, row };
        // 受信側(メインループ)が落ちていたら終了。
        if tx.send(input).is_err() {
            break;
        }
    }
    Ok(())
}
