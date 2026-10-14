use crate::app::{App, AppMode};
use chrono::{Datelike, Local};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

const DAY_NAMES: [&str; 7] = ["月", "火", "水", "木", "金", "土", "日"];

/// fbterm で崩れずに表示できる形へ変換する。fbterm は U+FFFF より上を � にし、幅も
/// Unicode 5.0 の mk_wcwidth で数えるため、絵文字は ratatui/tmux と桁数が食い違ったり
/// 全角グリフが左半分で切れたりする。F1 カレンダーのアイコンは漢字 1 文字に、それ以外の
/// 幅が食い違う文字は 〓 に置き換え、異体字セレクタ・ZWJ は取り除く
fn terminal_safe(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '\u{FE0E}' | '\u{FE0F}' | '\u{200D}'))
        .map(|c| match c {
            '\u{1F3CE}' => '練', // 🏎 フリー走行
            '\u{23F1}' => '予',  // ⏱ 予選・スプリント予選
            '\u{1F3C1}' => '決', // 🏁 決勝・スプリント
            c if c.width() == Some(2) && !fbterm_wide(c) => '〓',
            c => c,
        })
        .collect()
}

/// fbterm (mk_wcwidth) が全角として扱う文字か。U+FFFF より上は � (半角) になる
fn fbterm_wide(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x115F | 0x2329 | 0x232A | 0x2E80..=0x303E | 0x3040..=0xA4CF
        | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE10..=0xFE19 | 0xFE30..=0xFE6F
        | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6)
}

/// 表示幅が max_cols を超える場合は末尾を「…」にして切り詰める
fn truncate_to_width(s: &str, max_cols: usize) -> String {
    if s.width() <= max_cols {
        return s.to_string();
    }
    let target = max_cols.saturating_sub(1);
    let mut used = 0;
    let mut end = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(1);
        if used + cw > target {
            break;
        }
        used += cw;
        end += c.len_utf8();
    }
    format!("{}…", &s[..end])
}

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(49), Constraint::Length(32)])
        .split(f.area());

    render_calendar(f, chunks[0], app);
    render_sidebar(f, chunks[1], app);
}

fn render_calendar(f: &mut Frame, area: Rect, app: &App) {
    let today = Local::now().date_naive();
    let title = format!(
        " {}年{}月  hjkl:移動  [/]:月移動  t:今日  o:追加  q:終了 ",
        app.current_month.year(),
        app.current_month.month()
    );

    let block = Block::default().title(title).borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = app.grid_rows();
    let mut constraints = vec![Constraint::Length(1)];
    constraints.extend(vec![Constraint::Min(8); rows]);

    let row_areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    // Day-of-week header
    let col_areas = split_cols(row_areas[0]);
    for (i, name) in DAY_NAMES.iter().enumerate() {
        let style = day_style(i, false, false, false, true);
        f.render_widget(Paragraph::new(*name).style(style).centered(), col_areas[i]);
    }

    // Week rows
    let dates = app.grid_dates();
    for week in 0..rows {
        let col_areas = split_cols(row_areas[week + 1]);
        for day in 0..7 {
            let date = dates[week * 7 + day];
            let is_selected = date == app.selected_date;
            let is_today = date == today;
            let is_current = date.month() == app.current_month.month();

            let events = app.events_for(&date);

            let cell_block = if is_selected {
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
            } else {
                Block::default().borders(Borders::ALL)
            };
            let cell_inner = cell_block.inner(col_areas[day]);
            f.render_widget(cell_block, col_areas[day]);

            let date_style = day_style(day, is_selected, is_today, is_current, false);
            let mut lines = vec![Line::from(Span::styled(
                format!("{}", date.day()),
                date_style,
            ))];

            let total = events.len();
            let display_limit = if total >= 6 { 4 } else { 5 };
            let mut count = 0;

            for event in events.iter() {
                if count >= display_limit {
                    break;
                }
                let style = if event.is_holiday {
                    Style::default().fg(Color::Red)
                } else if event.calendar_name.is_some() {
                    Style::default().fg(Color::Magenta)
                } else {
                    Style::default().fg(Color::Green)
                };
                let truncated = truncate_to_width(&terminal_safe(&event.title), cell_inner.width as usize);
                lines.push(Line::from(Span::styled(truncated, style)));
                count += 1;
            }

            if total > display_limit {
                lines.push(Line::from(Span::styled(
                    format!("+{}", total - display_limit),
                    Style::default().fg(Color::DarkGray),
                )));
            }

            f.render_widget(Paragraph::new(lines), cell_inner);
        }
    }
}

/// 各日付セルの矩形を `render_calendar` と同一のレイアウトで算出する。
/// タッチ座標→日付のヒットテストに使う(描画とレイアウトを共有するための単一ソース)。
/// 変更時は `draw`/`render_calendar` のレイアウト(トップ分割・枠・行分割)と必ず揃えること。
pub fn calendar_cell_rects(viewport: Rect, app: &App) -> Vec<(chrono::NaiveDate, Rect)> {
    let calendar_area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(49), Constraint::Length(32)])
        .split(viewport)[0];
    let inner = Block::default().borders(Borders::ALL).inner(calendar_area);

    let rows = app.grid_rows();
    let mut constraints = vec![Constraint::Length(1)];
    constraints.extend(vec![Constraint::Min(8); rows]);
    let row_areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    let dates = app.grid_dates();
    let mut out = Vec::with_capacity(rows * 7);
    for week in 0..rows {
        let col_areas = split_cols(row_areas[week + 1]);
        for day in 0..7 {
            out.push((dates[week * 7 + day], col_areas[day]));
        }
    }
    out
}

fn split_cols(area: Rect) -> Vec<Rect> {
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![Constraint::Ratio(1, 7); 7])
        .split(area)
        .to_vec()
}

fn day_style(col: usize, is_selected: bool, is_today: bool, is_current: bool, header: bool) -> Style {
    if header {
        return match col {
            5 => Style::default().fg(Color::Blue),
            6 => Style::default().fg(Color::Red),
            _ => Style::default().add_modifier(Modifier::BOLD),
        };
    }
    if is_selected {
        return Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
    }
    if is_today {
        return Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
    }
    if !is_current {
        return Style::default().fg(Color::DarkGray);
    }
    match col {
        5 => Style::default().fg(Color::Blue),
        6 => Style::default().fg(Color::Red),
        _ => Style::default(),
    }
}

// ─── Sidebar ────────────────────────────────────────────────────────────────

fn render_sidebar(f: &mut Frame, area: Rect, app: &App) {
    match &app.mode {
        AppMode::AddEvent { title } => render_add_event(f, area, app, title),
        AppMode::DeleteConfirm { event_index } => {
            render_delete_confirm(f, area, app, *event_index)
        }
        AppMode::SelectItemDelete { selected } => {
            render_select_item_delete(f, area, app, *selected)
        }
        AppMode::SelectItemEdit { selected } => {
            render_select_item_edit(f, area, app, *selected)
        }
        AppMode::EditEvent { event_index, title } => {
            render_edit_event(f, area, app, *event_index, title)
        }
        AppMode::EditEventTime { event_index, title, time_input } => {
            render_edit_event_time(f, area, app, *event_index, title, time_input)
        }
        AppMode::EditEventEndDate { event_index, title, start_time, end_date_input } => {
            render_edit_event_end_date(f, area, app, *event_index, title, start_time.as_deref(), end_date_input)
        }
        AppMode::AddEventTime { title, time_input } => {
            render_add_event_time(f, area, app, title, time_input)
        }
        AppMode::AddEventEndTime { title, start_time, end_time_input } => {
            render_add_event_end_time(f, area, app, title, start_time, end_time_input)
        }
        AppMode::AddEventEndDate { title, start_time, end_time, end_date_input } => {
            render_add_event_end_date(f, area, app, title, start_time.as_deref(), end_time.as_deref(), end_date_input)
        }
        AppMode::Normal => render_normal_sidebar(f, area, app),
    }
}

fn render_normal_sidebar(f: &mut Frame, area: Rect, app: &App) {
    let d = app.selected_date;
    let wd = d.weekday() as usize;
    let title = format!(
        " {}年{}月{}日({}) ",
        d.year(),
        d.month(),
        d.day(),
        DAY_NAMES[wd]
    );

    let block = Block::default().title(title).borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1), Constraint::Length(3)])
        .split(inner);

    let events = app.selected_events();

    let mut items: Vec<ListItem> = Vec::new();

    if events.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "予定なし",
            Style::default().fg(Color::DarkGray),
        ))));
    } else {
        for e in events.iter() {
            let time_str = if let Some(end_d) = e.end_date {
                let s_str = format!("{}/{}", e.date.month(), e.date.day());
                let e_str = format!("{}/{}", end_d.month(), end_d.day());
                match (&e.start_time, &e.end_time) {
                    (Some(st), Some(et)) => format!("{} {} ~ {} {} ", s_str, st, e_str, et),
                    (Some(st), None)     => format!("{} {} ~ {} ", s_str, st, e_str),
                    _                    => format!("{} ~ {} ", s_str, e_str),
                }
            } else {
                match (&e.start_time, &e.end_time) {
                    (Some(s), Some(end)) if s != end => format!("{}-{} ", s, end),
                    (Some(s), _) => format!("{} ", s),
                    _ => String::new(),
                }
            };
            let style = if e.is_holiday {
                Style::default().fg(Color::Red)
            } else if e.calendar_name.is_some() {
                Style::default().fg(Color::Magenta)
            } else {
                Style::default()
            };
            let mut spans = vec![Span::styled(time_str, Style::default().fg(Color::Cyan))];
            if let Some(ref cal_name) = e.calendar_name {
                spans.push(Span::styled(
                    format!("[{}] ", terminal_safe(cal_name)),
                    Style::default().fg(Color::Magenta),
                ));
            }
            spans.push(Span::styled(terminal_safe(&e.title), style));
            items.push(ListItem::new(Line::from(spans)));
        }
    }

    f.render_widget(List::new(items), v[0]);

    if let Some(msg) = &app.status_message {
        f.render_widget(
            Paragraph::new(msg.as_str()).style(Style::default().fg(Color::Yellow)),
            v[1],
        );
    }

    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("o:追加 c:編集 d:削除 q:終了", Style::default().fg(Color::DarkGray))),
            Line::from(Span::styled("hjkl:日/週  []:月  t:今日", Style::default().fg(Color::DarkGray))),
            Line::from(Span::styled("0/$:週頭/週末  gg/G:月初/月末", Style::default().fg(Color::DarkGray))),
        ]),
        v[2],
    );
}

fn render_add_event(f: &mut Frame, area: Rect, app: &App, title_input: &str) {
    let d = app.selected_date;
    let block = Block::default()
        .title(format!(" 予定追加 — {}月{}日 ", d.month(), d.day()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    f.render_widget(Paragraph::new("タイトル:"), v[0]);

    let input_block = Block::default().borders(Borders::ALL);
    let input_inner = input_block.inner(v[1]);
    f.render_widget(input_block, v[1]);
    f.render_widget(
        Paragraph::new(title_input).style(Style::default().fg(Color::White)),
        input_inner,
    );
    f.set_cursor_position((
        input_inner.x + title_input.width() as u16,
        input_inner.y,
    ));

    f.render_widget(
        Paragraph::new("Enter:確定  Esc:キャンセル")
            .style(Style::default().fg(Color::DarkGray)),
        v[2],
    );
}

fn render_delete_confirm(f: &mut Frame, area: Rect, app: &App, idx: usize) {
    let block = Block::default()
        .title(" 予定を削除 ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if let Some(event) = app.selected_events().get(idx) {
        f.render_widget(
            Paragraph::new(vec![
                Line::from("以下の予定を削除しますか？"),
                Line::from(""),
                Line::from(Span::styled(
                    terminal_safe(&event.title),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "y:削除  n/Esc:キャンセル",
                    Style::default().fg(Color::DarkGray),
                )),
            ]),
            inner,
        );
    }
}

fn build_item_list(app: &App, selected: usize) -> Vec<ListItem<'static>> {
    app.selected_events()
        .iter()
        .filter(|e| !e.is_holiday)
        .enumerate()
        .map(|(idx, event)| {
            let is_sel = idx == selected;
            let style = if is_sel {
                Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            };
            let prefix = if is_sel { "> " } else { "  " };
            ListItem::new(Line::from(Span::styled(
                format!("{}{}", prefix, terminal_safe(&event.title)),
                style,
            )))
        })
        .collect()
}

fn render_select_item_delete(f: &mut Frame, area: Rect, app: &App, selected: usize) {
    let d = app.selected_date;
    let block = Block::default()
        .title(format!(" 削除する予定を選択 — {}月{}日 ", d.month(), d.day()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    f.render_widget(List::new(build_item_list(app, selected)), v[0]);
    f.render_widget(
        Paragraph::new("j/k:移動  Enter:削除  Esc:キャンセル")
            .style(Style::default().fg(Color::DarkGray)),
        v[1],
    );
}

fn render_select_item_edit(f: &mut Frame, area: Rect, app: &App, selected: usize) {
    let d = app.selected_date;
    let block = Block::default()
        .title(format!(" 編集する予定を選択 — {}月{}日 ", d.month(), d.day()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    f.render_widget(List::new(build_item_list(app, selected)), v[0]);
    f.render_widget(
        Paragraph::new("j/k:移動  Enter:編集  Esc:キャンセル")
            .style(Style::default().fg(Color::DarkGray)),
        v[1],
    );
}

fn render_edit_event(f: &mut Frame, area: Rect, app: &App, event_index: usize, title_input: &str) {
    let d = app.selected_date;
    let original = app
        .selected_events()
        .get(event_index)
        .map(|e| e.title.as_str())
        .unwrap_or("");
    let block = Block::default()
        .title(format!(" 予定を編集 — {}月{}日 ", d.month(), d.day()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    f.render_widget(
        Paragraph::new(format!("変更前: {}", terminal_safe(original)))
            .style(Style::default().fg(Color::DarkGray)),
        v[0],
    );
    f.render_widget(Paragraph::new("新しいタイトル:"), v[1]);

    let input_block = Block::default().borders(Borders::ALL);
    let input_inner = input_block.inner(v[2]);
    f.render_widget(input_block, v[2]);
    f.render_widget(
        Paragraph::new(title_input).style(Style::default().fg(Color::White)),
        input_inner,
    );
    f.set_cursor_position((
        input_inner.x + title_input.width() as u16,
        input_inner.y,
    ));

    f.render_widget(
        Paragraph::new("Enter:確定  Esc:キャンセル")
            .style(Style::default().fg(Color::DarkGray)),
        v[3],
    );
}

fn render_add_event_time(f: &mut Frame, area: Rect, app: &App, title: &str, time_input: &str) {
    let d = app.selected_date;
    let block = Block::default()
        .title(format!(" 予定追加 — {}月{}日 (2/4) ", d.month(), d.day()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    f.render_widget(
        Paragraph::new(format!("タイトル: {}", title)).style(Style::default().fg(Color::DarkGray)),
        v[0],
    );
    f.render_widget(Paragraph::new("開始時刻 (HH:MM, 空白でスキップ):"), v[1]);

    let input_block = Block::default().borders(Borders::ALL);
    let input_inner = input_block.inner(v[2]);
    f.render_widget(input_block, v[2]);
    f.render_widget(
        Paragraph::new(time_input).style(Style::default().fg(Color::White)),
        input_inner,
    );
    f.set_cursor_position((input_inner.x + time_input.width() as u16, input_inner.y));

    f.render_widget(
        Paragraph::new("Enter:次へ  Esc:戻る").style(Style::default().fg(Color::DarkGray)),
        v[3],
    );
}

fn render_add_event_end_time(
    f: &mut Frame,
    area: Rect,
    app: &App,
    title: &str,
    start_time: &str,
    end_time_input: &str,
) {
    let d = app.selected_date;
    let block = Block::default()
        .title(format!(" 予定追加 — {}月{}日 (3/4) ", d.month(), d.day()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    f.render_widget(
        Paragraph::new(format!("タイトル: {}", title)).style(Style::default().fg(Color::DarkGray)),
        v[0],
    );
    f.render_widget(
        Paragraph::new(format!("開始時刻: {}", start_time)).style(Style::default().fg(Color::DarkGray)),
        v[1],
    );
    f.render_widget(Paragraph::new("終了時刻 (HH:MM, 空白で1時間後):"), v[2]);

    let input_block = Block::default().borders(Borders::ALL);
    let input_inner = input_block.inner(v[3]);
    f.render_widget(input_block, v[3]);
    f.render_widget(
        Paragraph::new(end_time_input).style(Style::default().fg(Color::White)),
        input_inner,
    );
    f.set_cursor_position((input_inner.x + end_time_input.width() as u16, input_inner.y));

    f.render_widget(
        Paragraph::new("Enter:次へ  Esc:戻る").style(Style::default().fg(Color::DarkGray)),
        v[4],
    );
}

fn render_add_event_end_date(
    f: &mut Frame,
    area: Rect,
    app: &App,
    title: &str,
    start_time: Option<&str>,
    end_time: Option<&str>,
    end_date_input: &str,
) {
    let d = app.selected_date;
    let block = Block::default()
        .title(format!(" 予定追加 — {}月{}日 (4/4) ", d.month(), d.day()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    f.render_widget(
        Paragraph::new(format!("タイトル: {}", title)).style(Style::default().fg(Color::DarkGray)),
        v[0],
    );
    let time_label = match (start_time, end_time) {
        (Some(s), Some(e)) => format!("時刻: {}〜{}", s, e),
        (Some(s), None) => format!("時刻: {}", s),
        _ => "時刻: なし".to_string(),
    };
    f.render_widget(
        Paragraph::new(time_label).style(Style::default().fg(Color::DarkGray)),
        v[1],
    );
    f.render_widget(Paragraph::new("終了日 (YYYY-MM-DD, 空白で同日):"), v[2]);

    let input_block = Block::default().borders(Borders::ALL);
    let input_inner = input_block.inner(v[3]);
    f.render_widget(input_block, v[3]);
    f.render_widget(
        Paragraph::new(end_date_input).style(Style::default().fg(Color::White)),
        input_inner,
    );
    f.set_cursor_position((input_inner.x + end_date_input.width() as u16, input_inner.y));

    f.render_widget(
        Paragraph::new("Enter:確定  Esc:戻る").style(Style::default().fg(Color::DarkGray)),
        v[4],
    );
}

fn render_edit_event_time(
    f: &mut Frame,
    area: Rect,
    app: &App,
    event_index: usize,
    title: &str,
    time_input: &str,
) {
    let d = app.selected_date;
    let original = app
        .selected_events()
        .get(event_index)
        .map(|e| e.title.as_str())
        .unwrap_or("");
    let block = Block::default()
        .title(format!(" 予定を編集 — {}月{}日 (2/3) ", d.month(), d.day()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    f.render_widget(
        Paragraph::new(format!("予定: {}", terminal_safe(original))).style(Style::default().fg(Color::DarkGray)),
        v[0],
    );
    f.render_widget(
        Paragraph::new(format!("新タイトル: {}", title)).style(Style::default().fg(Color::White)),
        v[1],
    );
    f.render_widget(Paragraph::new("開始時刻 (HH:MM, 空白でクリア):"), v[2]);

    let input_block = Block::default().borders(Borders::ALL);
    let input_inner = input_block.inner(v[3]);
    f.render_widget(input_block, v[3]);
    f.render_widget(
        Paragraph::new(time_input).style(Style::default().fg(Color::White)),
        input_inner,
    );
    f.set_cursor_position((input_inner.x + time_input.width() as u16, input_inner.y));

    f.render_widget(
        Paragraph::new("Enter:次へ  Esc:戻る").style(Style::default().fg(Color::DarkGray)),
        v[4],
    );
}

fn render_edit_event_end_date(
    f: &mut Frame,
    area: Rect,
    app: &App,
    event_index: usize,
    title: &str,
    start_time: Option<&str>,
    end_date_input: &str,
) {
    let d = app.selected_date;
    let original = app
        .selected_events()
        .get(event_index)
        .map(|e| e.title.as_str())
        .unwrap_or("");
    let block = Block::default()
        .title(format!(" 予定を編集 — {}月{}日 (3/3) ", d.month(), d.day()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    f.render_widget(
        Paragraph::new(format!("予定: {}", terminal_safe(original))).style(Style::default().fg(Color::DarkGray)),
        v[0],
    );
    f.render_widget(
        Paragraph::new(format!("新タイトル: {}", title)).style(Style::default().fg(Color::White)),
        v[1],
    );
    let time_label = start_time.map(|t| format!("時刻: {}", t)).unwrap_or_else(|| "時刻: なし".to_string());
    f.render_widget(
        Paragraph::new(time_label).style(Style::default().fg(Color::DarkGray)),
        v[2],
    );
    f.render_widget(Paragraph::new("終了日 (YYYY-MM-DD, 空白で同日):"), v[3]);

    let input_block = Block::default().borders(Borders::ALL);
    let input_inner = input_block.inner(v[4]);
    f.render_widget(input_block, v[4]);
    f.render_widget(
        Paragraph::new(end_date_input).style(Style::default().fg(Color::White)),
        input_inner,
    );
    f.set_cursor_position((input_inner.x + end_date_input.width() as u16, input_inner.y));

    f.render_widget(
        Paragraph::new("Enter:確定  Esc:戻る").style(Style::default().fg(Color::DarkGray)),
        v[5],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f1_icons_become_kanji() {
        assert_eq!(terminal_safe("🏎\u{FE0F} FORMULA 1 - Practice 1"), "練 FORMULA 1 - Practice 1");
        assert_eq!(terminal_safe("⏱\u{FE0F} FORMULA 1 - Qualifying"), "予 FORMULA 1 - Qualifying");
        assert_eq!(terminal_safe("🏁 FORMULA 1 - Race"), "決 FORMULA 1 - Race");
    }

    #[test]
    fn width_mismatched_chars_become_geta() {
        assert_eq!(terminal_safe("🚗⏰👨\u{200D}👩"), "〓〓〓〓");
        assert_eq!(terminal_safe("会議 ● 10:00"), "会議 ● 10:00");
    }
}
