use crate::app::{App, AppMode};
use crate::matrix::{self, truncate_to_width};
use crate::meta::{self, StackCategory};
use crate::tasks::Task;
use chrono::{Datelike, Local};
use unicode_width::UnicodeWidthStr;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

const DAY_NAMES: [&str; 7] = ["月", "火", "水", "木", "金", "土", "日"];

pub fn draw(f: &mut Frame, app: &App) {
    if is_matrix_mode(&app.mode) {
        render_matrix_screen(f, f.area(), app);
        return;
    }

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

            let total = events.len() + tasks.len();
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
                let truncated = truncate_to_width(&event.title, cell_inner.width as usize);
                lines.push(Line::from(Span::styled(truncated, style)));
                count += 1;
            }

            for task in tasks.iter() {
                if count >= display_limit {
                    break;
                }
                let (prefix, style) = task_display_style(task);
                let title_width = cell_inner.width.saturating_sub(prefix.len() as u16) as usize;
                let truncated = format!("{}{}", prefix, truncate_to_width(&task.title, title_width));
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

fn task_display_style(task: &Task) -> (&'static str, Style) {
    if task.completed {
        ("☑ ", Style::default().fg(Color::DarkGray))
    } else {
        ("□ ", Style::default().fg(Color::Yellow))
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
        AppMode::SelectItemDelete { selected } => {
            render_select_item_delete(f, area, app, *selected)
        }
        AppMode::SelectItemEdit { selected } => {
            render_select_item_edit(f, area, app, *selected)
        }
        AppMode::SelectTaskToggle { selected } => {
            render_select_task_toggle(f, area, app, *selected)
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
        AppMode::EditTask { task_index, title } => {
            render_edit_task(f, area, app, *task_index, title)
        }
        AppMode::EditTaskSelectList { task_index, title, selected } => {
            render_edit_task_select_list(f, area, app, *task_index, title, *selected)
        }
        AppMode::Normal => render_normal_sidebar(f, area, app),
        // マトリックス系モードは draw() の先頭で全画面描画されるためここには来ない
        _ => {}
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
                    format!("[{}] ", cal_name),
                    Style::default().fg(Color::Magenta),
                ));
            }
            spans.push(Span::styled(e.title.clone(), style));
            items.push(ListItem::new(Line::from(spans)));
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
            Line::from(Span::styled("a:予定追加 n:タスク追加", Style::default().fg(Color::DarkGray))),
            Line::from(Span::styled("t:完了切替 d:削除 e:編集  []:月移動", Style::default().fg(Color::DarkGray))),
            Line::from(Span::styled("hjkl:日移動  q:終了", Style::default().fg(Color::DarkGray))),
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
        input_inner.x + title_input.width() as u16,
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
        input_inner.x + new_list.width() as u16,
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

fn build_item_list(app: &App, selected: usize, tasks_only: bool) -> Vec<ListItem<'static>> {
    let events = app.selected_events();
    let tasks = app.selected_tasks();

    let non_holiday_events: Vec<(usize, &_)> = if tasks_only {
        vec![]
    } else {
        events.iter().enumerate().filter(|(_, e)| !e.is_holiday).collect()
    };

    let mut items: Vec<ListItem<'static>> = Vec::new();
    let mut idx = 0usize;

    if !non_holiday_events.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "── 予定 ──",
            Style::default().fg(Color::DarkGray),
        ))));
        for (_, event) in &non_holiday_events {
            let is_sel = idx == selected;
            let style = if is_sel {
                Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            };
            let prefix = if is_sel { "> " } else { "  " };
            items.push(ListItem::new(Line::from(Span::styled(
                format!("{}{}", prefix, event.title),
                style,
            ))));
            idx += 1;
        }
    }

    if !tasks.is_empty() {
        if !tasks_only {
            items.push(ListItem::new(Line::from(Span::styled(
                "── タスク ──",
                Style::default().fg(Color::DarkGray),
            ))));
        }
        for task in tasks.iter() {
            let is_sel = idx == selected;
            let (prefix_icon, base_style) = task_display_style(task);
            let style = if is_sel {
                Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                base_style
            };
            let arrow = if is_sel { "> " } else { "  " };
            items.push(ListItem::new(Line::from(Span::styled(
                format!("{}{}{}", arrow, prefix_icon, task.title),
                style,
            ))));
            idx += 1;
        }
    }

    items
}

fn render_select_item_delete(f: &mut Frame, area: Rect, app: &App, selected: usize) {
    let d = app.selected_date;
    let block = Block::default()
        .title(format!(" 削除する項目を選択 — {}月{}日 ", d.month(), d.day()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    f.render_widget(List::new(build_item_list(app, selected, false)), v[0]);
    f.render_widget(
        Paragraph::new("j/k:移動  Enter:削除  Esc:キャンセル")
            .style(Style::default().fg(Color::DarkGray)),
        v[1],
    );
}

fn render_select_item_edit(f: &mut Frame, area: Rect, app: &App, selected: usize) {
    let d = app.selected_date;
    let block = Block::default()
        .title(format!(" 編集する項目を選択 — {}月{}日 ", d.month(), d.day()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    f.render_widget(List::new(build_item_list(app, selected, false)), v[0]);
    f.render_widget(
        Paragraph::new("j/k:移動  Enter:編集  Esc:キャンセル")
            .style(Style::default().fg(Color::DarkGray)),
        v[1],
    );
}

fn render_select_task_toggle(f: &mut Frame, area: Rect, app: &App, selected: usize) {
    let d = app.selected_date;
    let block = Block::default()
        .title(format!(" 完了切替するタスクを選択 — {}月{}日 ", d.month(), d.day()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    f.render_widget(List::new(build_item_list(app, selected, true)), v[0]);
    f.render_widget(
        Paragraph::new("j/k:移動  Enter:切替  Esc:キャンセル")
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
        Paragraph::new(format!("変更前: {}", original))
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
        Paragraph::new(format!("予定: {}", original)).style(Style::default().fg(Color::DarkGray)),
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
        Paragraph::new(format!("予定: {}", original)).style(Style::default().fg(Color::DarkGray)),
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

fn render_edit_task(f: &mut Frame, area: Rect, app: &App, task_index: usize, title_input: &str) {
    let d = app.selected_date;
    let original = app
        .selected_tasks()
        .get(task_index)
        .map(|t| t.title.as_str())
        .unwrap_or("");
    let block = Block::default()
        .title(format!(" タスクを編集 — {}月{}日 ", d.month(), d.day()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
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
        Paragraph::new(format!("変更前: {}", original))
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
        Paragraph::new("Enter:次へ(リスト選択)  Esc:キャンセル")
            .style(Style::default().fg(Color::DarkGray)),
        v[3],
    );
}

fn render_edit_task_select_list(
    f: &mut Frame,
    area: Rect,
    app: &App,
    task_index: usize,
    title: &str,
    selected: usize,
) {
    let current_list_id = app
        .selected_tasks()
        .get(task_index)
        .map(|t| t.list_id.as_str())
        .unwrap_or("");
    let block = Block::default()
        .title(" リストを選択 ")
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
        Paragraph::new("移動先のリスト:")
            .style(Style::default().fg(Color::DarkGray)),
        v[1],
    );

    let mut items: Vec<ListItem> = Vec::new();
    for (i, list) in app.task_lists.iter().enumerate() {
        let is_sel = selected == i;
        let is_current = list.id == current_list_id;
        let label = if is_current {
            format!("{} (現在)", list.title)
        } else {
            list.title.clone()
        };
        let style = if is_sel {
            Style::default().fg(Color::Black).bg(Color::Magenta).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Magenta)
        };
        let prefix = if is_sel { "> " } else { "  " };
        items.push(ListItem::new(Line::from(Span::styled(
            format!("{}{}", prefix, label),
            style,
        ))));
    }

    f.render_widget(List::new(items), v[2]);

    f.render_widget(
        Paragraph::new("j/k:移動  Enter:確定  Esc:戻る")
            .style(Style::default().fg(Color::DarkGray)),
        v[3],
    );
}


// ─── マトリックス画面 ─────────────────────────────────────────────────────────

fn is_matrix_mode(mode: &AppMode) -> bool {
    matches!(
        mode,
        AppMode::Matrix
            | AppMode::MatrixAddTitle { .. }
            | AppMode::MatrixAddList { .. }
            | AppMode::MatrixAddDate { .. }
            | AppMode::MatrixAddImp { .. }
            | AppMode::MatrixAddClau { .. }
            | AppMode::MatrixStackCat { .. }
            | AppMode::MatrixStackNote { .. }
            | AppMode::MatrixDeleteConfirm
    )
}

fn render_matrix_screen(f: &mut Frame, area: Rect, app: &App) {
    let (graph_area, sidebar_area) = matrix::split_layout(area);
    render_matrix_graph(f, graph_area, app);

    match &app.mode {
        AppMode::MatrixAddTitle { editing, title } => render_matrix_input_panel(
            f,
            sidebar_area,
            if editing.is_some() { " タスク編集 1/5 " } else { " タスク追加 1/5 " },
            "タスク名:",
            title,
            &[],
            "Enter:次へ  Esc:キャンセル",
        ),
        AppMode::MatrixAddList { editing, title, selected } => {
            render_matrix_list_panel(f, sidebar_area, app, title, *selected, editing.is_some())
        }
        AppMode::MatrixAddDate { editing, title, date_input, .. } => render_matrix_input_panel(
            f,
            sidebar_area,
            if editing.is_some() { " タスク編集 3/5 " } else { " タスク追加 3/5 " },
            "日付 (YYYY-MM-DD, 空白=なし):",
            date_input,
            &[format!("「{}」", title)],
            "Enter:次へ  Esc:戻る",
        ),
        AppMode::MatrixAddImp { editing, title, input, .. } => render_matrix_input_panel(
            f,
            sidebar_area,
            if editing.is_some() { " タスク編集 4/5 " } else { " タスク追加 4/5 " },
            "重要度 (0-10):",
            input,
            &[format!("「{}」", title), "上に行くほど重要".to_string()],
            "Enter:次へ  Esc:戻る",
        ),
        AppMode::MatrixAddClau { editing, title, imp, input, .. } => render_matrix_input_panel(
            f,
            sidebar_area,
            if editing.is_some() { " タスク編集 5/5 " } else { " タスク追加 5/5 " },
            "clau度 (0-10):",
            input,
            &[
                format!("「{}」 重要度:{}", title, imp),
                "Claudeに任せられる度合い".to_string(),
            ],
            if editing.is_some() { "Enter:更新  Esc:戻る" } else { "Enter:追加  Esc:戻る" },
        ),
        AppMode::MatrixStackCat { selected } => {
            render_matrix_stack_cat_panel(f, sidebar_area, app, *selected)
        }
        AppMode::MatrixStackNote { category, note } => {
            let cat_label = category.map(|c| c.label()).unwrap_or("なし");
            render_matrix_input_panel(
                f,
                sidebar_area,
                " スタック詳細 ",
                "詳細 (何に阻まれているか):",
                note,
                &[format!("カテゴリー: {}", cat_label)],
                "Enter:確定  Esc:戻る",
            )
        }
        AppMode::MatrixDeleteConfirm => render_matrix_delete_confirm(f, sidebar_area, app),
        _ => render_matrix_detail_sidebar(f, sidebar_area, app),
    }
}

fn stack_color(stack: Option<StackCategory>) -> Color {
    // スタック済みは種類を問わず寒色(LightCyan)に統一、未スタックは白
    match stack {
        Some(_) => Color::LightCyan,
        None => Color::White,
    }
}

fn render_matrix_graph(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .title(" タスクマトリックス ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.width < 10 || inner.height < 5 {
        return;
    }

    let cx = inner.x + inner.width / 2;
    let cy = inner.y + inner.height / 2;
    let axis_style = Style::default().fg(Color::DarkGray);

    // 配置はタスクラベル描画にも使うので先に計算(app側のhjkl移動と同一結果)
    let placed = matrix::compute_layout(&app.matrix_items(), inner);

    let buf = f.buffer_mut();

    // 横軸(右端が矢印)・縦軸(上端が矢印)
    let h_line = format!("{}→", "─".repeat(inner.width.saturating_sub(1) as usize));
    buf.set_string(inner.x, cy, h_line, axis_style);
    for y in inner.y..inner.y + inner.height {
        buf.set_string(cx, y, "│", axis_style);
    }
    buf.set_string(cx, cy, "┼", axis_style);
    buf.set_string(cx, inner.y, "↑", axis_style);

    // 軸ラベルと目盛
    buf.set_string(cx + 2, inner.y, "重要度", axis_style);
    let clau_label = "clau度";
    let label_y = if cy + 1 < inner.y + inner.height { cy + 1 } else { cy - 1 };
    let label_x = (inner.x + inner.width).saturating_sub(clau_label.width() as u16);
    buf.set_string(label_x, label_y, clau_label, axis_style);
    buf.set_string(inner.x, label_y, "1", axis_style);
    buf.set_string(cx + 1, inner.y + inner.height - 1, "1", axis_style);

    // タスクラベル(軸の上に重ねて描く)
    for p in &placed {
        let is_selected = app.matrix_selected.as_deref() == Some(p.task_id.as_str());
        let stack = app.meta_store.get(&p.task_id).stack;
        let color = stack_color(stack);
        let style = if is_selected {
            Style::default()
                .fg(Color::Black)
                .bg(color)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(color)
        };
        buf.set_string(p.x, p.y, &p.label, style);
    }
}

fn render_matrix_detail_sidebar(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .title(" 詳細 ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let label_style = Style::default().fg(Color::DarkGray);
    let mut lines: Vec<Line> = Vec::new();

    if let Some(task) = app.selected_matrix_task() {
        let m = app.meta_store.get(&task.id);
        let list_name = app
            .task_lists
            .iter()
            .find(|l| l.id == task.list_id)
            .map(|l| l.title.as_str())
            .unwrap_or("-");
        let pri = app
            .matrix_items()
            .iter()
            .find(|i| i.task_id == task.id)
            .and_then(|i| i.pri)
            .map(|p| p.to_string())
            .unwrap_or_else(|| "-".to_string());
        let due = task
            .due
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "-".to_string());

        lines.push(Line::from(vec![
            Span::styled("優先順位: ", label_style),
            Span::styled(pri, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("タスク名: ", label_style),
            Span::styled(task.title.clone(), Style::default().fg(Color::White)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("カテゴリー: ", label_style),
            Span::styled(list_name.to_string(), Style::default().fg(Color::Magenta)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("日付: ", label_style),
            Span::raw(due),
        ]));
        lines.push(Line::from(vec![
            Span::styled("重要度: ", label_style),
            Span::raw(m.imp.to_string()),
        ]));
        lines.push(Line::from(vec![
            Span::styled("clau度: ", label_style),
            Span::raw(m.clau.to_string()),
        ]));
        lines.push(Line::from(vec![
            Span::styled("スタック: ", label_style),
            Span::styled(
                m.stack.map(|c| c.label().to_string()).unwrap_or_else(|| "なし".to_string()),
                Style::default().fg(stack_color(m.stack)),
            ),
        ]));
        if let Some(note) = &m.stack_note {
            lines.push(Line::from(vec![
                Span::styled("スタック詳細: ", label_style),
                Span::styled(note.clone(), Style::default().fg(stack_color(m.stack))),
            ]));
        }
        let (body, _) = meta::parse_notes(task.notes.as_deref().unwrap_or(""));
        if !body.is_empty() {
            lines.push(Line::from(Span::styled("メモ:", label_style)));
            for l in body.lines().take(4) {
                lines.push(Line::from(Span::raw(l.to_string())));
            }
        }
    } else {
        lines.push(Line::from("タスクがありません"));
        lines.push(Line::from(Span::styled("n キーで追加できます", label_style)));
    }

    lines.push(Line::from(""));
    if let Some(msg) = &app.status_message {
        lines.push(Line::from(Span::styled(
            msg.clone(),
            Style::default().fg(Color::Yellow),
        )));
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled("hjkl:選択移動", label_style)));
    lines.push(Line::from(Span::styled("1-9,0:優先順位で選択", label_style)));
    lines.push(Line::from(Span::styled("n:追加  e:編集  t:完了  d:削除", label_style)));
    lines.push(Line::from(Span::styled("s:スタック  T/q:カレンダーへ", label_style)));

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/// ラベル+入力枠+ヘルプの汎用入力パネル(タスク追加・スタック詳細で共用)
fn render_matrix_input_panel(
    f: &mut Frame,
    area: Rect,
    title: &str,
    label: &str,
    input: &str,
    context: &[String],
    help: &str,
) {
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::White));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut constraints = vec![Constraint::Length(1); context.len()];
    constraints.push(Constraint::Length(1));
    constraints.push(Constraint::Length(3));
    constraints.push(Constraint::Length(1));
    constraints.push(Constraint::Min(0));
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    for (i, line) in context.iter().enumerate() {
        f.render_widget(
            Paragraph::new(line.as_str()).style(Style::default().fg(Color::Magenta)),
            v[i],
        );
    }
    let base = context.len();
    f.render_widget(
        Paragraph::new(label).style(Style::default().fg(Color::DarkGray)),
        v[base],
    );

    let input_block = Block::default().borders(Borders::ALL);
    let input_inner = input_block.inner(v[base + 1]);
    f.render_widget(input_block, v[base + 1]);
    f.render_widget(
        Paragraph::new(input).style(Style::default().fg(Color::White)),
        input_inner,
    );
    f.set_cursor_position((input_inner.x + input.width() as u16, input_inner.y));

    f.render_widget(
        Paragraph::new(help).style(Style::default().fg(Color::DarkGray)),
        v[base + 2],
    );
}

fn render_matrix_list_panel(
    f: &mut Frame,
    area: Rect,
    app: &App,
    title: &str,
    selected: usize,
    is_edit: bool,
) {
    let block = Block::default()
        .title(if is_edit { " タスク編集 2/5 " } else { " タスク追加 2/5 " })
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::White));
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
        Paragraph::new(format!("「{}」", title)).style(Style::default().fg(Color::White)),
        v[0],
    );
    f.render_widget(
        Paragraph::new("追加先のリスト:").style(Style::default().fg(Color::DarkGray)),
        v[1],
    );

    let items: Vec<ListItem> = app
        .task_lists
        .iter()
        .enumerate()
        .map(|(i, list)| {
            let is_sel = i == selected;
            let style = if is_sel {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Magenta)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Magenta)
            };
            ListItem::new(Line::from(Span::styled(
                format!("{}{}", if is_sel { "> " } else { "  " }, list.title),
                style,
            )))
        })
        .collect();
    f.render_widget(List::new(items), v[2]);

    f.render_widget(
        Paragraph::new("j/k:移動  Enter:次へ  Esc:戻る").style(Style::default().fg(Color::DarkGray)),
        v[3],
    );
}

fn render_matrix_stack_cat_panel(f: &mut Frame, area: Rect, app: &App, selected: usize) {
    let block = Block::default()
        .title(" スタックカテゴリー ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
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

    let task_title = app
        .selected_matrix_task()
        .map(|t| t.title.as_str())
        .unwrap_or("");
    f.render_widget(
        Paragraph::new(format!("「{}」", task_title)).style(Style::default().fg(Color::White)),
        v[0],
    );
    f.render_widget(
        Paragraph::new("何に阻まれていますか:").style(Style::default().fg(Color::DarkGray)),
        v[1],
    );

    let mut items: Vec<ListItem> = StackCategory::ALL
        .iter()
        .enumerate()
        .map(|(i, cat)| {
            let is_sel = i == selected;
            let color = stack_color(Some(*cat));
            let style = if is_sel {
                Style::default().fg(Color::Black).bg(color).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(color)
            };
            ListItem::new(Line::from(Span::styled(
                format!("{}{}", if is_sel { "> " } else { "  " }, cat.label()),
                style,
            )))
        })
        .collect();
    let clear_idx = StackCategory::ALL.len();
    let is_clear_sel = selected == clear_idx;
    items.push(ListItem::new(Line::from(Span::styled(
        format!("{}スタック解除", if is_clear_sel { "> " } else { "  " }),
        if is_clear_sel {
            Style::default().fg(Color::Black).bg(Color::White).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        },
    ))));
    f.render_widget(List::new(items), v[2]);

    f.render_widget(
        Paragraph::new("j/k:移動  Enter:決定  Esc:戻る").style(Style::default().fg(Color::DarkGray)),
        v[3],
    );
}

fn render_matrix_delete_confirm(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .title(" タスクを削除 ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if let Some(task) = app.selected_matrix_task() {
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
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
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
            ])
            .wrap(Wrap { trim: false }),
            inner,
        );
    }
}
