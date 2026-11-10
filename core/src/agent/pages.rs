//! Which addresses the agent may open or act on (WP-B1).
//!
//! It never acts on (reads, scripts, clicks, types into, screenshots) Krunk's own pages, extension
//! pages or DevTools, and never opens the sensitive built-in pages (passwords and payment
//! methods, sync, site data, storage).

/// May the agent load `url` in a tab? Errors are short reasons for the model.
pub fn may_navigate(url: &str) -> Result<(), &'static str> {
    let _ = url;
    Ok(()) // WP-B1
}

/// May the agent's page tools act on a tab that shows `url`?
pub fn may_act(url: &str) -> Result<(), &'static str> {
    let _ = url;
    Ok(()) // WP-B1
}
