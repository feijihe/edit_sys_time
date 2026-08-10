use axum::{
    http::StatusCode,
    routing::post,
    Json, Router,
};
use chrono::{DateTime, FixedOffset, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

static SYSTEM_TZ: Lazy<Tz> = Lazy::new(|| detect_system_timezone());

#[derive(Clone, Copy)]
enum TzInput {
    Named(Tz),
    Offset(FixedOffset),
}

#[derive(Deserialize)]
struct SetTimeRequest {
    datetime: String,
    #[serde(default)]
    timezone: Option<String>,
}

#[derive(Deserialize)]
struct SetTimezoneRequest {
    timezone: String,
}

#[derive(Serialize)]
struct ApiResponse {
    success: bool,
    message: String,
}

const BIND_ADDR: &str = "0.0.0.0:3333";

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    info!("Starting edit_sys_time service...");
    info!("System timezone detected: {:?}", *SYSTEM_TZ);

    disable_ntp();

    let app = Router::new()
        .route("/api/time", post(set_time))
        .route("/api/timezone", post(set_timezone));

    let listener = match tokio::net::TcpListener::bind(BIND_ADDR).await {
        Ok(l) => {
            info!("Listening on http://{}", BIND_ADDR);
            l
        }
        Err(e) => {
            error!("Failed to bind to {}: {}", BIND_ADDR, e);
            std::process::exit(1);
        }
    };

    if let Err(e) = axum::serve(listener, app).await {
        error!("Server error: {}", e);
    }
}

#[cfg(target_os = "linux")]
fn disable_ntp() {
    use std::process::Command;

    info!("Disabling automatic time synchronization...");

    let output = Command::new("timedatectl")
        .args(["set-ntp", "false"])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            info!("NTP auto-sync disabled successfully");
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            error!("Failed to disable NTP: {}", stderr.trim());
        }
        Err(e) => {
            error!("Failed to run timedatectl: {}", e);
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn disable_ntp() {
    info!("NTP disable skipped (not running on Linux)");
}

async fn set_time(Json(req): Json<SetTimeRequest>) -> (StatusCode, Json<ApiResponse>) {
    let tz_input = match parse_timezone(req.timezone.as_deref()) {
        Ok(t) => t,
        Err(msg) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse {
                    success: false,
                    message: msg,
                }),
            );
        }
    };

    let ts = match parse_datetime(&req.datetime, tz_input) {
        Ok(t) => t,
        Err(msg) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse {
                    success: false,
                    message: msg,
                }),
            );
        }
    };

    match set_system_time(ts) {
        Ok(()) => {
            let utc_dt = Utc
                .timestamp_opt(ts, 0)
                .single()
                .map(|d| d.to_rfc3339())
                .unwrap_or_else(|| ts.to_string());
            info!("System time set to {} UTC (timestamp: {})", utc_dt, ts);

            if let Err(e) = sync_hwclock() {
                error!("Failed to sync hardware clock: {}", e);
            }

            (
                StatusCode::OK,
                Json(ApiResponse {
                    success: true,
                    message: format!("Time set to {} UTC", utc_dt),
                }),
            )
        }
        Err(msg) => {
            error!("Failed to set system time: {}", msg);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse {
                    success: false,
                    message: msg,
                }),
            )
        }
    }
}

async fn set_timezone(Json(req): Json<SetTimezoneRequest>) -> (StatusCode, Json<ApiResponse>) {
    let trimmed = req.timezone.trim();

    if trimmed.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse {
                success: false,
                message: "timezone is required".to_string(),
            }),
        );
    }

    let tz_name = match resolve_timezone_name(trimmed) {
        Ok(n) => n,
        Err(msg) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse { success: false, message: msg }),
            );
        }
    };

    match change_system_timezone(&tz_name) {
        Ok(()) => {
            info!("System timezone changed to {}", tz_name);
            (
                StatusCode::OK,
                Json(ApiResponse {
                    success: true,
                    message: format!("Timezone changed to {}", tz_name),
                }),
            )
        }
        Err(msg) => {
            error!("Failed to change timezone: {}", msg);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse {
                    success: false,
                    message: msg,
                }),
            )
        }
    }
}

fn resolve_timezone_name(input: &str) -> Result<String, String> {
    if let Ok(_tz) = input.parse::<Tz>() {
        return Ok(input.to_string());
    }

    if let Some(offset_secs) = parse_offset_str(input) {
        if offset_secs % 3600 == 0 {
            if let Some(name) = etc_gmt_name(offset_secs) {
                return Ok(name);
            }
        }
    }

    let upper = input.to_uppercase();
    if upper.starts_with("UTC") {
        let rest = upper[3..].trim();
        if let Some(offset_secs) = parse_offset_utc(rest) {
            if offset_secs % 3600 == 0 {
                if let Some(name) = etc_gmt_name(offset_secs) {
                    return Ok(name);
                }
            }
        }
    }

    Err(format!(
        "Unknown timezone: '{}'. Use IANA name (e.g. Asia/Shanghai, UTC) or offset (e.g. +08:00, UTC+08:00)",
        input
    ))
}

fn etc_gmt_name(offset_secs: i32) -> Option<String> {
    let abs_hours = offset_secs.unsigned_abs() / 3600;
    let name = if offset_secs >= 0 {
        format!("Etc/GMT-{}", abs_hours)
    } else {
        format!("Etc/GMT+{}", abs_hours)
    };
    if name.parse::<Tz>().is_ok() {
        Some(name)
    } else {
        None
    }
}

fn parse_offset_utc(s: &str) -> Option<i32> {
    let s = s.trim();
    if s.is_empty() {
        return Some(0);
    }
    let sign = if s.starts_with('+') {
        1
    } else if s.starts_with('-') {
        -1
    } else {
        if let Ok(n) = s.parse::<i32>() {
            return Some(n * 3600);
        }
        return None;
    };
    let body = &s[1..];
    let parts: Vec<&str> = body.split(':').collect();
    if parts.len() == 2 {
        let h: i32 = parts[0].parse().ok()?;
        let m: i32 = parts[1].parse().ok()?;
        if h > 14 || m > 59 {
            return None;
        }
        return Some(sign * (h * 3600 + m * 60));
    }
    if parts.len() == 1 {
        let h: i32 = parts[0].parse().ok()?;
        if h > 14 {
            return None;
        }
        return Some(sign * (h * 3600));
    }
    None
}

fn parse_timezone(raw: Option<&str>) -> Result<Option<TzInput>, String> {
    let Some(raw) = raw else {
        return Ok(None);
    };

    let trimmed = raw.trim();

    if trimmed.is_empty() {
        return Ok(None);
    }

    if let Ok(tz) = trimmed.parse::<Tz>() {
        return Ok(Some(TzInput::Named(tz)));
    }

    if let Some(offset_secs) = parse_offset_str(trimmed) {
        if let Some(offset) = FixedOffset::east_opt(offset_secs) {
            return Ok(Some(TzInput::Offset(offset)));
        }
    }

    Err(format!(
        "Unknown timezone: '{}'. Use IANA name (e.g. Asia/Shanghai, America/New_York) or offset (e.g. +08:00, -05:00)",
        trimmed
    ))
}

fn parse_offset_str(s: &str) -> Option<i32> {
    let s = s.trim();
    if s.len() < 2 {
        return None;
    }
    let sign = if s.starts_with('+') {
        1
    } else if s.starts_with('-') {
        -1
    } else {
        return None;
    };
    let body = &s[1..];
    let parts: Vec<&str> = body.split(':').collect();
    if parts.len() == 2 {
        let hours: i32 = parts[0].parse().ok()?;
        let minutes: i32 = parts[1].parse().ok()?;
        if hours > 14 || minutes > 59 {
            return None;
        }
        return Some(sign * (hours * 3600 + minutes * 60));
    }
    if parts.len() == 1 && parts[0].len() == 4 {
        let hours: i32 = parts[0][..2].parse().ok()?;
        let minutes: i32 = parts[0][2..].parse().ok()?;
        if hours > 14 || minutes > 59 {
            return None;
        }
        return Some(sign * (hours * 3600 + minutes * 60));
    }
    None
}

fn parse_datetime(s: &str, tz_input: Option<TzInput>) -> Result<i64, String> {
    if let Ok(ts) = s.parse::<i64>() {
        return Ok(ts);
    }

    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.timestamp());
    }

    let formats = [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S",
        "%Y/%m/%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M",
    ];

    for fmt in &formats {
        if let Ok(naive) = NaiveDateTime::parse_from_str(s, fmt) {
            return naive_to_utc(naive, tz_input);
        }
    }

    Err(format!(
        "Cannot parse datetime: '{}'. Supported formats:\n  - Unix timestamp\n  - RFC 3339 with timezone\n  - Naive datetime (treated in specified/detected timezone)",
        s
    ))
}

fn naive_to_utc(naive: NaiveDateTime, tz_input: Option<TzInput>) -> Result<i64, String> {
    match tz_input {
        Some(TzInput::Named(tz)) => {
            naive
                .and_local_timezone(tz)
                .single()
                .map(|dt| dt.with_timezone(&Utc).timestamp())
                .ok_or_else(|| {
                    format!(
                        "Ambiguous or invalid local time '{}' in timezone {:?} (possible DST gap)",
                        naive, tz
                    )
                })
        }
        Some(TzInput::Offset(offset)) => {
            naive
                .and_local_timezone(offset)
                .single()
                .map(|dt| dt.with_timezone(&Utc).timestamp())
                .ok_or_else(|| {
                    format!(
                        "Invalid local time '{}' with offset {}",
                        naive, offset
                    )
                })
        }
        None => {
            naive
                .and_local_timezone(*SYSTEM_TZ)
                .single()
                .map(|dt| dt.with_timezone(&Utc).timestamp())
                .ok_or_else(|| {
                    format!(
                        "Ambiguous or invalid local time '{}' in system timezone {:?}",
                        naive, *SYSTEM_TZ
                    )
                })
        }
    }
}

#[cfg(target_os = "linux")]
fn detect_system_timezone() -> Tz {
    if let Ok(tz) = std::fs::read_to_string("/etc/timezone") {
        let trimmed = tz.trim();
        if let Ok(parsed) = trimmed.parse::<Tz>() {
            return parsed;
        }
    }

    if let Ok(path) = std::fs::read_link("/etc/localtime") {
        let path_str = path.to_string_lossy();
        for part in path_str.rsplit('/') {
            if let Ok(parsed) = part.parse::<Tz>() {
                return parsed;
            }
        }
        if let Some(idx) = path_str.find("zoneinfo/") {
            let tz_name = &path_str[idx + "zoneinfo/".len()..];
            if let Ok(parsed) = tz_name.parse::<Tz>() {
                return parsed;
            }
        }
    }

    if let Ok(tz_env) = std::env::var("TZ") {
        if let Ok(parsed) = tz_env.trim().parse::<Tz>() {
            return parsed;
        }
    }

    warn!("Cannot detect system timezone, falling back to UTC");
    Tz::UTC
}

#[cfg(not(target_os = "linux"))]
fn detect_system_timezone() -> Tz {
    if let Ok(tz_env) = std::env::var("TZ") {
        if let Ok(parsed) = tz_env.trim().parse::<Tz>() {
            return parsed;
        }
    }
    warn!("Cannot detect system timezone, falling back to UTC");
    Tz::UTC
}

#[cfg(target_os = "linux")]
fn change_system_timezone(tz_name: &str) -> Result<(), String> {
    use std::process::Command;

    let output = Command::new("timedatectl")
        .args(["set-timezone", tz_name])
        .output();

    match output {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            Err(format!("timedatectl set-timezone failed: {}", stderr.trim()))
        }
        Err(e) => Err(format!("Failed to run timedatectl: {}", e)),
    }
}

#[cfg(not(target_os = "linux"))]
fn change_system_timezone(_tz_name: &str) -> Result<(), String> {
    Err("Changing system timezone is only supported on Linux".to_string())
}

#[cfg(target_os = "linux")]
fn set_system_time(timestamp: i64) -> Result<(), String> {
    let ts = libc::timespec {
        tv_sec: timestamp,
        tv_nsec: 0,
    };

    let ret = unsafe { libc::clock_settime(libc::CLOCK_REALTIME, &ts) };

    if ret == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

#[cfg(target_os = "linux")]
fn sync_hwclock() -> Result<(), String> {
    use std::process::Command;

    let output = Command::new("hwclock").arg("--systohc").output();

    match output {
        Ok(o) if o.status.success() => {
            info!("Hardware clock synced from system time");
            Ok(())
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            Err(format!("hwclock --systohc failed: {}", stderr.trim()))
        }
        Err(e) => Err(format!("Failed to run hwclock: {}", e)),
    }
}

#[cfg(not(target_os = "linux"))]
fn sync_hwclock() -> Result<(), String> {
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn set_system_time(timestamp: i64) -> Result<(), String> {
    let _ = timestamp;
    Err("Setting system time is only supported on Linux".to_string())
}
