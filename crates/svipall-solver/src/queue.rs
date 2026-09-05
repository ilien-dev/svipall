//! Job queue for solver workers.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Mutex;
use tokio::sync::Notify;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum JobType {
    ImageToText,
    RecaptchaV2,
    RecaptchaV3,
    Turnstile,
    HCaptcha,
    FunCaptcha,
    GeeTest,
    DataDome,
    Unknown,
}

impl JobType {
    /// Map an API task-type or legacy method name to a job type.
    pub fn parse(s: &str) -> Self {
        let lower = s.to_lowercase();
        if lower.contains("imagetotext")
            || lower == "image"
            || lower == "normal"
            || lower == "base64"
        {
            return JobType::ImageToText;
        }
        if lower.contains("recaptchav2") || lower == "userrecaptcha" {
            return JobType::RecaptchaV2;
        }
        if lower.contains("recaptchav3") {
            return JobType::RecaptchaV3;
        }
        if lower.contains("turnstile") {
            return JobType::Turnstile;
        }
        if lower.contains("hcaptcha") {
            return JobType::HCaptcha;
        }
        if lower.contains("funcaptcha") || lower.contains("arkose") {
            return JobType::FunCaptcha;
        }
        if lower.contains("geetest") {
            return JobType::GeeTest;
        }
        if lower.contains("datadome") {
            return JobType::DataDome;
        }
        // fallback for legacy method names
        match lower.as_str() {
            "turnstile" => JobType::Turnstile,
            "hcaptcha" => JobType::HCaptcha,
            _ => JobType::Unknown,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            JobType::ImageToText => "ImageToText",
            JobType::RecaptchaV2 => "RecaptchaV2",
            JobType::RecaptchaV3 => "RecaptchaV3",
            JobType::Turnstile => "Turnstile",
            JobType::HCaptcha => "HCaptcha",
            JobType::FunCaptcha => "FunCaptcha",
            JobType::GeeTest => "GeeTest",
            JobType::DataDome => "DataDome",
            JobType::Unknown => "Unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolverJob {
    pub task_id: String,
    pub job_type: JobType,
    pub sitekey: Option<String>,
    pub page_url: Option<String>,
    pub image_data: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub struct JobQueue {
    queue: Mutex<VecDeque<SolverJob>>,
    notify: Notify,
}

impl JobQueue {
    pub fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            notify: Notify::new(),
        }
    }

    pub fn push(&self, job: SolverJob) {
        self.queue.lock().unwrap().push_back(job);
        self.notify.notify_one();
    }

    pub fn pop(&self) -> Option<SolverJob> {
        self.queue.lock().unwrap().pop_front()
    }

    pub async fn wait_pop(&self) -> SolverJob {
        loop {
            if let Some(job) = self.pop() {
                return job;
            }
            self.notify.notified().await;
        }
    }

    pub fn len(&self) -> usize {
        self.queue.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.lock().unwrap().is_empty()
    }
}

impl Default for JobQueue {
    fn default() -> Self {
        Self::new()
    }
}
