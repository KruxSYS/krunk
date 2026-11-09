//! The lock popup (Krunk 0.5.0): what the engine reports about the page's connection and
//! certificate (`lb_core::certinfo`), opened from the site icon in the address bar.

use super::Browser;
use lb_core::certinfo;
use lb_core::protocol::{PopupContent, PopupRect};

impl Browser {
    /// The popup's content for the active tab.
    pub(super) fn site_info_content(&self) -> PopupContent {
        let url = self.active_tab().map(|t| t.url.clone()).unwrap_or_default();
        let report = self.active_view().and_then(|v| v.ssl_report());
        let mut info = certinfo::site_info(&url, report.as_ref());
        if !info.origin.is_empty() && !self.active_tab().is_some_and(|t| t.private) {
            info.permissions = self.site_permission_rows(&info.origin);
        }
        PopupContent::SiteInfo { info }
    }

    /// Opens the lock popup under the site icon (`rect`: shell CSS px).
    pub(super) fn show_site_info(&self, rect: PopupRect) {
        let content = self.site_info_content();
        self.show_popup(content, Some(rect));
    }

    /// The popup asks for another height (CSS px of the shell), e.g. "Certificate details"
    /// opened: it keeps its place and grows (or shrinks) within the work area.
    pub(super) fn popup_resize(&self, height_css: f64) {
        if !height_css.is_finite() {
            return;
        }
        let Some(main) = self.window() else { return };
        let scale = main.scale();
        let guard = self.popup_slot(self.request_slot()).borrow();
        let Some(p) = guard.as_ref().filter(|p| p.window.is_visible() && p.pending.is_none()) else { return };
        let r = p.window.rect();
        let work = crate::ffi::window::Window::work_area_at(r.x, r.y);
        let max = (work.y + work.height - r.y).max(40);
        let h = ((height_css.clamp(40.0, 4096.0) * scale).round() as i32).min(max);
        if h == r.height {
            return;
        }
        p.window.resize_height(h);
        if let Some(c) = &p.view {
            c.set_bounds(0, 0, r.width, h);
        }
    }
}
