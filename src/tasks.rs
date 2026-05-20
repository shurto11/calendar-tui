use anyhow::{bail, Result};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct TaskList {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct Task {
    pub id: String,
    pub list_id: String,
    pub title: String,
    pub due: Option<NaiveDate>,
    pub completed: bool,
}

#[derive(Deserialize)]
struct TaskListsResponse {
    items: Option<Vec<ApiTaskList>>,
}

#[derive(Deserialize)]
struct ApiTaskList {
    id: String,
    title: Option<String>,
}

#[derive(Deserialize)]
struct TasksResponse {
    items: Option<Vec<ApiTask>>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct ApiTask {
    id: String,
    title: Option<String>,
    status: Option<String>,
    due: Option<String>,
}

pub struct TasksClient {
    access_token: String,
    client: reqwest::Client,
}

impl TasksClient {
    pub fn new(access_token: String) -> Self {
        Self {
            access_token,
            client: reqwest::Client::new(),
        }
    }

    pub async fn list_task_lists(&self) -> Result<Vec<TaskList>> {
        let response = self
            .client
            .get("https://tasks.googleapis.com/tasks/v1/users/@me/lists")
            .bearer_auth(&self.access_token)
            .query(&[("maxResults", "100")])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Tasks API エラー {}: {}", status, body);
        }

        let resp: TaskListsResponse = response.json().await?;
        Ok(resp
            .items
            .unwrap_or_default()
            .into_iter()
            .map(|l| TaskList {
                id: l.id,
                title: l.title.unwrap_or_else(|| "無題".to_string()),
            })
            .collect())
    }

    pub async fn create_task_list(&self, title: &str) -> Result<TaskList> {
        let body = serde_json::json!({ "title": title });
        let response = self
            .client
            .post("https://tasks.googleapis.com/tasks/v1/users/@me/lists")
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Tasks API エラー {}: {}", status, body);
        }

        let resp: serde_json::Value = response.json().await?;
        Ok(TaskList {
            id: resp["id"].as_str().unwrap_or("").to_string(),
            title: resp["title"].as_str().unwrap_or(title).to_string(),
        })
    }

    pub async fn list_tasks_in(
        &self,
        list_id: &str,
        time_min: DateTime<Utc>,
        time_max: DateTime<Utc>,
    ) -> Result<Vec<Task>> {
        let mut tasks = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let mut req = self
                .client
                .get(format!(
                    "https://tasks.googleapis.com/tasks/v1/lists/{}/tasks",
                    list_id
                ))
                .bearer_auth(&self.access_token)
                .query(&[
                    ("dueMin", time_min.to_rfc3339()),
                    ("dueMax", time_max.to_rfc3339()),
                    ("showCompleted", "true".to_string()),
                    ("showHidden", "true".to_string()),
                    ("maxResults", "100".to_string()),
                ]);

            if let Some(token) = &page_token {
                req = req.query(&[("pageToken", token)]);
            }

            let response = req.send().await?;
            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                bail!("Tasks API エラー {}: {}", status, body);
            }
            let resp: TasksResponse = response.json().await?;

            for item in resp.items.unwrap_or_default() {
                if let Some(task) = parse_task(item, list_id) {
                    tasks.push(task);
                }
            }

            page_token = resp.next_page_token;
            if page_token.is_none() {
                break;
            }
        }

        Ok(tasks)
    }

    pub async fn list_all_tasks(&self, list_id: &str) -> Result<Vec<Task>> {
        let mut tasks = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let mut req = self
                .client
                .get(format!(
                    "https://tasks.googleapis.com/tasks/v1/lists/{}/tasks",
                    list_id
                ))
                .bearer_auth(&self.access_token)
                .query(&[
                    ("showCompleted", "true".to_string()),
                    ("showHidden", "true".to_string()),
                    ("maxResults", "100".to_string()),
                ]);

            if let Some(token) = &page_token {
                req = req.query(&[("pageToken", token)]);
            }

            let response = req.send().await?;
            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                bail!("Tasks API エラー {}: {}", status, body);
            }
            let resp: TasksResponse = response.json().await?;

            for item in resp.items.unwrap_or_default() {
                if let Some(task) = parse_task(item, list_id) {
                    tasks.push(task);
                }
            }

            page_token = resp.next_page_token;
            if page_token.is_none() {
                break;
            }
        }

        Ok(tasks)
    }

    pub async fn create_task(&self, list_id: &str, title: &str, due: Option<NaiveDate>) -> Result<Task> {
        let mut body = serde_json::json!({ "title": title });
        if let Some(d) = due {
            body["due"] = serde_json::Value::String(format!("{}T00:00:00.000Z", d.format("%Y-%m-%d")));
        }

        let response = self
            .client
            .post(format!(
                "https://tasks.googleapis.com/tasks/v1/lists/{}/tasks",
                list_id
            ))
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Tasks API エラー {}: {}", status, body);
        }

        let resp: serde_json::Value = response.json().await?;

        Ok(Task {
            id: resp["id"].as_str().unwrap_or("").to_string(),
            list_id: list_id.to_string(),
            title: title.to_string(),
            due,
            completed: false,
        })
    }

    pub async fn delete_task(&self, list_id: &str, task_id: &str) -> Result<()> {
        let response = self
            .client
            .delete(format!(
                "https://tasks.googleapis.com/tasks/v1/lists/{}/tasks/{}",
                list_id, task_id
            ))
            .bearer_auth(&self.access_token)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Tasks API エラー {}: {}", status, body);
        }

        Ok(())
    }

    pub async fn update_task_title(&self, list_id: &str, task_id: &str, title: &str) -> Result<()> {
        let body = serde_json::json!({ "title": title });
        let response = self
            .client
            .patch(format!(
                "https://tasks.googleapis.com/tasks/v1/lists/{}/tasks/{}",
                list_id, task_id
            ))
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Tasks API エラー {}: {}", status, body);
        }
        Ok(())
    }

    pub async fn update_task(&self, list_id: &str, task_id: &str, title: &str, due: Option<NaiveDate>) -> Result<()> {
        let mut body = serde_json::json!({ "title": title });
        match due {
            Some(d) => {
                body["due"] = serde_json::Value::String(format!("{}T00:00:00.000Z", d.format("%Y-%m-%d")));
            }
            None => {
                body["due"] = serde_json::Value::Null;
            }
        }
        let response = self
            .client
            .patch(format!(
                "https://tasks.googleapis.com/tasks/v1/lists/{}/tasks/{}",
                list_id, task_id
            ))
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Tasks API エラー {}: {}", status, body);
        }
        Ok(())
    }

    pub async fn move_task_to_list(&self, task: &Task, new_list_id: &str, new_title: &str) -> Result<Task> {
        let new_task = self.create_task(new_list_id, new_title, task.due).await?;
        let _ = self.delete_task(&task.list_id, &task.id).await;
        Ok(Task {
            completed: task.completed,
            ..new_task
        })
    }

    pub async fn toggle_task(&self, task: &Task) -> Result<Task> {
        let new_status = if task.completed {
            "needsAction"
        } else {
            "completed"
        };
        let body = serde_json::json!({ "status": new_status });

        let response = self
            .client
            .patch(format!(
                "https://tasks.googleapis.com/tasks/v1/lists/{}/tasks/{}",
                task.list_id, task.id
            ))
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Tasks API エラー {}: {}", status, body);
        }

        let resp: serde_json::Value = response.json().await?;

        Ok(Task {
            id: task.id.clone(),
            list_id: task.list_id.clone(),
            title: task.title.clone(),
            due: task.due,
            completed: resp["status"].as_str() == Some("completed"),
        })
    }
}

fn parse_task(item: ApiTask, list_id: &str) -> Option<Task> {
    let title = item.title.unwrap_or_default();
    if title.is_empty() {
        return None;
    }

    let due = item.due.as_ref().and_then(|s| {
        NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok()
    });

    Some(Task {
        id: item.id,
        list_id: list_id.to_string(),
        title,
        due,
        completed: item.status.as_deref() == Some("completed"),
    })
}
