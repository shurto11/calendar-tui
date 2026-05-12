use crate::app::{App, AppMode};
use crate::tasks::Task;
use chrono::{Datelike, Local};
use unicode_width::UnicodeWidthStr;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

const DAY_NAMES: [&str; 7] = ["月", "火", "水", "木", "金", "土", "日"];

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
        " {}年{}月  [/]:月移動  hjkl:日移動  a:予定  t:タスク  q:終了 ",
        app.current_month.year(),
        app.current_month.month()
    );

    let block = Block::default().title(title).borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = app.grid_rows();
    let mut constraints = vec![Constraint::Length(1)];
    constraints.extend(vec![Constraint::Min(3); rows]);

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
            let tasks = app.tasks_for(&date);

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

            let max_items = 2usize;
            let mut count = 0;

            for event in events.iter() {
                if count >= max_items {
                    break;
                }
                let style = if event.is_holiday {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default().fg(Color::Green)
                };
                let truncated = truncate_to_width(&event.title, cell_inner.width as usize);
                lines.push(Line::from(Span::styled(truncated, style)));
                count += 1;
            }

            for task in tasks.iter() {
                if count >= max_items {
                    break;
                }
                let (prefix, style) = task_display_style(task);
                let title_width = cell_inner.width.saturating_sub(prefix.len() as u16) as usize;
                let truncated = format!("{}{}", prefix, truncate_to_width(&task.title, title_width));
                lines.push(Line::from(Span::styled(truncated, style)));
                count += 1;
            }

            let total = events.len() + tasks.len();
            if total > max_items {
                lines.push(Line::from(Span::styled(
                    format!("+{}", total - max_items),
                    Style::default().fg(Color::DarkGray),
                )));
            }

            f.render_widget(Paragraph::new(lines), cell_inner);
        }
    }
}

fn task_display_style(task: &Task) -> (&'static str, Style) {
    if task.completed {
        ("☑ ", Style::default().fg(Color::DarkGray))
    } else {
        ("□ ", Style::default().fg(Color::Yellow))
    }
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

fn truncate_to_width(s: &str, max_cols: usize) -> String {
    if s.width() <= max_cols {
        return s.to_string();
    }
    let target = max_cols.saturating_sub(1);
    let mut used = 0;
    let mut end = 0;
    for c in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
        if used + cw > target {
            break;
        }
        used += cw;
        end += c.len_utf8();
    }
    format!("{}…", &s[..end])
}

// ─── Sidebar ────────────────────────────────────────────────────────────────

fn render_sidebar(f: &mut Frame, area: Rect, app: &App) {
    match &app.mode {
        AppMode::AddEvent { title } => render_add_event(f, area, app, title),
        AppMode::AddTask { title } => render_add_task(f, area, app, title),
        AppMode::AddTaskSelectList { title, selected } => {
            render_select_list(f, area, app, title, *selected)
        }
        AppMode::AddTaskNewList { title, new_list } => {
            render_new_list(f, area, app, title, new_list)
        }
        AppMode::DeleteConfirm { event_index } => {
            render_delete_confirm(f, area, app, *event_index)
        }
        AppMode::DeleteTaskConfirm { task_index } => {
            render_delete_task_confirm(f, area, app, *task_index)
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
    let tasks = app.selected_tasks();

    let mut items: Vec<ListItem> = Vec::new();

    if events.is_empty() && tasks.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "予定・タスクなし",
            Style::default().fg(Color::DarkGray),
        ))));
    } else {
        for e in events.iter() {
            let time_str = match (&e.start_time, &e.end_time) {
                (Some(s), Some(end)) if s != end => format!("{}-{} ", s, end),
                (Some(s), _) => format!("{} ", s),
                _ => String::new(),
            };
            let style = if e.is_holiday {
                Style::default().fg(Color::Red)
            } else {
                Style::default()
            };
            items.push(ListItem::new(Line::from(vec![
                Span::styled(time_str, Style::default().fg(Color::Cyan)),
                Span::styled(e.title.clone(), style),
            ])));
        }

        if !events.is_empty() && !tasks.is_empty() {
            items.push(ListItem::new(Line::from(Span::styled(
                "── タスク ──",
                Style::default().fg(Color::DarkGray),
            ))));
        }

        for task in tasks.iter() {
            let (prefix, style) = task_display_style(task);
            let list_name = app
                .task_lists
                .iter()
                .find(|l| l.id == task.list_id)
                .map(|l| l.title.as_str())
                .unwrap_or("");
            let mut spans = vec![Span::styled(prefix, style)];
            if !list_name.is_empty() {
                spans.push(Span::styled(
                    format!("[{}] ", list_name),
                    Style::default().fg(Color::Magenta),
                ));
            }
            spans.push(Span::styled(task.title.clone(), style));
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
            Line::from(Span::styled("a:予定追加 t:タスク追加 c:完了切替", Style::default().fg(Color::DarkGray))),
            Line::from(Span::styled("d:予定削除 D:タスク削除", Style::default().fg(Color::DarkGray))),
            Line::from(Span::styled("[]:月移動  q:終了", Style::default().fg(Color::DarkGray))),
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
        input_inner.x + title_input.chars().count() as u16,
        input_inner.y,
    ));

    f.render_widget(
        Paragraph::new("Enter:確定  Esc:キャンセル")
            .style(Style::default().fg(Color::DarkGray)),
        v[2],
    );
}

fn render_add_task(f: &mut Frame, area: Rect, app: &App, title_input: &str) {
    let d = app.selected_date;
    let block = Block::default()
        .title(format!(" タスク追加 — {}月{}日 ", d.month(), d.day()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
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
        input_inner.x + title_input.chars().count() as u16,
        input_inner.y,
    ));

    f.render_widget(
        Paragraph::new("Enter:次へ  Esc:キャンセル")
            .style(Style::default().fg(Color::DarkGray)),
        v[2],
    );
}

fn render_select_list(
    f: &mut Frame,
    area: Rect,
    app: &App,
    title: &str,
    selected: usize,
) {
    let d = app.selected_date;
    let block = Block::default()
        .title(format!(" リスト選択 — {}月{}日 ", d.month(), d.day()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

    f.render_widget(
        Paragraph::new(format!("「{}」", title))
            .style(Style::default().fg(Color::White)),
        v[0],
    );
    f.render_widget(
        Paragraph::new("追加先のリスト:")
            .style(Style::default().fg(Color::DarkGray)),
        v[1],
    );

    let mut items: Vec<ListItem> = Vec::new();

    // 既存タスクリスト
    for (i, list) in app.task_lists.iter().enumerate() {
        let is_sel = selected == i;
        let style = if is_sel {
            Style::default().fg(Color::Black).bg(Color::Magenta).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Magenta)
        };
        let prefix = if is_sel { "> " } else { "  " };
        items.push(ListItem::new(Line::from(Span::styled(
            format!("{}{}", prefix, list.title),
            style,
        ))));
    }

    // 新規リスト作成
    let new_idx = app.task_lists.len();
    let is_new_sel = selected == new_idx;
    let new_style = if is_new_sel {
        Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };
    let new_prefix = if is_new_sel { "> " } else { "  " };
    items.push(ListItem::new(Line::from(Span::styled(
        format!("{}+ 新規リスト作成", new_prefix),
        new_style,
    ))));

    f.render_widget(List::new(items), v[2]);

    f.render_widget(
        Paragraph::new("j/k:移動  Enter:選択  Esc:戻る")
            .style(Style::default().fg(Color::DarkGray)),
        v[3],
    );
}

fn render_new_list(
    f: &mut Frame,
    area: Rect,
    app: &App,
    title: &str,
    new_list: &str,
) {
    let d = app.selected_date;
    let block = Block::default()
        .title(format!(" 新規リスト作成 — {}月{}日 ", d.month(), d.day()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green));
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
        Paragraph::new(format!("「{}」", title))
            .style(Style::default().fg(Color::White)),
        v[0],
    );
    f.render_widget(Paragraph::new("リスト名:"), v[1]);

    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green));
    let input_inner = input_block.inner(v[2]);
    f.render_widget(input_block, v[2]);
    f.render_widget(
        Paragraph::new(new_list).style(Style::default().fg(Color::White)),
        input_inner,
    );
    f.set_cursor_position((
        input_inner.x + new_list.chars().count() as u16,
        input_inner.y,
    ));

    f.render_widget(
        Paragraph::new("Enter:作成&追加  Esc:戻る")
            .style(Style::default().fg(Color::DarkGray)),
        v[3],
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
                    event.title.clone(),
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

fn render_delete_task_confirm(f: &mut Frame, area: Rect, app: &App, idx: usize) {
    let block = Block::default()
        .title(" タスクを削除 ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if let Some(task) = app.selected_tasks().get(idx) {
        let list_name = app
            .task_lists
            .iter()
            .find(|l| l.id == task.list_id)
            .map(|l| l.title.as_str())
            .unwrap_or("");
        f.render_widget(
            Paragraph::new(vec![
                Line::from("以下のタスクを削除しますか？"),
                Line::from(""),
                Line::from(Span::styled(
                    format!("□ {}", task.title),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    format!("  リスト: {}", list_name),
                    Style::default().fg(Color::Magenta),
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
