//! Best-effort desktop notifications via `org.freedesktop.Notifications`.
//! Failures are logged to stderr (some minimal window managers ship no
//! notification daemon).

use std::collections::HashMap;

use zbus::blocking::Connection;

pub fn notify(summary: &str, body: &str) {
    if let Err(e) = push(summary, body) {
        eprintln!("[swaptray] {summary}: {body} (notification failed: {e})");
    }
}

fn push(summary: &str, body: &str) -> Result<(), zbus::Error> {
    let conn = Connection::session()?;
    let hints: HashMap<&str, &zbus::zvariant::Value<'_>> = HashMap::new();
    conn.call_method(
        Some("org.freedesktop.Notifications"),
        "/org/freedesktop/Notifications",
        Some("org.freedesktop.Notifications"),
        "Notify",
        &(
            "swaptray",
            0u32, // replaces_id
            "",    // app_icon
            summary,
            body,
            Vec::<String>::new(), // actions
            hints,
            -1i32, // expire_timeout: server default
        ),
    )?;
    Ok(())
}
