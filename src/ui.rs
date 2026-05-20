use crate::app::{App, AppMode};
use crate::tasks::Task;
use chrono::{Datelike, Local};
use unicode_width::UnicodeWidthStr;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame,
};

const DAY_NAMES: [&str; 7] = ["月", "火", "水", "木", "金", "土", "日"];

pub fn draw(f: &mut Frame, app: &App) {
    if matches!(
        &app.mode,
        AppMode::TaskList { .. }
            | AppMode::TaskListAdd { .. }
            | AppMode::TaskListAddSelectList { .. }
            | AppMode::TaskListAddDate { .. }
            | AppMode::TaskListEdit { .. }
            | AppMode::TaskListEditSelectList { .. }
            | AppMode::TaskListEditDate { .. }
            | AppMode::TaskListDeleteConfirm { .. }
    ) {
        render_task_list_screen(f, f.area(), app);
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
        AppMode::EditTask { task_index, title } => {
            render_edit_task(f, area, app, *task_index, title)
        }
        AppMode::EditTaskSelectList { task_index, title, selected } => {
            render_edit_task_select_list(f, area, app, *task_index, title, *selected)
        }
        AppMode::Normal => render_normal_sidebar(f, area, app),
        AppMode::TaskList { .. }
        | AppMode::TaskListAdd { .. }
        | AppMode::TaskListAddSelectList { .. }
        | AppMode::TaskListAddDate { .. }
        | AppMode::TaskListEdit { .. }
        | AppMode::TaskListEditSelectList { .. }
        | AppMode::TaskListEditDate { .. }
        | AppMode::TaskListDeleteConfirm { .. } => {}
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

// ─── タスク一覧画面 ─────────────────────────────────────────────────────────────

fn render_task_list_screen(f: &mut Frame, area: Rect, app: &App) {
    match &app.mode {
        AppMode::TaskList { selected } => render_task_list_main(f, area, app, *selected),
        AppMode::TaskListAdd { title } => render_task_list_with_overlay(f, area, app, render_tl_add_overlay(title)),
        AppMode::TaskListAddSelectList { title, selected_list } => {
            render_task_list_with_overlay(f, area, app, render_tl_select_list_overlay(app, title, *selected_list, false))
        }
        AppMode::TaskListAddDate { title, list_id, date_input } => {
            let list_name = app.task_lists.iter().find(|l| &l.id == list_id).map(|l| l.title.as_str()).unwrap_or("");
            render_task_list_with_overlay(f, area, app, render_tl_date_overlay(title, list_name, date_input, false))
        }
        AppMode::TaskListEdit { task_index, title } => {
            let original = app.all_tasks.get(*task_index).map(|t| t.title.as_str()).unwrap_or("");
            render_task_list_with_overlay(f, area, app, render_tl_edit_overlay(original, title))
        }
        AppMode::TaskListEditSelectList { task_index, title, selected_list } => {
            let task = app.all_tasks.get(*task_index);
            let current_list_id = task.map(|t| t.list_id.as_str()).unwrap_or("");
            render_task_list_with_overlay(f, area, app, render_tl_select_list_overlay_edit(app, title, *selected_list, current_list_id))
        }
        AppMode::TaskListEditDate { task_index: _, title, list_id, date_input } => {
            let list_name = app.task_lists.iter().find(|l| &l.id == list_id).map(|l| l.title.as_str()).unwrap_or("");
            render_task_list_with_overlay(f, area, app, render_tl_date_overlay(title, list_name, date_input, true))
        }
        AppMode::TaskListDeleteConfirm { task_index } => {
            render_task_list_delete_confirm(f, area, app, *task_index)
        }
        _ => {}
    }
}

fn render_task_list_main(f: &mut Frame, area: Rect, app: &App, selected: usize) {
    let block = Block::default()
        .title(" タスク一覧  T/Esc:カレンダーへ  n:追加  t:完了切替  e:編集  d:削除 ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let tasks = &app.all_tasks;

    if tasks.is_empty() {
        f.render_widget(
            Paragraph::new("タスクなし  n:新規追加").style(Style::default().fg(Color::DarkGray)),
            v[0],
        );
    } else {
        let items: Vec<ListItem> = tasks.iter().enumerate().map(|(i, task)| {
            let is_sel = i == selected;
            let list_name = app.task_lists.iter().find(|l| l.id == task.list_id)
                .map(|l| l.title.as_str()).unwrap_or("");
            let (icon, base_style) = task_display_style(task);
            let due_str = task.due.map(|d| format!("  {}", d.format("%m/%d"))).unwrap_or_default();

            if is_sel {
                let sel_style = Style::default().fg(Color::Black).bg(Color::White).add_modifier(Modifier::BOLD);
                ListItem::new(Line::from(vec![
                    Span::styled(format!("> {}", icon), sel_style),
                    Span::styled(format!("[{}] ", list_name), sel_style),
                    Span::styled(task.title.clone(), sel_style),
                    Span::styled(due_str, sel_style),
                ]))
            } else {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("  {}", icon), base_style),
                    Span::styled(format!("[{}] ", list_name), Style::default().fg(Color::Magenta)),
                    Span::styled(task.title.clone(), base_style),
                    Span::styled(due_str, Style::default().fg(Color::DarkGray)),
                ]))
            }
        }).collect();

        let mut list_state = ListState::default().with_selected(Some(selected));
        let mut scroll_state = ScrollbarState::new(tasks.len()).position(selected);
        f.render_stateful_widget(List::new(items), v[0], &mut list_state);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            v[0],
            &mut scroll_state,
        );
    }

    if let Some(msg) = &app.status_message {
        f.render_widget(
            Paragraph::new(msg.as_str()).style(Style::default().fg(Color::White)),
            v[1],
        );
    }
}

fn render_task_list_with_overlay(
    f: &mut Frame,
    area: Rect,
    app: &App,
    overlay_fn: impl Fn(&mut Frame, Rect),
) {
    let selected = match &app.mode {
        AppMode::TaskListAdd { .. }
        | AppMode::TaskListAddSelectList { .. }
        | AppMode::TaskListAddDate { .. } => 0,
        AppMode::TaskListEdit { task_index, .. }
        | AppMode::TaskListEditSelectList { task_index, .. }
        | AppMode::TaskListEditDate { task_index, .. } => *task_index,
        _ => 0,
    };

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(40)])
        .split(area);

    render_task_list_main(f, chunks[0], app, selected);
    overlay_fn(f, chunks[1]);
}

fn render_tl_add_overlay(title: &str) -> impl Fn(&mut Frame, Rect) + '_ {
    move |f: &mut Frame, area: Rect| {
        let block = Block::default()
            .title(" タスク追加 ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::White));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let v = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(3), Constraint::Length(1), Constraint::Min(0)])
            .split(inner);

        f.render_widget(Paragraph::new("タイトル:"), v[0]);

        let input_block = Block::default().borders(Borders::ALL);
        let input_inner = input_block.inner(v[1]);
        f.render_widget(input_block, v[1]);
        f.render_widget(Paragraph::new(title).style(Style::default().fg(Color::White)), input_inner);
        f.set_cursor_position((input_inner.x + title.width() as u16, input_inner.y));

        f.render_widget(
            Paragraph::new("Enter:次へ  Esc:キャンセル").style(Style::default().fg(Color::DarkGray)),
            v[2],
        );
    }
}

fn render_tl_select_list_overlay<'a>(
    app: &'a App,
    title: &'a str,
    selected_list: usize,
    _edit: bool,
) -> impl Fn(&mut Frame, Rect) + 'a {
    move |f: &mut Frame, area: Rect| {
        let block = Block::default()
            .title(" リスト選択 ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::White));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let v = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)])
            .split(inner);

        f.render_widget(
            Paragraph::new(format!("「{}」", title)).style(Style::default().fg(Color::White)),
            v[0],
        );
        f.render_widget(
            Paragraph::new("追加先のリスト:").style(Style::default().fg(Color::DarkGray)),
            v[1],
        );

        let mut items: Vec<ListItem> = app.task_lists.iter().enumerate().map(|(i, list)| {
            let is_sel = i == selected_list;
            let style = if is_sel {
                Style::default().fg(Color::Black).bg(Color::Magenta).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Magenta)
            };
            ListItem::new(Line::from(Span::styled(
                format!("{}{}", if is_sel { "> " } else { "  " }, list.title),
                style,
            )))
        }).collect();

        let new_idx = app.task_lists.len();
        let is_new_sel = selected_list == new_idx;
        items.push(ListItem::new(Line::from(Span::styled(
            format!("{}+ 新規リスト作成", if is_new_sel { "> " } else { "  " }),
            if is_new_sel {
                Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            },
        ))));

        f.render_widget(List::new(items), v[2]);
        f.render_widget(
            Paragraph::new("j/k:移動  Enter:次へ  Esc:戻る").style(Style::default().fg(Color::DarkGray)),
            v[3],
        );
    }
}

fn render_tl_select_list_overlay_edit<'a>(
    app: &'a App,
    title: &'a str,
    selected_list: usize,
    current_list_id: &'a str,
) -> impl Fn(&mut Frame, Rect) + 'a {
    move |f: &mut Frame, area: Rect| {
        let block = Block::default()
            .title(" リスト選択 ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::White));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let v = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)])
            .split(inner);

        f.render_widget(
            Paragraph::new(format!("「{}」", title)).style(Style::default().fg(Color::White)),
            v[0],
        );
        f.render_widget(
            Paragraph::new("移動先のリスト:").style(Style::default().fg(Color::DarkGray)),
            v[1],
        );

        let items: Vec<ListItem> = app.task_lists.iter().enumerate().map(|(i, list)| {
            let is_sel = i == selected_list;
            let is_current = list.id == current_list_id;
            let label = if is_current { format!("{} (現在)", list.title) } else { list.title.clone() };
            let style = if is_sel {
                Style::default().fg(Color::Black).bg(Color::Magenta).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Magenta)
            };
            ListItem::new(Line::from(Span::styled(
                format!("{}{}", if is_sel { "> " } else { "  " }, label),
                style,
            )))
        }).collect();

        f.render_widget(List::new(items), v[2]);
        f.render_widget(
            Paragraph::new("j/k:移動  Enter:次へ  Esc:戻る").style(Style::default().fg(Color::DarkGray)),
            v[3],
        );
    }
}

fn render_tl_date_overlay<'a>(
    title: &'a str,
    list_name: &'a str,
    date_input: &'a str,
    is_edit: bool,
) -> impl Fn(&mut Frame, Rect) + 'a {
    move |f: &mut Frame, area: Rect| {
        let label = if is_edit { " 日付を変更 " } else { " 日付を設定 " };
        let block = Block::default()
            .title(label)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::White));
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
            Paragraph::new(format!("「{}」", title)).style(Style::default().fg(Color::White)),
            v[0],
        );
        f.render_widget(
            Paragraph::new(format!("リスト: {}", list_name)).style(Style::default().fg(Color::Magenta)),
            v[1],
        );
        f.render_widget(
            Paragraph::new("日付 (YYYY-MM-DD, 空白=なし):").style(Style::default().fg(Color::DarkGray)),
            v[2],
        );

        let input_block = Block::default().borders(Borders::ALL);
        let input_inner = input_block.inner(v[3]);
        f.render_widget(input_block, v[3]);
        f.render_widget(
            Paragraph::new(date_input).style(Style::default().fg(Color::White)),
            input_inner,
        );
        f.set_cursor_position((input_inner.x + date_input.width() as u16, input_inner.y));

        f.render_widget(
            Paragraph::new("Enter:確定  Esc:戻る").style(Style::default().fg(Color::DarkGray)),
            v[4],
        );
    }
}

fn render_tl_edit_overlay<'a>(original: &'a str, title: &'a str) -> impl Fn(&mut Frame, Rect) + 'a {
    move |f: &mut Frame, area: Rect| {
        let block = Block::default()
            .title(" タスクを編集 ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::White));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let v = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Length(3), Constraint::Length(1), Constraint::Min(0)])
            .split(inner);

        f.render_widget(
            Paragraph::new(format!("変更前: {}", original)).style(Style::default().fg(Color::DarkGray)),
            v[0],
        );
        f.render_widget(Paragraph::new("新しいタイトル:"), v[1]);

        let input_block = Block::default().borders(Borders::ALL);
        let input_inner = input_block.inner(v[2]);
        f.render_widget(input_block, v[2]);
        f.render_widget(
            Paragraph::new(title).style(Style::default().fg(Color::White)),
            input_inner,
        );
        f.set_cursor_position((input_inner.x + title.width() as u16, input_inner.y));

        f.render_widget(
            Paragraph::new("Enter:次へ  Esc:キャンセル").style(Style::default().fg(Color::DarkGray)),
            v[3],
        );
    }
}

fn render_task_list_delete_confirm(f: &mut Frame, area: Rect, app: &App, task_index: usize) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(40)])
        .split(area);

    render_task_list_main(f, chunks[0], app, task_index);

    let block = Block::default()
        .title(" タスクを削除 ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red));
    let inner = block.inner(chunks[1]);
    f.render_widget(block, chunks[1]);

    if let Some(task) = app.all_tasks.get(task_index) {
        let list_name = app.task_lists.iter().find(|l| l.id == task.list_id)
            .map(|l| l.title.as_str()).unwrap_or("");
        f.render_widget(
            Paragraph::new(vec![
                Line::from("以下のタスクを削除しますか？"),
                Line::from(""),
                Line::from(Span::styled(
                    format!("□ {}", task.title),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(format!("  リスト: {}", list_name), Style::default().fg(Color::Magenta))),
                Line::from(""),
                Line::from(Span::styled("y:削除  n/Esc:キャンセル", Style::default().fg(Color::DarkGray))),
            ]),
            inner,
        );
    }
}
