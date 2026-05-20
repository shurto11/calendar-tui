use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub title: String,
    pub date: NaiveDate,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub is_holiday: bool,
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

    pub async fn list_events(
        &self,
        calendar_id: &str,
        time_min: DateTime<Utc>,
        time_max: DateTime<Utc>,
        is_holiday: bool,
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
                if let Some(event) = parse_event(item, is_holiday) {
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
            )
            .await
        {
            Ok(events) => Ok(events),
            Err(_) => Ok(vec![]),
        }
    }

    pub async fn create_event(&self, title: &str, date: NaiveDate) -> Result<Event> {
        let date_str = date.format("%Y-%m-%d").to_string();
        let body = serde_json::json!({
            "summary": title,
            "start": { "date": date_str },
            "end": { "date": date_str },
        });

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
            start_time: None,
            end_time: None,
            is_holiday: false,
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

    pub async fn update_event(&self, event_id: &str, title: &str) -> Result<()> {
        let body = serde_json::json!({ "summary": title });
        self.client
            .patch(format!(
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

fn parse_event(item: ApiEvent, is_holiday: bool) -> Option<Event> {
    let title = item.summary.unwrap_or_else(|| "(無題)".to_string());

    let (date, start_time, end_time) = if let Some(dt_str) = &item.start.date_time {
        let dt = DateTime::parse_from_rfc3339(dt_str).ok()?;
        let end_time = item
            .end
            .as_ref()
            .and_then(|e| e.date_time.as_ref())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.format("%H:%M").to_string());
        (
            dt.date_naive(),
            Some(dt.format("%H:%M").to_string()),
            end_time,
        )
    } else if let Some(date_str) = &item.start.date {
        (
            NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()?,
            None,
            None,
        )
    } else {
        return None;
    };

    Some(Event {
        id: item.id,
        title,
        date,
        start_time,
        end_time,
        is_holiday,
    })
}
