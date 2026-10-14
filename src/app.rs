use anyhow::Result;
use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone, Utc, Weekday};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use std::collections::HashMap;

use crate::calendar::{CalendarClient, CalendarInfo, Event};
use crate::touch::TouchInput;

#[derive(Debug, Clone, PartialEq)]
pub enum AppMode {
    Normal,
    AddEvent { title: String },
    SelectItemDelete { selected: usize },
    SelectItemEdit { selected: usize },
    DeleteConfirm { event_index: usize },
    EditEvent { event_index: usize, title: String },
    EditEventTime { event_index: usize, title: String, time_input: String },
    EditEventEndDate { event_index: usize, title: String, start_time: Option<String>, end_date_input: String },
    AddEventTime { title: String, time_input: String },
    AddEventEndTime { title: String, start_time: String, end_time_input: String },
    AddEventEndDate { title: String, start_time: Option<String>, end_time: Option<String>, end_date_input: String },
}

pub struct App {
    pub current_month: NaiveDate,
    pub selected_date: NaiveDate,
    pub events: HashMap<NaiveDate, Vec<Event>>,
    pub mode: AppMode,
    pub status_message: Option<String>,
    pub calendars: Vec<CalendarInfo>,
    /// 直近の描画領域。main ループが毎フレーム更新する
    pub viewport: Rect,
    calendar_client: CalendarClient,
}

impl App {
    pub async fn new() -> Result<Self> {
        let token = crate::auth::load_or_authenticate().await?;
        let client = CalendarClient::new(token.access_token);

        let today = Local::now().date_naive();
        let current_month =
            NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap();

        let calendars = client.list_calendars().await.unwrap_or_default();

        let mut app = App {
            current_month,
            selected_date: today,
            events: HashMap::new(),
            mode: AppMode::Normal,
            status_message: None,
            calendars,
            viewport: Rect::default(),
            calendar_client: client,
        };

        app.load_events().await?;
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

    pub async fn handle_key(&mut self, key: KeyEvent) -> Result<bool> {
        match self.mode.clone() {
            AppMode::Normal => self.handle_normal(key).await,
            AppMode::AddEvent { title } => self.handle_add_event(key, title).await,
            AppMode::SelectItemDelete { selected } => {
                self.handle_select_item_delete(key, selected).await
            }
            AppMode::SelectItemEdit { selected } => {
                self.handle_select_item_edit(key, selected).await
            }
            AppMode::DeleteConfirm { event_index } => {
                self.handle_delete_confirm(key, event_index).await
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
            KeyCode::Char('d') => {
                if self.selected_events().iter().all(|e| e.is_holiday) {
                    self.status_message = Some("この日に削除できる予定はありません".to_string());
                } else {
                    self.mode = AppMode::SelectItemDelete { selected: 0 };
                }
            }
            KeyCode::Char('e') => {
                if self.selected_events().iter().all(|e| e.is_holiday) {
                    self.status_message = Some("この日に編集できる予定はありません".to_string());
                } else {
                    self.mode = AppMode::SelectItemEdit { selected: 0 };
                }
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

    async fn handle_select_item_delete(&mut self, key: KeyEvent, selected: usize) -> Result<bool> {
        let non_holiday_indices: Vec<usize> = self.selected_events().iter()
            .enumerate()
            .filter(|(_, e)| !e.is_holiday)
            .map(|(i, _)| i)
            .collect();
        let total = non_holiday_indices.len();

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
                if let Some(&event_index) = non_holiday_indices.get(selected) {
                    self.mode = AppMode::DeleteConfirm { event_index };
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
        let total = non_holiday_indices.len();

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
                if let Some(&event_index) = non_holiday_indices.get(selected) {
                    let title = self.selected_events()[event_index].title.clone();
                    self.mode = AppMode::EditEvent { event_index, title };
                }
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

    /// touch-server から届いたタッチ入力を処理する。カレンダー画面以外の
    /// 入力・編集中モードでは無視する。戻り値 true で終了(現状タッチでは無し)。
    pub async fn handle_touch(&mut self, input: TouchInput) -> Result<bool> {
        match self.mode {
            AppMode::Normal => self.handle_touch_calendar(input),
            _ => {}
        }
        Ok(false)
    }

    /// カレンダー画面: タップした位置の日付セルを選択する。
    fn handle_touch_calendar(&mut self, input: TouchInput) {
        let TouchInput::Tap { col, row } = input;
        if let Some(date) = self.date_at(col, row) {
            self.selected_date = date;
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
