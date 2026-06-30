use anyhow::Result;
use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct CalendarInfo {
    pub id: String,
    pub summary: String,
    pub is_primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub title: String,
    pub date: NaiveDate,
    pub end_date: Option<NaiveDate>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub is_holiday: bool,
    pub calendar_name: Option<String>,
}

#[derive(Deserialize)]
struct CalendarListResponse {
    items: Option<Vec<CalendarListItem>>,
}

#[derive(Deserialize)]
struct CalendarListItem {
    id: String,
    summary: String,
    #[serde(default)]
    primary: bool,
    #[serde(rename = "accessRole")]
    access_role: String,
}

#[derive(Deserialize)]
struct EventsResponse {
    items: Option<Vec<ApiEvent>>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct ApiEvent {
    id: String,
    summary: Option<String>,
    start: EventDateTime,
    end: Option<EventDateTime>,
}

#[derive(Deserialize)]
struct EventDateTime {
    #[serde(rename = "dateTime")]
    date_time: Option<String>,
    date: Option<String>,
}

pub struct CalendarClient {
    access_token: String,
    client: reqwest::Client,
}

impl CalendarClient {
    pub fn new(access_token: String) -> Self {
        Self {
            access_token,
            client: reqwest::Client::new(),
        }
    }

    pub async fn list_calendars(&self) -> Result<Vec<CalendarInfo>> {
        let excluded = crate::config::load_excluded_calendars();

        let resp: CalendarListResponse = self
            .client
            .get("https://www.googleapis.com/calendar/v3/users/me/calendarList")
            .bearer_auth(&self.access_token)
            .send()
            .await?
            .json()
            .await?;

        let calendars = resp
            .items
            .unwrap_or_default()
            .into_iter()
            .filter(|item| {
                item.id != "ja.japanese#holiday@group.v.calendar.google.com"
                    && item.access_role != "freeBusyReader"
                    && !excluded.contains(&item.summary)
            })
            .map(|item| CalendarInfo {
                is_primary: item.primary,
                summary: item.summary,
                id: item.id,
            })
            .collect();

        Ok(calendars)
    }

    pub async fn list_events(
        &self,
        calendar_id: &str,
        time_min: DateTime<Utc>,
        time_max: DateTime<Utc>,
        is_holiday: bool,
        calendar_name: Option<String>,
    ) -> Result<Vec<Event>> {
        let mut events = Vec::new();
        let mut page_token: Option<String> = None;
        let encoded_id = urlencoding::encode(calendar_id).into_owned();

        loop {
            let mut req = self
                .client
                .get(format!(
                    "https://www.googleapis.com/calendar/v3/calendars/{}/events",
                    encoded_id
                ))
                .bearer_auth(&self.access_token)
                .query(&[
                    ("timeMin", time_min.to_rfc3339()),
                    ("timeMax", time_max.to_rfc3339()),
                    ("singleEvents", "true".to_string()),
                    ("orderBy", "startTime".to_string()),
                    ("maxResults", "250".to_string()),
                ]);

            if let Some(token) = &page_token {
                req = req.query(&[("pageToken", token)]);
            }

            let resp: EventsResponse = req.send().await?.json().await?;

            for item in resp.items.unwrap_or_default() {
                if let Some(event) = parse_event(item, is_holiday, calendar_name.clone()) {
                    events.push(event);
                }
            }

            page_token = resp.next_page_token;
            if page_token.is_none() {
                break;
            }
        }

        Ok(events)
    }

    pub async fn list_holiday_events(
        &self,
        time_min: DateTime<Utc>,
        time_max: DateTime<Utc>,
    ) -> Result<Vec<Event>> {
        match self
            .list_events(
                "ja.japanese#holiday@group.v.calendar.google.com",
                time_min,
                time_max,
                true,
                None,
            )
            .await
        {
            Ok(events) => Ok(events),
            Err(_) => Ok(vec![]),
        }
    }

    pub async fn create_event(
        &self,
        title: &str,
        date: NaiveDate,
        start_time: Option<&str>,
        end_time: Option<&str>,
        end_date: Option<NaiveDate>,
    ) -> Result<Event> {
        let body = build_event_body(title, date, start_time, end_time, end_date)?;

        let resp: serde_json::Value = self
            .client
            .post("https://www.googleapis.com/calendar/v3/calendars/primary/events")
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        Ok(Event {
            id: resp["id"].as_str().unwrap_or("").to_string(),
            title: title.to_string(),
            date,
            end_date,
            start_time: start_time.map(|s| s.to_string()),
            end_time: end_time.map(|s| s.to_string()),
            is_holiday: false,
            calendar_name: None,
        })
    }

    pub async fn delete_event(&self, event_id: &str) -> Result<()> {
        self.client
            .delete(format!(
                "https://www.googleapis.com/calendar/v3/calendars/primary/events/{}",
                event_id
            ))
            .bearer_auth(&self.access_token)
            .send()
            .await?;
        Ok(())
    }

    pub async fn update_event(
        &self,
        event_id: &str,
        title: &str,
        date: NaiveDate,
        start_time: Option<&str>,
        end_date: Option<NaiveDate>,
    ) -> Result<()> {
        let body = build_event_body(title, date, start_time, None, end_date)?;
        self.client
            .put(format!(
                "https://www.googleapis.com/calendar/v3/calendars/primary/events/{}",
                event_id
            ))
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .await?;
        Ok(())
    }
}

fn build_event_body(
    title: &str,
    date: NaiveDate,
    start_time: Option<&str>,
    end_time: Option<&str>,
    end_date: Option<NaiveDate>,
) -> Result<serde_json::Value> {
    if let Some(time_str) = start_time {
        let (h, m) = parse_time_str(time_str);

        let naive_start = NaiveDateTime::new(
            date,
            NaiveTime::from_hms_opt(h, m, 0).unwrap_or_default(),
        );
        let local_start = Local
            .from_local_datetime(&naive_start)
            .single()
            .ok_or_else(|| anyhow::anyhow!("ambiguous start datetime"))?;

        let end_date_actual = end_date.unwrap_or(date);
        let naive_end = if let Some(end_str) = end_time {
            let (eh, em) = parse_time_str(end_str);
            NaiveDateTime::new(
                end_date_actual,
                NaiveTime::from_hms_opt(eh, em, 0).unwrap_or_default(),
            )
        } else if end_date_actual == date {
            naive_start + Duration::hours(1)
        } else {
            NaiveDateTime::new(
                end_date_actual,
                NaiveTime::from_hms_opt(h, m, 0).unwrap_or_default(),
            )
        };
        // 終了が開始以前になる入力はデフォルトの1時間に倒す
        let naive_end = if naive_end <= naive_start {
            naive_start + Duration::hours(1)
        } else {
            naive_end
        };
        let local_end = Local
            .from_local_datetime(&naive_end)
            .single()
            .ok_or_else(|| anyhow::anyhow!("ambiguous end datetime"))?;

        Ok(serde_json::json!({
            "summary": title,
            "start": { "dateTime": local_start.to_rfc3339(), "timeZone": "Asia/Tokyo" },
            "end":   { "dateTime": local_end.to_rfc3339(),   "timeZone": "Asia/Tokyo" },
        }))
    } else {
        let start_str = date.format("%Y-%m-%d").to_string();
        let end_str = end_date
            .map(|d| (d + Duration::days(1)).format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| (date + Duration::days(1)).format("%Y-%m-%d").to_string());
        Ok(serde_json::json!({
            "summary": title,
            "start": { "date": start_str },
            "end":   { "date": end_str },
        }))
    }
}

fn parse_time_str(time_str: &str) -> (u32, u32) {
    let parts: Vec<&str> = time_str.splitn(2, ':').collect();
    let h: u32 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let m: u32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    (h.min(23), m.min(59))
}

fn parse_event(item: ApiEvent, is_holiday: bool, calendar_name: Option<String>) -> Option<Event> {
    let title = item.summary.unwrap_or_else(|| "(無題)".to_string());

    let (date, end_date, start_time, end_time) = if let Some(dt_str) = &item.start.date_time {
        // RFC3339はイベント元のタイムゾーン（UTCやヨーロッパ時間など）で返るため、
        // ローカルタイムゾーン（日本時間）へ変換してから表示する
        let dt = DateTime::parse_from_rfc3339(dt_str).ok()?.with_timezone(&Local);
        let end_dt = item
            .end
            .as_ref()
            .and_then(|e| e.date_time.as_ref())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.with_timezone(&Local));
        let end_time = end_dt.map(|d| d.format("%H:%M").to_string());
        let end_date = end_dt
            .map(|d| d.date_naive())
            .filter(|&d| d != dt.date_naive());
        (
            dt.date_naive(),
            end_date,
            Some(dt.format("%H:%M").to_string()),
            end_time,
        )
    } else if let Some(date_str) = &item.start.date {
        let start = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()?;
        // Google Calendar end date is exclusive; convert to inclusive
        let end = item
            .end
            .as_ref()
            .and_then(|e| e.date.as_ref())
            .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
            .map(|d| d - Duration::days(1))
            .filter(|&d| d > start);
        (start, end, None, None)
    } else {
        return None;
    };

    Some(Event {
        id: item.id,
        title,
        date,
        end_date,
        start_time,
        end_time,
        is_holiday,
        calendar_name,
    })
}
