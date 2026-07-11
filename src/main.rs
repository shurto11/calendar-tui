mod app;
mod auth;
mod calendar;
mod config;
mod matrix;
mod meta;
mod priority;
mod tasks;
mod touch;
mod ui;

use anyhow::Result;
use crossterm::{
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;

#[tokio::main]
async fn main() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    // ペーストされたテキストを個々のキー入力として誤解釈しないよう bracketed paste を有効化する
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableBracketedPaste,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    if let Err(e) = &result {
        eprintln!("エラー: {}", e);
    }

    result
}

async fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    use std::time::Duration;

    let mut app = app::App::new().await?;

    // touch-server からのタッチイベントを受け取るチャネルを用意し、受信スレッドを起動する。
    // 接続できなくてもアプリはキー操作で通常どおり動く。
    let (touch_tx, touch_rx) = std::sync::mpsc::channel();
    touch::spawn(touch_tx);

    // 起動・認証中にターミナルへ溜まった入力（type-ahead やペースト）を破棄する。
    // これを捨てないと、バッファ内のバイト列がキー入力として実行され、勝手に予定が追加されてしまう。
    while event::poll(Duration::from_millis(0))? {
        let _ = event::read()?;
    }

    // uim-fep の IME トグル(Ctrl+Space)はステータス行の描画で画面を乱すことがあり、
    // 差分描画のままだと前フレームの残骸が重なって表示される。
    // モードファイルの変化を監視し、トグルを検知したら全画面を描き直す。
    let getmode_path = std::env::var_os("UIM_FEP_GETMODE").map(std::path::PathBuf::from);
    let mut last_ime_mode = getmode_path.as_deref().and_then(|p| std::fs::read(p).ok());

    'main: loop {
        if let Some(path) = getmode_path.as_deref() {
            let mode = std::fs::read(path).ok();
            if mode != last_ime_mode {
                last_ime_mode = mode;
                terminal.clear()?;
            }
        }

        terminal.draw(|f| {
            app.viewport = f.area();
            ui::draw(f, &app)
        })?;

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => {
                    // キーの離上(Release)/リピートを処理すると同じ入力が二重に実行されるため、
                    // 押下(Press)イベントのみを処理する。
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
                        break;
                    }
                    if app.handle_key(key).await? {
                        break;
                    }
                }
                // ペーストされたテキストはコマンドとして解釈しない。
                Event::Paste(_) => {}
                _ => {}
            }
        }

        // 溜まったタッチ入力を処理する。
        while let Ok(input) = touch_rx.try_recv() {
            if app.handle_touch(input).await? {
                break 'main;
            }
        }
    }

    Ok(())
}
