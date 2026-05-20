use anyhow::Result;
use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone, Utc, Weekday};
use crossterm::event::{KeyCode, KeyEvent};
use std::collections::HashMap;

use crate::calendar::{CalendarClient, Event};
use crate::tasks::{Task, TaskList, TasksClient};

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
    EditTask { task_index: usize, title: String },
    EditTaskSelectList { task_index: usize, title: String, selected: usize },
    TaskList { selected: usize },
    TaskListAdd { title: String },
    TaskListAddSelectList { title: String, selected_list: usize },
    TaskListAddDate { title: String, list_id: String, date_input: String },
    TaskListEdit { task_index: usize, title: String },
    TaskListEditSelectList { task_index: usize, title: String, selected_list: usize },
    TaskListEditDate { task_index: usize, title: String, list_id: String, date_input: String },
    TaskListDeleteConfirm { task_index: usize },
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

        let mut app = App {
            current_month,
            selected_date: today,
            events: HashMap::new(),
            tasks: HashMap::new(),
            all_tasks: Vec::new(),
            mode: AppMode::Normal,
            status_message: None,
            task_lists,
            calendar_client: client,
            tasks_client,
        };

        app.load_events().await?;
        let _ = app.load_tasks().await;
        Ok(app)
    }

    pub async fn load_events(&mut self) -> Result<()> {
        let (time_min, time_max) = self.month_range();

        let (primary, holidays) = tokio::join!(
            self.calendar_client
                .list_events("primary", time_min, time_max, false),
            self.calendar_client.list_holiday_events(time_min, time_max),
        );

        self.events.clear();
        for event in primary.unwrap_or_default() {
            self.events.entry(event.date).or_default().push(event);
        }
        for event in holidays.unwrap_or_default() {
            self.events.entry(event.date).or_default().push(event);
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
            AppMode::EditTask { task_index, title } => {
                self.handle_edit_task(key, task_index, title).await
            }
            AppMode::EditTaskSelectList { task_index, title, selected } => {
                self.handle_edit_task_select_list(key, task_index, title, selected).await
            }
            AppMode::TaskList { selected } => self.handle_task_list(key, selected).await,
            AppMode::TaskListAdd { title } => self.handle_task_list_add(key, title).await,
            AppMode::TaskListAddSelectList { title, selected_list } => {
                self.handle_task_list_add_select_list(key, title, selected_list).await
            }
            AppMode::TaskListAddDate { title, list_id, date_input } => {
                self.handle_task_list_add_date(key, title, list_id, date_input).await
            }
            AppMode::TaskListEdit { task_index, title } => {
                self.handle_task_list_edit(key, task_index, title).await
            }
            AppMode::TaskListEditSelectList { task_index, title, selected_list } => {
                self.handle_task_list_edit_select_list(key, task_index, title, selected_list).await
            }
            AppMode::TaskListEditDate { task_index, title, list_id, date_input } => {
                self.handle_task_list_edit_date(key, task_index, title, list_id, date_input).await
            }
            AppMode::TaskListDeleteConfirm { task_index } => {
                self.handle_task_list_delete_confirm(key, task_index).await
            }
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
                self.mode = AppMode::TaskList { selected: 0 };
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
                    match self
                        .calendar_client
                        .create_event(&title, self.selected_date)
                        .await
                    {
                        Ok(event) => {
                            self.events
                                .entry(self.selected_date)
                                .or_default()
                                .push(event);
                            self.status_message = Some("予定を追加しました".to_string());
                        }
                        Err(e) => {
                            self.status_message = Some(format!("エラー: {}", e));
                        }
                    }
                }
                self.mode = AppMode::Normal;
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
                    let events = self.events.get(&self.selected_date).cloned().unwrap_or_default();
                    if let Some(event) = events.get(event_index) {
                        let event_id = event.id.clone();
                        match self.calendar_client.update_event(&event_id, &trimmed).await {
                            Ok(()) => {
                                if let Some(evs) = self.events.get_mut(&self.selected_date) {
                                    if let Some(e) = evs.get_mut(event_index) {
                                        e.title = trimmed;
                                    }
                                }
                                self.status_message = Some("予定を更新しました".to_string());
                            }
                            Err(e) => {
                                self.status_message = Some(format!("エラー: {}", e));
                            }
                        }
                    }
                }
                self.mode = AppMode::Normal;
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
            .create_task(&list_id, &title, Some(self.selected_date))
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
                let events = self.events.entry(self.selected_date).or_default();
                if idx < events.len() {
                    let event_id = events[idx].id.clone();
                    match self.calendar_client.delete_event(&event_id).await {
                        Ok(()) => {
                            self.events
                                .entry(self.selected_date)
                                .or_default()
                                .remove(idx);
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

    pub async fn load_all_tasks(&mut self) -> Result<()> {
        self.all_tasks.clear();
        for list in &self.task_lists {
            if let Ok(tasks) = self.tasks_client.list_all_tasks(&list.id).await {
                self.all_tasks.extend(tasks);
            }
        }
        Ok(())
    }

    async fn handle_task_list(&mut self, key: KeyEvent, selected: usize) -> Result<bool> {
        let n = self.all_tasks.len();
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                let _ = self.load_tasks().await;
                self.mode = AppMode::Normal;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if n > 0 {
                    self.mode = AppMode::TaskList { selected: (selected + 1).min(n - 1) };
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.mode = AppMode::TaskList { selected: selected.saturating_sub(1) };
            }
            KeyCode::Char('n') => {
                self.mode = AppMode::TaskListAdd { title: String::new() };
            }
            KeyCode::Char('t') => {
                if n > 0 && selected < n {
                    let task = self.all_tasks[selected].clone();
                    match self.tasks_client.toggle_task(&task).await {
                        Ok(updated) => {
                            self.all_tasks[selected] = updated;
                            self.status_message = Some("完了状態を切り替えました".to_string());
                        }
                        Err(e) => {
                            self.status_message = Some(format!("エラー: {}", e));
                        }
                    }
                    self.mode = AppMode::TaskList { selected };
                }
            }
            KeyCode::Char('d') => {
                if n > 0 && selected < n {
                    self.mode = AppMode::TaskListDeleteConfirm { task_index: selected };
                }
            }
            KeyCode::Char('e') => {
                if n > 0 && selected < n {
                    let title = self.all_tasks[selected].title.clone();
                    self.mode = AppMode::TaskListEdit { task_index: selected, title };
                }
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_task_list_add(&mut self, key: KeyEvent, mut title: String) -> Result<bool> {
        match key.code {
            KeyCode::Esc => self.mode = AppMode::TaskList { selected: 0 },
            KeyCode::Enter => {
                if !title.trim().is_empty() {
                    if self.task_lists.is_empty() {
                        self.mode = AppMode::AddTaskNewList { title, new_list: String::new() };
                    } else {
                        self.mode = AppMode::TaskListAddSelectList { title, selected_list: 0 };
                    }
                } else {
                    self.mode = AppMode::TaskList { selected: 0 };
                }
            }
            KeyCode::Char(c) => {
                title.push(c);
                self.mode = AppMode::TaskListAdd { title };
            }
            KeyCode::Backspace => {
                title.pop();
                self.mode = AppMode::TaskListAdd { title };
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_task_list_add_select_list(
        &mut self,
        key: KeyEvent,
        title: String,
        selected_list: usize,
    ) -> Result<bool> {
        let total = self.task_lists.len() + 1;
        match key.code {
            KeyCode::Esc => self.mode = AppMode::TaskListAdd { title },
            KeyCode::Char('j') | KeyCode::Down => {
                self.mode = AppMode::TaskListAddSelectList {
                    title,
                    selected_list: (selected_list + 1).min(total - 1),
                };
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.mode = AppMode::TaskListAddSelectList {
                    title,
                    selected_list: selected_list.saturating_sub(1),
                };
            }
            KeyCode::Enter => {
                if selected_list == self.task_lists.len() {
                    self.mode = AppMode::AddTaskNewList { title, new_list: String::new() };
                } else {
                    let list_id = self.task_lists[selected_list].id.clone();
                    self.mode = AppMode::TaskListAddDate {
                        title,
                        list_id,
                        date_input: String::new(),
                    };
                }
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_task_list_add_date(
        &mut self,
        key: KeyEvent,
        title: String,
        list_id: String,
        mut date_input: String,
    ) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                let selected_list = self.task_lists.iter().position(|l| l.id == list_id).unwrap_or(0);
                self.mode = AppMode::TaskListAddSelectList { title, selected_list };
            }
            KeyCode::Enter => {
                let due = if date_input.trim().is_empty() {
                    None
                } else {
                    NaiveDate::parse_from_str(date_input.trim(), "%Y-%m-%d").ok()
                        .or_else(|| {
                            let today = Local::now().date_naive();
                            NaiveDate::parse_from_str(
                                &format!("{}-{}", today.year(), date_input.trim()),
                                "%Y-%m-%d",
                            ).ok()
                        })
                };
                match self.tasks_client.create_task(&list_id, &title, due).await {
                    Ok(task) => {
                        if let Some(d) = task.due {
                            self.tasks.entry(d).or_default().push(task.clone());
                        }
                        self.all_tasks.push(task);
                        self.status_message = Some("タスクを追加しました".to_string());
                    }
                    Err(e) => {
                        self.status_message = Some(format!("エラー: {}", e));
                    }
                }
                let selected = self.all_tasks.len().saturating_sub(1);
                self.mode = AppMode::TaskList { selected };
            }
            KeyCode::Char(c) => {
                date_input.push(c);
                self.mode = AppMode::TaskListAddDate { title, list_id, date_input };
            }
            KeyCode::Backspace => {
                date_input.pop();
                self.mode = AppMode::TaskListAddDate { title, list_id, date_input };
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_task_list_edit(
        &mut self,
        key: KeyEvent,
        task_index: usize,
        mut title: String,
    ) -> Result<bool> {
        match key.code {
            KeyCode::Esc => self.mode = AppMode::TaskList { selected: task_index },
            KeyCode::Enter => {
                if !title.trim().is_empty() {
                    let selected_list = self.all_tasks.get(task_index)
                        .and_then(|t| self.task_lists.iter().position(|l| l.id == t.list_id))
                        .unwrap_or(0);
                    self.mode = AppMode::TaskListEditSelectList { task_index, title, selected_list };
                } else {
                    self.mode = AppMode::TaskList { selected: task_index };
                }
            }
            KeyCode::Char(c) => {
                title.push(c);
                self.mode = AppMode::TaskListEdit { task_index, title };
            }
            KeyCode::Backspace => {
                title.pop();
                self.mode = AppMode::TaskListEdit { task_index, title };
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_task_list_edit_select_list(
        &mut self,
        key: KeyEvent,
        task_index: usize,
        title: String,
        selected_list: usize,
    ) -> Result<bool> {
        let total = self.task_lists.len();
        match key.code {
            KeyCode::Esc => self.mode = AppMode::TaskListEdit { task_index, title },
            KeyCode::Char('j') | KeyCode::Down => {
                self.mode = AppMode::TaskListEditSelectList {
                    task_index,
                    title,
                    selected_list: (selected_list + 1).min(total.saturating_sub(1)),
                };
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.mode = AppMode::TaskListEditSelectList {
                    task_index,
                    title,
                    selected_list: selected_list.saturating_sub(1),
                };
            }
            KeyCode::Enter => {
                if let Some(task) = self.all_tasks.get(task_index).cloned() {
                    let new_list_id = self.task_lists[selected_list].id.clone();
                    let current_due = task.due;
                    let due_str = current_due
                        .map(|d| d.format("%Y-%m-%d").to_string())
                        .unwrap_or_default();
                    self.mode = AppMode::TaskListEditDate {
                        task_index,
                        title,
                        list_id: new_list_id,
                        date_input: due_str,
                    };
                } else {
                    self.mode = AppMode::TaskList { selected: task_index };
                }
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_task_list_edit_date(
        &mut self,
        key: KeyEvent,
        task_index: usize,
        title: String,
        list_id: String,
        mut date_input: String,
    ) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                let selected_list = self.task_lists.iter().position(|l| l.id == list_id).unwrap_or(0);
                self.mode = AppMode::TaskListEditSelectList { task_index, title, selected_list };
            }
            KeyCode::Enter => {
                let new_due = if date_input.trim().is_empty() {
                    None
                } else {
                    NaiveDate::parse_from_str(date_input.trim(), "%Y-%m-%d").ok()
                        .or_else(|| {
                            let today = Local::now().date_naive();
                            NaiveDate::parse_from_str(
                                &format!("{}-{}", today.year(), date_input.trim()),
                                "%Y-%m-%d",
                            ).ok()
                        })
                };
                if let Some(task) = self.all_tasks.get(task_index).cloned() {
                    let new_title = title.trim().to_string();
                    if task.list_id == list_id {
                        match self.tasks_client.update_task(&task.list_id, &task.id, &new_title, new_due).await {
                            Ok(()) => {
                                if let Some(t) = self.all_tasks.get_mut(task_index) {
                                    t.title = new_title;
                                    t.due = new_due;
                                }
                                self.status_message = Some("タスクを更新しました".to_string());
                            }
                            Err(e) => {
                                self.status_message = Some(format!("エラー: {}", e));
                            }
                        }
                    } else {
                        let updated = Task {
                            title: new_title.clone(),
                            due: new_due,
                            ..task.clone()
                        };
                        match self.tasks_client.move_task_to_list(&updated, &list_id, &new_title).await {
                            Ok(new_task) => {
                                self.all_tasks[task_index] = new_task;
                                self.status_message = Some("タスクを更新しました".to_string());
                            }
                            Err(e) => {
                                self.status_message = Some(format!("エラー: {}", e));
                            }
                        }
                    }
                }
                self.mode = AppMode::TaskList { selected: task_index };
            }
            KeyCode::Char(c) => {
                date_input.push(c);
                self.mode = AppMode::TaskListEditDate { task_index, title, list_id, date_input };
            }
            KeyCode::Backspace => {
                date_input.pop();
                self.mode = AppMode::TaskListEditDate { task_index, title, list_id, date_input };
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_task_list_delete_confirm(
        &mut self,
        key: KeyEvent,
        task_index: usize,
    ) -> Result<bool> {
        match key.code {
            KeyCode::Char('y') => {
                if let Some(task) = self.all_tasks.get(task_index).cloned() {
                    match self.tasks_client.delete_task(&task.list_id, &task.id).await {
                        Ok(()) => {
                            self.all_tasks.remove(task_index);
                            self.status_message = Some("タスクを削除しました".to_string());
                        }
                        Err(e) => {
                            self.status_message = Some(format!("エラー: {}", e));
                        }
                    }
                }
                let selected = task_index.min(self.all_tasks.len().saturating_sub(1));
                self.mode = AppMode::TaskList { selected };
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                self.mode = AppMode::TaskList { selected: task_index };
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
