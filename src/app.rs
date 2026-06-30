use anyhow::Result;
use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone, Utc, Weekday};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use std::collections::HashMap;

use crate::calendar::{CalendarClient, CalendarInfo, Event};
use crate::matrix::{self, Direction, MatrixItem, PlacedTask};
use crate::meta::{self, MetaStore, StackCategory, TaskMeta};
use crate::priority::{self, ScoreInput};
use crate::tasks::{Task, TaskList, TasksClient};
use crate::touch::TouchInput;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, PartialEq)]
pub enum AppMode {
    Normal,
    AddEvent { title: String },
    AddTask { title: String },
    AddTaskSelectList { title: String, selected: usize },
    AddTaskNewList { title: String, new_list: String },
    SelectItemDelete { selected: usize },
    SelectItemEdit { selected: usize },
    SelectTaskToggle { selected: usize },
    DeleteConfirm { event_index: usize },
    DeleteTaskConfirm { task_index: usize },
    EditEvent { event_index: usize, title: String },
    EditEventTime { event_index: usize, title: String, time_input: String },
    EditEventEndDate { event_index: usize, title: String, start_time: Option<String>, end_date_input: String },
    AddEventTime { title: String, time_input: String },
    AddEventEndTime { title: String, start_time: String, end_time_input: String },
    AddEventEndDate { title: String, start_time: Option<String>, end_time: Option<String>, end_date_input: String },
    EditTask { task_index: usize, title: String },
    EditTaskSelectList { task_index: usize, title: String, selected: usize },
    Matrix,
    // editing: Some(task_id) なら既存タスクの編集、None なら新規追加
    MatrixAddTitle { editing: Option<String>, title: String },
    MatrixAddList { editing: Option<String>, title: String, selected: usize },
    MatrixAddDate { editing: Option<String>, title: String, list_id: String, date_input: String },
    MatrixAddImp { editing: Option<String>, title: String, list_id: String, due: Option<NaiveDate>, input: String },
    MatrixAddClau { editing: Option<String>, title: String, list_id: String, due: Option<NaiveDate>, imp: u8, input: String },
    MatrixStackCat { selected: usize },
    MatrixStackNote { category: Option<StackCategory>, note: String },
    MatrixDeleteConfirm,
}

pub struct App {
    pub current_month: NaiveDate,
    pub selected_date: NaiveDate,
    pub events: HashMap<NaiveDate, Vec<Event>>,
    pub tasks: HashMap<NaiveDate, Vec<Task>>,
    pub all_tasks: Vec<Task>,
    pub mode: AppMode,
    pub status_message: Option<String>,
    pub task_lists: Vec<TaskList>,
    pub calendars: Vec<CalendarInfo>,
    /// マトリックス画面で選択中のタスクID(indexでなくIDで持つ)
    pub matrix_selected: Option<String>,
    /// 直近の描画領域。main ループが毎フレーム更新する
    pub viewport: Rect,
    /// タスクの重要度・clau度・スタックをローカル保存するストア(Google Tasks には送らない)
    pub meta_store: MetaStore,
    calendar_client: CalendarClient,
    tasks_client: TasksClient,
}

impl App {
    pub async fn new() -> Result<Self> {
        let token = crate::auth::load_or_authenticate().await?;
        let client = CalendarClient::new(token.access_token.clone());
        let tasks_client = TasksClient::new(token.access_token);

        let today = Local::now().date_naive();
        let current_month =
            NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap();

        let task_lists = tasks_client.list_task_lists().await.unwrap_or_default();
        let calendars = client.list_calendars().await.unwrap_or_default();

        let mut app = App {
            current_month,
            selected_date: today,
            events: HashMap::new(),
            tasks: HashMap::new(),
            all_tasks: Vec::new(),
            mode: AppMode::Normal,
            status_message: None,
            task_lists,
            calendars,
            matrix_selected: None,
            viewport: Rect::default(),
            meta_store: MetaStore::load(),
            calendar_client: client,
            tasks_client,
        };

        app.load_events().await?;
        let _ = app.load_tasks().await;
        Ok(app)
    }

    pub async fn load_events(&mut self) -> Result<()> {
        let (time_min, time_max) = self.month_range();

        let cal_targets: Vec<(String, Option<String>)> = self
            .calendars
            .iter()
            .map(|cal| {
                let name = if cal.is_primary { None } else { Some(cal.summary.clone()) };
                (cal.id.clone(), name)
            })
            .collect();

        self.events.clear();

        for (cal_id, cal_name) in cal_targets {
            if let Ok(events) = self
                .calendar_client
                .list_events(&cal_id, time_min, time_max, false, cal_name)
                .await
            {
                for event in events {
                    expand_event_to_dates(&mut self.events, event);
                }
            }
        }

        if let Ok(events) = self.calendar_client.list_holiday_events(time_min, time_max).await {
            for event in events {
                expand_event_to_dates(&mut self.events, event);
            }
        }

        Ok(())
    }

    pub async fn load_tasks(&mut self) -> Result<()> {
        let (time_min, time_max) = self.month_range();
        self.tasks.clear();
        for list in &self.task_lists {
            if let Ok(task_list) = self
                .tasks_client
                .list_tasks_in(&list.id, time_min, time_max)
                .await
            {
                for task in task_list {
                    if let Some(due) = task.due {
                        self.tasks.entry(due).or_default().push(task);
                    }
                }
            }
        }
        Ok(())
    }

    fn month_range(
        &self,
    ) -> (
        chrono::DateTime<chrono::Utc>,
        chrono::DateTime<chrono::Utc>,
    ) {
        let y = self.current_month.year();
        let m = self.current_month.month();
        let start = Utc.with_ymd_and_hms(y, m, 1, 0, 0, 0).unwrap();
        let next = if m == 12 {
            NaiveDate::from_ymd_opt(y + 1, 1, 1).unwrap()
        } else {
            NaiveDate::from_ymd_opt(y, m + 1, 1).unwrap()
        };
        let end = Utc
            .with_ymd_and_hms(next.year(), next.month(), 1, 0, 0, 0)
            .unwrap();
        (start, end)
    }

    pub fn grid_start(&self) -> NaiveDate {
        let first = self.current_month;
        let days_back = match first.weekday() {
            Weekday::Mon => 0,
            Weekday::Tue => 1,
            Weekday::Wed => 2,
            Weekday::Thu => 3,
            Weekday::Fri => 4,
            Weekday::Sat => 5,
            Weekday::Sun => 6,
        };
        first - Duration::days(days_back)
    }

    pub fn grid_rows(&self) -> usize {
        let start = self.grid_start();
        let last = self.month_last_day();
        let days = (last - start).num_days() + 1;
        ((days + 6) / 7) as usize
    }

    fn month_last_day(&self) -> NaiveDate {
        let y = self.current_month.year();
        let m = self.current_month.month();
        if m == 12 {
            NaiveDate::from_ymd_opt(y + 1, 1, 1).unwrap() - Duration::days(1)
        } else {
            NaiveDate::from_ymd_opt(y, m + 1, 1).unwrap() - Duration::days(1)
        }
    }

    pub fn grid_dates(&self) -> Vec<NaiveDate> {
        let start = self.grid_start();
        let rows = self.grid_rows();
        (0..(rows * 7) as i64)
            .map(|i| start + Duration::days(i))
            .collect()
    }

    pub fn events_for(&self, date: &NaiveDate) -> &[Event] {
        self.events.get(date).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn selected_events(&self) -> &[Event] {
        self.events_for(&self.selected_date)
    }

    pub fn tasks_for(&self, date: &NaiveDate) -> &[Task] {
        self.tasks.get(date).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn selected_tasks(&self) -> &[Task] {
        self.tasks_for(&self.selected_date)
    }

    pub async fn handle_key(&mut self, key: KeyEvent) -> Result<bool> {
        match self.mode.clone() {
            AppMode::Normal => self.handle_normal(key).await,
            AppMode::AddEvent { title } => self.handle_add_event(key, title).await,
            AppMode::AddTask { title } => self.handle_add_task(key, title).await,
            AppMode::AddTaskSelectList { title, selected } => {
                self.handle_select_list(key, title, selected).await
            }
            AppMode::AddTaskNewList { title, new_list } => {
                self.handle_new_list(key, title, new_list).await
            }
            AppMode::SelectItemDelete { selected } => {
                self.handle_select_item_delete(key, selected).await
            }
            AppMode::SelectItemEdit { selected } => {
                self.handle_select_item_edit(key, selected).await
            }
            AppMode::SelectTaskToggle { selected } => {
                self.handle_select_task_toggle(key, selected).await
            }
            AppMode::DeleteConfirm { event_index } => {
                self.handle_delete_confirm(key, event_index).await
            }
            AppMode::DeleteTaskConfirm { task_index } => {
                self.handle_delete_task_confirm(key, task_index).await
            }
            AppMode::EditEvent { event_index, title } => {
                self.handle_edit_event(key, event_index, title).await
            }
            AppMode::EditEventTime { event_index, title, time_input } => {
                self.handle_edit_event_time(key, event_index, title, time_input).await
            }
            AppMode::EditEventEndDate { event_index, title, start_time, end_date_input } => {
                self.handle_edit_event_end_date(key, event_index, title, start_time, end_date_input).await
            }
            AppMode::AddEventTime { title, time_input } => {
                self.handle_add_event_time(key, title, time_input).await
            }
            AppMode::AddEventEndTime { title, start_time, end_time_input } => {
                self.handle_add_event_end_time(key, title, start_time, end_time_input).await
            }
            AppMode::AddEventEndDate { title, start_time, end_time, end_date_input } => {
                self.handle_add_event_end_date(key, title, start_time, end_time, end_date_input).await
            }
            AppMode::EditTask { task_index, title } => {
                self.handle_edit_task(key, task_index, title).await
            }
            AppMode::EditTaskSelectList { task_index, title, selected } => {
                self.handle_edit_task_select_list(key, task_index, title, selected).await
            }
            AppMode::Matrix => self.handle_matrix(key).await,
            AppMode::MatrixAddTitle { editing, title } => {
                self.handle_matrix_add_title(key, editing, title).await
            }
            AppMode::MatrixAddList { editing, title, selected } => {
                self.handle_matrix_add_list(key, editing, title, selected).await
            }
            AppMode::MatrixAddDate { editing, title, list_id, date_input } => {
                self.handle_matrix_add_date(key, editing, title, list_id, date_input).await
            }
            AppMode::MatrixAddImp { editing, title, list_id, due, input } => {
                self.handle_matrix_add_imp(key, editing, title, list_id, due, input).await
            }
            AppMode::MatrixAddClau { editing, title, list_id, due, imp, input } => {
                self.handle_matrix_add_clau(key, editing, title, list_id, due, imp, input).await
            }
            AppMode::MatrixStackCat { selected } => {
                self.handle_matrix_stack_cat(key, selected).await
            }
            AppMode::MatrixStackNote { category, note } => {
                self.handle_matrix_stack_note(key, category, note).await
            }
            AppMode::MatrixDeleteConfirm => self.handle_matrix_delete_confirm(key).await,
        }
    }

    async fn handle_normal(&mut self, key: KeyEvent) -> Result<bool> {
        self.status_message = None;
        match key.code {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Char('h') => self.move_in_grid(-1),
            KeyCode::Char('l') => self.move_in_grid(1),
            KeyCode::Char('k') => self.move_in_grid(-7),
            KeyCode::Char('j') => self.move_in_grid(7),
            KeyCode::Char('[') => self.change_month(-1).await?,
            KeyCode::Char(']') => self.change_month(1).await?,
            KeyCode::Char('a') => {
                self.mode = AppMode::AddEvent { title: String::new() }
            }
            KeyCode::Char('n') => {
                self.mode = AppMode::AddTask { title: String::new() }
            }
            KeyCode::Char('t') => {
                if self.selected_tasks().is_empty() {
                    self.status_message = Some("この日にタスクはありません".to_string());
                } else {
                    self.mode = AppMode::SelectTaskToggle { selected: 0 };
                }
            }
            KeyCode::Char('d') => {
                let n_events = self.selected_events().iter().filter(|e| !e.is_holiday).count();
                let n_tasks = self.selected_tasks().len();
                if n_events + n_tasks == 0 {
                    self.status_message = Some("この日に削除できる項目はありません".to_string());
                } else {
                    self.mode = AppMode::SelectItemDelete { selected: 0 };
                }
            }
            KeyCode::Char('e') => {
                let n_events = self.selected_events().iter().filter(|e| !e.is_holiday).count();
                let n_tasks = self.selected_tasks().len();
                if n_events + n_tasks == 0 {
                    self.status_message = Some("この日に編集できる項目はありません".to_string());
                } else {
                    self.mode = AppMode::SelectItemEdit { selected: 0 };
                }
            }
            KeyCode::Char('T') => {
                let _ = self.load_all_tasks().await;
                self.matrix_selected = None;
                self.ensure_matrix_selection();
                self.mode = AppMode::Matrix;
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_add_event(&mut self, key: KeyEvent, mut title: String) -> Result<bool> {
        match key.code {
            KeyCode::Esc => self.mode = AppMode::Normal,
            KeyCode::Enter => {
                if !title.trim().is_empty() {
                    self.mode = AppMode::AddEventTime { title, time_input: String::new() };
                } else {
                    self.mode = AppMode::Normal;
                }
            }
            KeyCode::Char(c) => {
                title.push(c);
                self.mode = AppMode::AddEvent { title };
            }
            KeyCode::Backspace => {
                title.pop();
                self.mode = AppMode::AddEvent { title };
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_add_event_time(
        &mut self,
        key: KeyEvent,
        title: String,
        mut time_input: String,
    ) -> Result<bool> {
        match key.code {
            KeyCode::Esc => self.mode = AppMode::AddEvent { title },
            KeyCode::Enter => {
                let trimmed = time_input.trim();
                if trimmed.is_empty() {
                    // 終日予定は終了時刻をスキップ
                    self.mode = AppMode::AddEventEndDate {
                        title,
                        start_time: None,
                        end_time: None,
                        end_date_input: String::new(),
                    };
                } else {
                    self.mode = AppMode::AddEventEndTime {
                        title,
                        start_time: trimmed.to_string(),
                        end_time_input: String::new(),
                    };
                }
            }
            KeyCode::Char(c) => {
                time_input.push(c);
                self.mode = AppMode::AddEventTime { title, time_input };
            }
            KeyCode::Backspace => {
                time_input.pop();
                self.mode = AppMode::AddEventTime { title, time_input };
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_add_event_end_time(
        &mut self,
        key: KeyEvent,
        title: String,
        start_time: String,
        mut end_time_input: String,
    ) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::AddEventTime { title, time_input: start_time };
            }
            KeyCode::Enter => {
                let end_time = if end_time_input.trim().is_empty() {
                    None
                } else {
                    Some(end_time_input.trim().to_string())
                };
                self.mode = AppMode::AddEventEndDate {
                    title,
                    start_time: Some(start_time),
                    end_time,
                    end_date_input: String::new(),
                };
            }
            KeyCode::Char(c) => {
                end_time_input.push(c);
                self.mode = AppMode::AddEventEndTime { title, start_time, end_time_input };
            }
            KeyCode::Backspace => {
                end_time_input.pop();
                self.mode = AppMode::AddEventEndTime { title, start_time, end_time_input };
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_add_event_end_date(
        &mut self,
        key: KeyEvent,
        title: String,
        start_time: Option<String>,
        end_time: Option<String>,
        mut end_date_input: String,
    ) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                match start_time {
                    Some(start_time) => {
                        self.mode = AppMode::AddEventEndTime {
                            title,
                            start_time,
                            end_time_input: end_time.unwrap_or_default(),
                        };
                    }
                    None => {
                        self.mode = AppMode::AddEventTime { title, time_input: String::new() };
                    }
                }
            }
            KeyCode::Enter => {
                let end_date = parse_date_input(&end_date_input);
                match self
                    .calendar_client
                    .create_event(
                        &title,
                        self.selected_date,
                        start_time.as_deref(),
                        end_time.as_deref(),
                        end_date,
                    )
                    .await
                {
                    Ok(_) => {
                        let _ = self.load_events().await;
                        self.status_message = Some("予定を追加しました".to_string());
                    }
                    Err(e) => {
                        self.status_message = Some(format!("エラー: {}", e));
                    }
                }
                self.mode = AppMode::Normal;
            }
            KeyCode::Char(c) => {
                end_date_input.push(c);
                self.mode = AppMode::AddEventEndDate { title, start_time, end_time, end_date_input };
            }
            KeyCode::Backspace => {
                end_date_input.pop();
                self.mode = AppMode::AddEventEndDate { title, start_time, end_time, end_date_input };
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_add_task(&mut self, key: KeyEvent, mut title: String) -> Result<bool> {
        match key.code {
            KeyCode::Esc => self.mode = AppMode::Normal,
            KeyCode::Enter => {
                if !title.trim().is_empty() {
                    if self.task_lists.is_empty() {
                        // リストがない場合は新規リスト作成へ
                        self.mode = AppMode::AddTaskNewList {
                            title,
                            new_list: String::new(),
                        };
                    } else {
                        self.mode = AppMode::AddTaskSelectList { title, selected: 0 };
                    }
                } else {
                    self.mode = AppMode::Normal;
                }
            }
            KeyCode::Char(c) => {
                title.push(c);
                self.mode = AppMode::AddTask { title };
            }
            KeyCode::Backspace => {
                title.pop();
                self.mode = AppMode::AddTask { title };
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_select_list(
        &mut self,
        key: KeyEvent,
        title: String,
        selected: usize,
    ) -> Result<bool> {
        // 選択肢: [list0, list1, ..., + 新規リスト作成]
        let total = self.task_lists.len() + 1;

        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::AddTask { title };
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let new_sel = (selected + 1).min(total - 1);
                self.mode = AppMode::AddTaskSelectList { title, selected: new_sel };
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let new_sel = selected.saturating_sub(1);
                self.mode = AppMode::AddTaskSelectList { title, selected: new_sel };
            }
            KeyCode::Enter => {
                let new_idx = self.task_lists.len();
                if selected == new_idx {
                    self.mode = AppMode::AddTaskNewList {
                        title,
                        new_list: String::new(),
                    };
                } else {
                    let list_id = self.task_lists[selected].id.clone();
                    self.finish_create_task(title, list_id).await?;
                }
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_new_list(
        &mut self,
        key: KeyEvent,
        title: String,
        mut new_list: String,
    ) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                if self.task_lists.is_empty() {
                    self.mode = AppMode::AddTask { title };
                } else {
                    let selected = self.task_lists.len();
                    self.mode = AppMode::AddTaskSelectList { title, selected };
                }
            }
            KeyCode::Enter => {
                let list_title = new_list.trim().to_string();
                if list_title.is_empty() {
                    self.mode = AppMode::Normal;
                    return Ok(false);
                }
                match self.tasks_client.create_task_list(&list_title).await {
                    Ok(new_task_list) => {
                        let list_id = new_task_list.id.clone();
                        self.task_lists.push(new_task_list);
                        self.finish_create_task(title, list_id).await?;
                    }
                    Err(e) => {
                        self.status_message = Some(format!("リスト作成エラー: {}", e));
                        self.mode = AppMode::Normal;
                    }
                }
            }
            KeyCode::Char(c) => {
                new_list.push(c);
                self.mode = AppMode::AddTaskNewList { title, new_list };
            }
            KeyCode::Backspace => {
                new_list.pop();
                self.mode = AppMode::AddTaskNewList { title, new_list };
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_select_item_delete(&mut self, key: KeyEvent, selected: usize) -> Result<bool> {
        let non_holiday_indices: Vec<usize> = self.selected_events().iter()
            .enumerate()
            .filter(|(_, e)| !e.is_holiday)
            .map(|(i, _)| i)
            .collect();
        let n_events = non_holiday_indices.len();
        let n_tasks = self.selected_tasks().len();
        let total = n_events + n_tasks;

        match key.code {
            KeyCode::Esc => self.mode = AppMode::Normal,
            KeyCode::Char('j') | KeyCode::Down => {
                let new_sel = (selected + 1).min(total.saturating_sub(1));
                self.mode = AppMode::SelectItemDelete { selected: new_sel };
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let new_sel = selected.saturating_sub(1);
                self.mode = AppMode::SelectItemDelete { selected: new_sel };
            }
            KeyCode::Enter => {
                if selected < n_events {
                    self.mode = AppMode::DeleteConfirm { event_index: non_holiday_indices[selected] };
                } else {
                    self.mode = AppMode::DeleteTaskConfirm { task_index: selected - n_events };
                }
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_select_item_edit(&mut self, key: KeyEvent, selected: usize) -> Result<bool> {
        let non_holiday_indices: Vec<usize> = self.selected_events().iter()
            .enumerate()
            .filter(|(_, e)| !e.is_holiday)
            .map(|(i, _)| i)
            .collect();
        let n_events = non_holiday_indices.len();
        let n_tasks = self.selected_tasks().len();
        let total = n_events + n_tasks;

        match key.code {
            KeyCode::Esc => self.mode = AppMode::Normal,
            KeyCode::Char('j') | KeyCode::Down => {
                let new_sel = (selected + 1).min(total.saturating_sub(1));
                self.mode = AppMode::SelectItemEdit { selected: new_sel };
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let new_sel = selected.saturating_sub(1);
                self.mode = AppMode::SelectItemEdit { selected: new_sel };
            }
            KeyCode::Enter => {
                if selected < n_events {
                    let event_idx = non_holiday_indices[selected];
                    let title = self.selected_events()[event_idx].title.clone();
                    self.mode = AppMode::EditEvent { event_index: event_idx, title };
                } else {
                    let task_idx = selected - n_events;
                    let title = self.selected_tasks()[task_idx].title.clone();
                    self.mode = AppMode::EditTask { task_index: task_idx, title };
                }
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_select_task_toggle(&mut self, key: KeyEvent, selected: usize) -> Result<bool> {
        let n_tasks = self.selected_tasks().len();

        match key.code {
            KeyCode::Esc => self.mode = AppMode::Normal,
            KeyCode::Char('j') | KeyCode::Down => {
                let new_sel = (selected + 1).min(n_tasks.saturating_sub(1));
                self.mode = AppMode::SelectTaskToggle { selected: new_sel };
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let new_sel = selected.saturating_sub(1);
                self.mode = AppMode::SelectTaskToggle { selected: new_sel };
            }
            KeyCode::Enter => {
                let tasks = self.tasks.get(&self.selected_date).cloned().unwrap_or_default();
                if let Some(task) = tasks.get(selected) {
                    let task = task.clone();
                    match self.tasks_client.toggle_task(&task).await {
                        Ok(updated) => {
                            if let Some(day_tasks) = self.tasks.get_mut(&self.selected_date) {
                                if let Some(t) = day_tasks.get_mut(selected) {
                                    *t = updated;
                                }
                            }
                            self.status_message = Some("タスクの完了状態を切り替えました".to_string());
                        }
                        Err(e) => {
                            self.status_message = Some(format!("エラー: {}", e));
                        }
                    }
                }
                self.mode = AppMode::Normal;
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_edit_event(
        &mut self,
        key: KeyEvent,
        event_index: usize,
        mut title: String,
    ) -> Result<bool> {
        match key.code {
            KeyCode::Esc => self.mode = AppMode::Normal,
            KeyCode::Enter => {
                let trimmed = title.trim().to_string();
                if !trimmed.is_empty() {
                    let current_start_time = self
                        .events
                        .get(&self.selected_date)
                        .and_then(|evs| evs.get(event_index))
                        .and_then(|e| e.start_time.clone())
                        .unwrap_or_default();
                    self.mode = AppMode::EditEventTime {
                        event_index,
                        title: trimmed,
                        time_input: current_start_time,
                    };
                } else {
                    self.mode = AppMode::Normal;
                }
            }
            KeyCode::Char(c) => {
                title.push(c);
                self.mode = AppMode::EditEvent { event_index, title };
            }
            KeyCode::Backspace => {
                title.pop();
                self.mode = AppMode::EditEvent { event_index, title };
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_edit_event_time(
        &mut self,
        key: KeyEvent,
        event_index: usize,
        title: String,
        mut time_input: String,
    ) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::EditEvent { event_index, title };
            }
            KeyCode::Enter => {
                let start_time = if time_input.trim().is_empty() {
                    None
                } else {
                    Some(time_input.trim().to_string())
                };
                let current_end_date = self
                    .events
                    .get(&self.selected_date)
                    .and_then(|evs| evs.get(event_index))
                    .and_then(|e| e.end_date)
                    .map(|d| d.format("%Y-%m-%d").to_string())
                    .unwrap_or_default();
                self.mode = AppMode::EditEventEndDate {
                    event_index,
                    title,
                    start_time,
                    end_date_input: current_end_date,
                };
            }
            KeyCode::Char(c) => {
                time_input.push(c);
                self.mode = AppMode::EditEventTime { event_index, title, time_input };
            }
            KeyCode::Backspace => {
                time_input.pop();
                self.mode = AppMode::EditEventTime { event_index, title, time_input };
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_edit_event_end_date(
        &mut self,
        key: KeyEvent,
        event_index: usize,
        title: String,
        start_time: Option<String>,
        mut end_date_input: String,
    ) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::EditEventTime {
                    event_index,
                    title,
                    time_input: start_time.unwrap_or_default(),
                };
            }
            KeyCode::Enter => {
                let trimmed_title = title.trim().to_string();
                let events = self.events.get(&self.selected_date).cloned().unwrap_or_default();
                if let Some(event) = events.get(event_index) {
                    let event_id = event.id.clone();
                    let event_date = event.date;
                    let end_date = parse_date_input(&end_date_input);
                    match self
                        .calendar_client
                        .update_event(
                            &event_id,
                            &trimmed_title,
                            event_date,
                            start_time.as_deref(),
                            end_date,
                        )
                        .await
                    {
                        Ok(()) => {
                            let _ = self.load_events().await;
                            self.status_message = Some("予定を更新しました".to_string());
                        }
                        Err(e) => {
                            self.status_message = Some(format!("エラー: {}", e));
                        }
                    }
                }
                self.mode = AppMode::Normal;
            }
            KeyCode::Char(c) => {
                end_date_input.push(c);
                self.mode = AppMode::EditEventEndDate { event_index, title, start_time, end_date_input };
            }
            KeyCode::Backspace => {
                end_date_input.pop();
                self.mode = AppMode::EditEventEndDate { event_index, title, start_time, end_date_input };
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_edit_task(
        &mut self,
        key: KeyEvent,
        task_index: usize,
        mut title: String,
    ) -> Result<bool> {
        match key.code {
            KeyCode::Esc => self.mode = AppMode::Normal,
            KeyCode::Enter => {
                if !title.trim().is_empty() {
                    let current_list_idx = self
                        .selected_tasks()
                        .get(task_index)
                        .and_then(|t| self.task_lists.iter().position(|l| l.id == t.list_id))
                        .unwrap_or(0);
                    self.mode = AppMode::EditTaskSelectList {
                        task_index,
                        title,
                        selected: current_list_idx,
                    };
                } else {
                    self.mode = AppMode::Normal;
                }
            }
            KeyCode::Char(c) => {
                title.push(c);
                self.mode = AppMode::EditTask { task_index, title };
            }
            KeyCode::Backspace => {
                title.pop();
                self.mode = AppMode::EditTask { task_index, title };
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_edit_task_select_list(
        &mut self,
        key: KeyEvent,
        task_index: usize,
        title: String,
        selected: usize,
    ) -> Result<bool> {
        let total = self.task_lists.len();
        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::EditTask { task_index, title };
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let new_sel = (selected + 1).min(total.saturating_sub(1));
                self.mode = AppMode::EditTaskSelectList { task_index, title, selected: new_sel };
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let new_sel = selected.saturating_sub(1);
                self.mode = AppMode::EditTaskSelectList { task_index, title, selected: new_sel };
            }
            KeyCode::Enter => {
                let tasks = self.tasks.get(&self.selected_date).cloned().unwrap_or_default();
                if let Some(task) = tasks.get(task_index) {
                    let task = task.clone();
                    let new_list_id = self.task_lists[selected].id.clone();
                    let new_title = title.trim().to_string();

                    if task.list_id == new_list_id {
                        match self.tasks_client.update_task_title(&task.list_id, &task.id, &new_title).await {
                            Ok(()) => {
                                if let Some(day_tasks) = self.tasks.get_mut(&self.selected_date) {
                                    if let Some(t) = day_tasks.get_mut(task_index) {
                                        t.title = new_title;
                                    }
                                }
                                self.status_message = Some("タスクを更新しました".to_string());
                            }
                            Err(e) => {
                                self.status_message = Some(format!("エラー: {}", e));
                            }
                        }
                    } else {
                        match self.tasks_client.move_task_to_list(&task, &new_list_id, &new_title).await {
                            Ok(new_task) => {
                                if let Some(day_tasks) = self.tasks.get_mut(&self.selected_date) {
                                    day_tasks.remove(task_index);
                                    day_tasks.push(new_task);
                                }
                                self.status_message = Some("タスクを更新しました".to_string());
                            }
                            Err(e) => {
                                self.status_message = Some(format!("エラー: {}", e));
                            }
                        }
                    }
                }
                self.mode = AppMode::Normal;
            }
            _ => {}
        }
        Ok(false)
    }

    async fn finish_create_task(&mut self, title: String, list_id: String) -> Result<()> {
        match self
            .tasks_client
            .create_task(&list_id, &title, Some(self.selected_date), None)
            .await
        {
            Ok(task) => {
                self.tasks.entry(self.selected_date).or_default().push(task);
                self.status_message = Some("タスクを追加しました".to_string());
            }
            Err(e) => {
                self.status_message = Some(format!("エラー: {}", e));
            }
        }
        self.mode = AppMode::Normal;
        Ok(())
    }

    async fn handle_delete_confirm(&mut self, key: KeyEvent, idx: usize) -> Result<bool> {
        match key.code {
            KeyCode::Char('y') => {
                let event_id = self
                    .events
                    .get(&self.selected_date)
                    .and_then(|evs| evs.get(idx))
                    .map(|e| e.id.clone());
                if let Some(event_id) = event_id {
                    match self.calendar_client.delete_event(&event_id).await {
                        Ok(()) => {
                            for day_events in self.events.values_mut() {
                                day_events.retain(|e| e.id != event_id);
                            }
                            self.status_message = Some("予定を削除しました".to_string());
                        }
                        Err(e) => {
                            self.status_message = Some(format!("エラー: {}", e));
                        }
                    }
                }
                self.mode = AppMode::Normal;
            }
            KeyCode::Char('n') | KeyCode::Esc => self.mode = AppMode::Normal,
            _ => {}
        }
        Ok(false)
    }

    async fn handle_delete_task_confirm(&mut self, key: KeyEvent, idx: usize) -> Result<bool> {
        match key.code {
            KeyCode::Char('y') => {
                let tasks = self.tasks.get(&self.selected_date).cloned().unwrap_or_default();
                if idx < tasks.len() {
                    let task = &tasks[idx];
                    let list_id = task.list_id.clone();
                    let task_id = task.id.clone();
                    match self.tasks_client.delete_task(&list_id, &task_id).await {
                        Ok(()) => {
                            if let Some(day_tasks) = self.tasks.get_mut(&self.selected_date) {
                                day_tasks.remove(idx);
                            }
                            self.status_message = Some("タスクを削除しました".to_string());
                        }
                        Err(e) => {
                            self.status_message = Some(format!("エラー: {}", e));
                        }
                    }
                }
                self.mode = AppMode::Normal;
            }
            KeyCode::Char('n') | KeyCode::Esc => self.mode = AppMode::Normal,
            _ => {}
        }
        Ok(false)
    }

    async fn ensure_fresh_auth(&mut self) -> Result<()> {
        let token = crate::auth::load_or_authenticate().await?;
        self.calendar_client = CalendarClient::new(token.access_token.clone());
        self.tasks_client = TasksClient::new(token.access_token);
        Ok(())
    }

    pub async fn load_all_tasks(&mut self) -> Result<()> {
        let _ = self.ensure_fresh_auth().await;
        self.all_tasks.clear();
        for list in &self.task_lists {
            if let Ok(tasks) = self.tasks_client.list_all_tasks(&list.id).await {
                self.all_tasks.extend(tasks);
            }
        }
        self.migrate_embedded_meta();
        Ok(())
    }

    /// 旧フォーマット(Google Tasks の notes に埋め込まれた `[todo-meta]`)を
    /// ローカルストアへ取り込む。ローカルに既にエントリがあるタスクはローカルを優先する。
    fn migrate_embedded_meta(&mut self) {
        let mut imported = false;
        for task in &self.all_tasks {
            if self.meta_store.contains(&task.id) {
                continue;
            }
            if let Some(m) = meta::meta_of(task.notes.as_deref()) {
                self.meta_store.set(task.id.clone(), m);
                imported = true;
            }
        }
        if imported {
            if let Err(e) = self.meta_store.save() {
                self.status_message = Some(format!("メタ保存エラー: {}", e));
            }
        }
    }

    // ─── マトリックス画面 ───────────────────────────────────────────────

    /// 未完了タスクをメタ付きでレイアウト入力に変換する。
    /// 優先順位は計算式で都度算出する(保存はしない)。
    pub fn matrix_items(&self) -> Vec<MatrixItem> {
        let today = Local::now().date_naive();
        let entries: Vec<(&Task, TaskMeta)> = self
            .all_tasks
            .iter()
            .filter(|t| !t.completed)
            .map(|t| (t, self.meta_store.get(&t.id)))
            .collect();

        // スタック済みタスクは優先順位の計算対象から外す。
        // 非スタックタスクだけを取り出して rank し、結果を元の並びへ戻す。
        let active_idx: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, (_, m))| m.stack.is_none())
            .map(|(i, _)| i)
            .collect();
        let scores: Vec<i64> = active_idx
            .iter()
            .map(|&i| {
                let (t, m) = &entries[i];
                priority::score(
                    &ScoreInput { imp: m.imp, clau: m.clau, due: t.due },
                    today,
                )
            })
            .collect();
        let active_ranks = priority::rank(&scores);
        // entry index -> 優先順位(スタック済みは None)
        let mut pri_of: Vec<Option<u32>> = vec![None; entries.len()];
        for (k, &i) in active_idx.iter().enumerate() {
            pri_of[i] = Some(active_ranks[k]);
        }

        entries
            .iter()
            .enumerate()
            .map(|(i, (t, m))| MatrixItem {
                task_id: t.id.clone(),
                title: t.title.clone(),
                imp: m.imp,
                clau: m.clau,
                pri: pri_of[i],
                stack: m.stack,
            })
            .collect()
    }

    /// 現在の画面サイズでの配置結果(ui側の描画と同一)
    pub fn matrix_placed(&self) -> Vec<PlacedTask> {
        matrix::compute_layout(&self.matrix_items(), matrix::graph_inner(self.viewport))
    }

    /// touch-server から届いたタッチ入力を処理する。基本画面(カレンダー/マトリックス)
    /// 以外の入力・編集中モードでは無視する。戻り値 true で終了(現状タッチでは無し)。
    pub async fn handle_touch(&mut self, input: TouchInput) -> Result<bool> {
        match self.mode {
            AppMode::Normal => self.handle_touch_calendar(input),
            AppMode::Matrix => self.handle_touch_matrix(input),
            _ => {}
        }
        Ok(false)
    }

    /// カレンダー画面: タップ/ドラッグした位置の日付セルを選択する。
    fn handle_touch_calendar(&mut self, input: TouchInput) {
        let (col, row) = match input {
            TouchInput::Tap { col, row } => (col, row),
            TouchInput::LongDrag { to, .. } => to,
        };
        if let Some(date) = self.date_at(col, row) {
            self.selected_date = date;
        }
    }

    /// マトリックス画面: タップで選択、選択済みの再タップで編集、長押しドラッグで再配置。
    fn handle_touch_matrix(&mut self, input: TouchInput) {
        match input {
            TouchInput::Tap { col, row } => {
                let Some(id) = self.matrix_task_at(col, row) else {
                    return;
                };
                if self.matrix_selected.as_deref() == Some(id.as_str()) {
                    // 既に選択中のタスクを再タップ → 編集モードへ(`e` キーと同じ遷移)。
                    if let Some(task) = self.all_tasks.iter().find(|t| t.id == id) {
                        self.mode = AppMode::MatrixAddTitle {
                            editing: Some(task.id.clone()),
                            title: task.title.clone(),
                        };
                    }
                } else {
                    self.matrix_selected = Some(id);
                }
            }
            TouchInput::LongDrag { from, to } => {
                let Some(id) = self.matrix_task_at(from.0, from.1) else {
                    return;
                };
                let area = matrix::graph_inner(self.viewport);
                let (imp, clau) = matrix::coords_to_imp_clau(area, to.0, to.1);
                self.set_task_imp_clau(&id, imp, clau);
                self.matrix_selected = Some(id);
                self.status_message = Some(format!("重要度{} / clau度{} に移動", imp, clau));
            }
        }
    }

    /// セル座標 (col,row) を含む日付セルを返す(描画と同一レイアウトでヒットテスト)。
    fn date_at(&self, col: u16, row: u16) -> Option<NaiveDate> {
        crate::ui::calendar_cell_rects(self.viewport, self)
            .into_iter()
            .find(|(_, r)| {
                col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height
            })
            .map(|(d, _)| d)
    }

    /// セル座標 (col,row) に重なるマトリックスのタスク ID を返す。
    /// ターミナルのセルは縦長でタッチの縦精度が粗く、最下行(重要度0)は枠線が
    /// すぐ下にあって取りこぼしやすい。縦は ±1 行の許容を持たせ、列がラベル幅に
    /// 重なるタスクのうち最も近い行のものを選ぶ。
    fn matrix_task_at(&self, col: u16, row: u16) -> Option<String> {
        const ROW_TOL: i32 = 1;
        self.matrix_placed()
            .into_iter()
            .filter_map(|p| {
                let w = UnicodeWidthStr::width(p.label.as_str()) as u16;
                if col < p.x || col >= p.x + w {
                    return None;
                }
                let dy = (p.y as i32 - row as i32).abs();
                (dy <= ROW_TOL).then_some((dy, p.task_id))
            })
            .min_by_key(|(dy, _)| *dy)
            .map(|(_, id)| id)
    }

    /// タスクの imp/clau をローカルメタへ保存する(Google Tasks には送らない)。
    fn set_task_imp_clau(&mut self, task_id: &str, imp: u8, clau: u8) {
        let mut m = self.meta_store.get(task_id);
        m.imp = imp;
        m.clau = clau;
        self.meta_store.set(task_id.to_string(), m);
        if let Err(e) = self.meta_store.save() {
            self.status_message = Some(format!("メタ保存エラー: {}", e));
        }
    }

    pub fn selected_matrix_task(&self) -> Option<&Task> {
        let id = self.matrix_selected.as_deref()?;
        self.all_tasks.iter().find(|t| t.id == id)
    }

    /// 選択が無効(未選択・完了済み・削除済み)なら優先順位最小のタスクを選ぶ
    fn ensure_matrix_selection(&mut self) {
        let items = self.matrix_items();
        let valid = self
            .matrix_selected
            .as_ref()
            .map_or(false, |id| items.iter().any(|i| &i.task_id == id));
        if !valid {
            self.matrix_selected = items
                .iter()
                .min_by_key(|i| i.pri.unwrap_or(u32::MAX))
                .map(|i| i.task_id.clone());
        }
    }

    async fn handle_matrix(&mut self, key: KeyEvent) -> Result<bool> {
        self.status_message = None;
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc | KeyCode::Char('T') => {
                let _ = self.load_tasks().await;
                self.mode = AppMode::Normal;
            }
            KeyCode::Char(c @ ('h' | 'j' | 'k' | 'l')) => {
                self.ensure_matrix_selection();
                if let Some(cur) = self.matrix_selected.clone() {
                    let dir = match c {
                        'h' => Direction::Left,
                        'l' => Direction::Right,
                        'j' => Direction::Down,
                        _ => Direction::Up,
                    };
                    let placed = self.matrix_placed();
                    if let Some(id) = matrix::navigate(&placed, &cur, dir) {
                        self.matrix_selected = Some(id.to_string());
                    }
                }
            }
            KeyCode::Char(c @ '0'..='9') => {
                let pri = if c == '0' { 10 } else { c as u32 - '0' as u32 };
                let items = self.matrix_items();
                if let Some(item) = items.iter().find(|i| i.pri == Some(pri)) {
                    self.matrix_selected = Some(item.task_id.clone());
                } else {
                    self.status_message = Some(format!("優先順位{}のタスクはありません", pri));
                }
            }
            KeyCode::Char('n') => {
                self.mode = AppMode::MatrixAddTitle {
                    editing: None,
                    title: String::new(),
                };
            }
            KeyCode::Char('e') => {
                self.ensure_matrix_selection();
                if let Some(task) = self.selected_matrix_task() {
                    self.mode = AppMode::MatrixAddTitle {
                        editing: Some(task.id.clone()),
                        title: task.title.clone(),
                    };
                }
            }
            KeyCode::Char('t') => {
                self.ensure_matrix_selection();
                if let Some(id) = self.matrix_selected.clone() {
                    if let Some(idx) = self.all_tasks.iter().position(|t| t.id == id) {
                        let task = self.all_tasks[idx].clone();
                        match self.tasks_client.toggle_task(&task).await {
                            Ok(updated) => {
                                self.all_tasks[idx] = updated;
                                self.matrix_selected = None;
                                self.ensure_matrix_selection();
                                self.status_message = Some("タスクを完了にしました".to_string());
                            }
                            Err(e) => {
                                self.status_message = Some(format!("エラー: {}", e));
                            }
                        }
                    }
                }
            }
            KeyCode::Char('d') => {
                self.ensure_matrix_selection();
                if self.matrix_selected.is_some() {
                    self.mode = AppMode::MatrixDeleteConfirm;
                }
            }
            KeyCode::Char('s') => {
                self.ensure_matrix_selection();
                if let Some(task) = self.selected_matrix_task() {
                    let current = self
                        .meta_store
                        .get(&task.id)
                        .stack
                        .and_then(|c| StackCategory::ALL.iter().position(|x| *x == c))
                        .unwrap_or(0);
                    self.mode = AppMode::MatrixStackCat { selected: current };
                }
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_matrix_add_title(
        &mut self,
        key: KeyEvent,
        editing: Option<String>,
        mut title: String,
    ) -> Result<bool> {
        match key.code {
            KeyCode::Esc => self.mode = AppMode::Matrix,
            KeyCode::Enter => {
                if title.trim().is_empty() {
                    self.mode = AppMode::Matrix;
                } else if self.task_lists.is_empty() {
                    self.status_message = Some("タスクリストがありません".to_string());
                    self.mode = AppMode::Matrix;
                } else {
                    // 編集時は現在のリストを初期選択にする
                    let selected = editing
                        .as_ref()
                        .and_then(|id| self.all_tasks.iter().find(|t| &t.id == id))
                        .and_then(|t| self.task_lists.iter().position(|l| l.id == t.list_id))
                        .unwrap_or(0);
                    self.mode = AppMode::MatrixAddList { editing, title, selected };
                }
            }
            KeyCode::Char(c) => {
                title.push(c);
                self.mode = AppMode::MatrixAddTitle { editing, title };
            }
            KeyCode::Backspace => {
                title.pop();
                self.mode = AppMode::MatrixAddTitle { editing, title };
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_matrix_add_list(
        &mut self,
        key: KeyEvent,
        editing: Option<String>,
        title: String,
        selected: usize,
    ) -> Result<bool> {
        let total = self.task_lists.len();
        match key.code {
            KeyCode::Esc => self.mode = AppMode::MatrixAddTitle { editing, title },
            KeyCode::Char('j') | KeyCode::Down => {
                self.mode = AppMode::MatrixAddList {
                    editing,
                    title,
                    selected: (selected + 1).min(total.saturating_sub(1)),
                };
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.mode = AppMode::MatrixAddList {
                    editing,
                    title,
                    selected: selected.saturating_sub(1),
                };
            }
            KeyCode::Enter => {
                let list_id = self.task_lists[selected].id.clone();
                // 編集時は現在の日付を初期値にする
                let date_input = editing
                    .as_ref()
                    .and_then(|id| self.all_tasks.iter().find(|t| &t.id == id))
                    .and_then(|t| t.due)
                    .map(|d| d.format("%Y-%m-%d").to_string())
                    .unwrap_or_default();
                self.mode = AppMode::MatrixAddDate {
                    editing,
                    title,
                    list_id,
                    date_input,
                };
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_matrix_add_date(
        &mut self,
        key: KeyEvent,
        editing: Option<String>,
        title: String,
        list_id: String,
        mut date_input: String,
    ) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                let selected = self
                    .task_lists
                    .iter()
                    .position(|l| l.id == list_id)
                    .unwrap_or(0);
                self.mode = AppMode::MatrixAddList { editing, title, selected };
            }
            KeyCode::Enter => {
                let trimmed = date_input.trim().to_string();
                let due = parse_date_input(&trimmed);
                if !trimmed.is_empty() && due.is_none() {
                    self.status_message =
                        Some("日付はYYYY-MM-DDまたはMM-DDで入力してください".to_string());
                    self.mode = AppMode::MatrixAddDate { editing, title, list_id, date_input };
                } else {
                    // 編集時は現在の重要度を初期値にする
                    let input = match &editing {
                        Some(id) if self.all_tasks.iter().any(|t| &t.id == id) => {
                            self.meta_store.get(id).imp.to_string()
                        }
                        _ => String::new(),
                    };
                    self.mode = AppMode::MatrixAddImp {
                        editing,
                        title,
                        list_id,
                        due,
                        input,
                    };
                }
            }
            KeyCode::Char(c) => {
                date_input.push(c);
                self.mode = AppMode::MatrixAddDate { editing, title, list_id, date_input };
            }
            KeyCode::Backspace => {
                date_input.pop();
                self.mode = AppMode::MatrixAddDate { editing, title, list_id, date_input };
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_matrix_add_imp(
        &mut self,
        key: KeyEvent,
        editing: Option<String>,
        title: String,
        list_id: String,
        due: Option<NaiveDate>,
        mut input: String,
    ) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                let date_input = due.map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default();
                self.mode = AppMode::MatrixAddDate { editing, title, list_id, date_input };
            }
            KeyCode::Enter => match parse_score(&input) {
                Some(imp) => {
                    // 編集時は現在のclau度を初期値にする
                    let input = match &editing {
                        Some(id) if self.all_tasks.iter().any(|t| &t.id == id) => {
                            self.meta_store.get(id).clau.to_string()
                        }
                        _ => String::new(),
                    };
                    self.mode = AppMode::MatrixAddClau {
                        editing,
                        title,
                        list_id,
                        due,
                        imp,
                        input,
                    };
                }
                None => {
                    self.status_message = Some("重要度は0〜10で入力してください".to_string());
                    self.mode = AppMode::MatrixAddImp {
                        editing,
                        title,
                        list_id,
                        due,
                        input: String::new(),
                    };
                }
            },
            KeyCode::Char(c) if c.is_ascii_digit() && input.len() < 2 => {
                input.push(c);
                self.mode = AppMode::MatrixAddImp { editing, title, list_id, due, input };
            }
            KeyCode::Backspace => {
                input.pop();
                self.mode = AppMode::MatrixAddImp { editing, title, list_id, due, input };
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_matrix_add_clau(
        &mut self,
        key: KeyEvent,
        editing: Option<String>,
        title: String,
        list_id: String,
        due: Option<NaiveDate>,
        imp: u8,
        mut input: String,
    ) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::MatrixAddImp {
                    editing,
                    title,
                    list_id,
                    due,
                    input: imp.to_string(),
                };
            }
            KeyCode::Enter => match parse_score(&input) {
                Some(clau) => {
                    match editing {
                        Some(task_id) => {
                            self.finish_edit_matrix_task(task_id, title, list_id, due, imp, clau)
                                .await;
                        }
                        None => {
                            // 重要度・clau度は Google Tasks には送らず、ローカルに保存する
                            match self
                                .tasks_client
                                .create_task(&list_id, title.trim(), due, None)
                                .await
                            {
                                Ok(task) => {
                                    let meta_obj = TaskMeta { imp, clau, ..Default::default() };
                                    self.meta_store.set(task.id.clone(), meta_obj);
                                    if let Err(e) = self.meta_store.save() {
                                        self.status_message =
                                            Some(format!("メタ保存エラー: {}", e));
                                    }
                                    if let Some(d) = task.due {
                                        self.tasks.entry(d).or_default().push(task.clone());
                                    }
                                    self.matrix_selected = Some(task.id.clone());
                                    self.all_tasks.push(task);
                                    self.status_message =
                                        Some("タスクを追加しました".to_string());
                                }
                                Err(e) => {
                                    self.status_message = Some(format!("エラー: {}", e));
                                }
                            }
                        }
                    }
                    self.mode = AppMode::Matrix;
                }
                None => {
                    self.status_message = Some("clau度は0〜10で入力してください".to_string());
                    self.mode = AppMode::MatrixAddClau {
                        editing,
                        title,
                        list_id,
                        due,
                        imp,
                        input: String::new(),
                    };
                }
            },
            KeyCode::Char(c) if c.is_ascii_digit() && input.len() < 2 => {
                input.push(c);
                self.mode = AppMode::MatrixAddClau { editing, title, list_id, due, imp, input };
            }
            KeyCode::Backspace => {
                input.pop();
                self.mode = AppMode::MatrixAddClau { editing, title, list_id, due, imp, input };
            }
            _ => {}
        }
        Ok(false)
    }

    /// 編集フローの確定。リストが同じなら PATCH、変わっていれば移動(再作成+削除)
    async fn finish_edit_matrix_task(
        &mut self,
        task_id: String,
        title: String,
        list_id: String,
        due: Option<NaiveDate>,
        imp: u8,
        clau: u8,
    ) {
        let Some(idx) = self.all_tasks.iter().position(|t| t.id == task_id) else {
            self.status_message = Some("編集対象のタスクが見つかりません".to_string());
            return;
        };
        let task = self.all_tasks[idx].clone();
        // imp/clau はローカルに保存する。スタック等の既存ローカルメタは保持する。
        let mut m = self.meta_store.get(&task.id);
        m.imp = imp;
        m.clau = clau;
        self.meta_store.set(task.id.clone(), m);
        if let Err(e) = self.meta_store.save() {
            self.status_message = Some(format!("メタ保存エラー: {}", e));
        }
        // Google へ送る notes からは旧メタ行を取り除く(本文のみ)
        let (body, _) = meta::parse_notes(task.notes.as_deref().unwrap_or(""));
        let new_notes = body;
        let new_title = title.trim().to_string();

        if task.list_id == list_id {
            match self
                .tasks_client
                .update_task(&task.list_id, &task.id, &new_title, due, Some(&new_notes))
                .await
            {
                Ok(()) => {
                    let t = &mut self.all_tasks[idx];
                    t.title = new_title;
                    t.due = due;
                    t.notes = Some(new_notes);
                    self.status_message = Some("タスクを更新しました".to_string());
                }
                Err(e) => {
                    self.status_message = Some(format!("エラー: {}", e));
                }
            }
        } else {
            let updated = Task {
                title: new_title.clone(),
                due,
                notes: Some(new_notes),
                ..task
            };
            match self
                .tasks_client
                .move_task_to_list(&updated, &list_id, &new_title)
                .await
            {
                Ok(new_task) => {
                    // リスト移動でIDが変わるため、ローカルメタを新IDへ移す
                    let m = self.meta_store.get(&task_id);
                    self.meta_store.remove(&task_id);
                    self.meta_store.set(new_task.id.clone(), m);
                    if let Err(e) = self.meta_store.save() {
                        self.status_message = Some(format!("メタ保存エラー: {}", e));
                    }
                    self.matrix_selected = Some(new_task.id.clone());
                    self.all_tasks[idx] = new_task;
                    self.status_message = Some("タスクを更新しました".to_string());
                }
                Err(e) => {
                    self.status_message = Some(format!("エラー: {}", e));
                }
            }
        }
    }

    async fn handle_matrix_stack_cat(&mut self, key: KeyEvent, selected: usize) -> Result<bool> {
        // 選択肢: 4カテゴリー + スタック解除
        let total = StackCategory::ALL.len() + 1;
        match key.code {
            KeyCode::Esc => self.mode = AppMode::Matrix,
            KeyCode::Char('j') | KeyCode::Down => {
                self.mode = AppMode::MatrixStackCat {
                    selected: (selected + 1).min(total - 1),
                };
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.mode = AppMode::MatrixStackCat {
                    selected: selected.saturating_sub(1),
                };
            }
            KeyCode::Enter => {
                if selected < StackCategory::ALL.len() {
                    let category = StackCategory::ALL[selected];
                    let note = self
                        .selected_matrix_task()
                        .and_then(|t| self.meta_store.get(&t.id).stack_note)
                        .unwrap_or_default();
                    self.mode = AppMode::MatrixStackNote {
                        category: Some(category),
                        note,
                    };
                } else {
                    self.apply_stack(None, None).await;
                }
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_matrix_stack_note(
        &mut self,
        key: KeyEvent,
        category: Option<StackCategory>,
        mut note: String,
    ) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                let selected = category
                    .and_then(|c| StackCategory::ALL.iter().position(|x| *x == c))
                    .unwrap_or(0);
                self.mode = AppMode::MatrixStackCat { selected };
            }
            KeyCode::Enter => {
                self.apply_stack(category, Some(note)).await;
            }
            KeyCode::Char(c) => {
                note.push(c);
                self.mode = AppMode::MatrixStackNote { category, note };
            }
            KeyCode::Backspace => {
                note.pop();
                self.mode = AppMode::MatrixStackNote { category, note };
            }
            _ => {}
        }
        Ok(false)
    }

    /// 選択中タスクのスタック状態をローカルストアに保存する(Google Tasks には送らない)
    async fn apply_stack(&mut self, category: Option<StackCategory>, note: Option<String>) {
        if let Some(id) = self.matrix_selected.clone() {
            if self.all_tasks.iter().any(|t| t.id == id) {
                let mut m = self.meta_store.get(&id);
                m.stack = category;
                m.stack_note = if category.is_some() {
                    note.filter(|n| !n.trim().is_empty())
                } else {
                    None
                };
                self.meta_store.set(id, m);
                match self.meta_store.save() {
                    Ok(()) => {
                        self.status_message = Some(
                            if category.is_some() {
                                "スタックにしました"
                            } else {
                                "スタックを解除しました"
                            }
                            .to_string(),
                        );
                    }
                    Err(e) => {
                        self.status_message = Some(format!("メタ保存エラー: {}", e));
                    }
                }
            }
        }
        self.mode = AppMode::Matrix;
    }

    async fn handle_matrix_delete_confirm(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Char('y') => {
                if let Some(task) = self.selected_matrix_task().cloned() {
                    match self.tasks_client.delete_task(&task.list_id, &task.id).await {
                        Ok(()) => {
                            self.all_tasks.retain(|t| t.id != task.id);
                            self.meta_store.remove(&task.id);
                            let _ = self.meta_store.save();
                            self.matrix_selected = None;
                            self.ensure_matrix_selection();
                            self.status_message = Some("タスクを削除しました".to_string());
                        }
                        Err(e) => {
                            self.status_message = Some(format!("エラー: {}", e));
                        }
                    }
                }
                self.mode = AppMode::Matrix;
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                self.mode = AppMode::Matrix;
            }
            _ => {}
        }
        Ok(false)
    }

    fn move_in_grid(&mut self, delta: i64) {
        let dates = self.grid_dates();
        if let Some(idx) = dates.iter().position(|d| *d == self.selected_date) {
            let new_idx = idx as i64 + delta;
            if new_idx >= 0 && (new_idx as usize) < dates.len() {
                self.selected_date = dates[new_idx as usize];
            }
        }
    }

    async fn change_month(&mut self, delta: i32) -> Result<()> {
        let y = self.current_month.year();
        let m = self.current_month.month() as i32 + delta;
        let (ny, nm) = if m < 1 {
            (y - 1, 12u32)
        } else if m > 12 {
            (y + 1, 1u32)
        } else {
            (y, m as u32)
        };
        self.current_month = NaiveDate::from_ymd_opt(ny, nm, 1).unwrap();
        self.selected_date = self.current_month;
        self.load_events().await?;
        let _ = self.load_tasks().await;
        Ok(())
    }
}

fn expand_event_to_dates(
    events: &mut HashMap<NaiveDate, Vec<crate::calendar::Event>>,
    event: crate::calendar::Event,
) {
    let end = event.end_date.unwrap_or(event.date);
    let mut d = event.date;
    while d <= end {
        events.entry(d).or_default().push(event.clone());
        d += Duration::days(1);
    }
}

/// "0"〜"10" の入力を検証する(重要度・clau度)
fn parse_score(input: &str) -> Option<u8> {
    input.trim().parse::<u8>().ok().filter(|v| (0..=10).contains(v))
}

fn parse_date_input(input: &str) -> Option<NaiveDate> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    NaiveDate::parse_from_str(trimmed, "%Y-%m-%d").ok().or_else(|| {
        let today = Local::now().date_naive();
        NaiveDate::parse_from_str(&format!("{}-{}", today.year(), trimmed), "%Y-%m-%d").ok()
    })
}
