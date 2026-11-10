//! The page context menu's image items (0.12.2; the rules are in `lb_core::image_menu`).
//! "Save image as…" downloads the image through its tab (cookies, private profile) and always
//! asks where to save it; a `data:` image is written directly (no download list entry).
//! "Copy image" fetches it the same way and puts a DIB and a PNG on the clipboard.

use super::Browser;
use crate::ffi::cef::fetch::{self, Fetched};
use crate::ffi::dialogs;
use crate::log::{info, warning};
use lb_core::image_menu::{self, ImageAction};
use lb_core::protocol::Event;
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;

/// How long a "Save image as…" waits for its download to start.
const SAVE_AS_WINDOW_MS: i64 = 60_000;
/// How long "Copy image" waits for the image.
const COPY_TIMEOUT_MS: i64 = 30_000;

impl Browser {
    pub(super) fn image_action(&self, tab: TabId, action: ImageAction, url: String) {
        info!("tab {tab}: {}", action.label());
        match action {
            ImageAction::OpenInNewTab => self.open_tab_after(tab, &url),
            ImageAction::CopyAddress => {
                if let Some(w) = self.window() {
                    if !crate::ffi::secure::copy_text(w.hwnd(), &url) {
                        self.image_toast(tab, "The image address could not be copied");
                    }
                }
            }
            ImageAction::Save => self.save_image_as(tab, url),
            ImageAction::Copy => self.copy_image(tab, url),
        }
    }

    fn save_image_as(&self, tab: TabId, url: String) {
        if let Some((mime, bytes)) = image_menu::data_url(&url) {
            let Some(w) = self.window() else { return };
            let name = image_menu::file_name("", &url, &mime);
            let folder = self.download_folder();
            if let Some(path) =
                dialogs::save_file(w.hwnd(), "Save image as", &name, ("All files", "*.*"), folder.as_deref())
            {
                if let Err(e) = std::fs::write(&path, &bytes) {
                    warning!("save image: {e}");
                    self.image_toast(tab, "The image could not be saved");
                }
            }
            return;
        }
        let Some(view) = self.tab_view(tab) else { return };
        let now = now_ms();
        {
            let mut pending = self.save_image_as.borrow_mut();
            pending.retain(|(_, at)| now - at < SAVE_AS_WINDOW_MS);
            pending.push((url.clone(), now));
        }
        view.start_download(&url);
    }

    /// Was the download of `url` (or of `original_url`, before redirects) asked for by
    /// "Save image as…"? Takes the request.
    pub(super) fn take_save_image_as(&self, url: &str, original_url: &str) -> bool {
        let now = now_ms();
        let mut pending = self.save_image_as.borrow_mut();
        pending.retain(|(_, at)| now - at < SAVE_AS_WINDOW_MS);
        match pending.iter().position(|(u, _)| u == url || u == original_url) {
            Some(i) => {
                pending.remove(i);
                true
            }
            None => false,
        }
    }

    fn copy_image(&self, tab: TabId, url: String) {
        if let Some((_, bytes)) = image_menu::data_url(&url) {
            return self.copy_image_bytes(tab, bytes);
        }
        if !image_menu::fetchable(&url) {
            return self.image_toast(tab, "This image can't be copied (it exists only inside the page)");
        }
        let private = self.tab(tab).is_some_and(|t| t.private);
        let opts = fetch::Opts { private, credentials: true, cache_only: false };
        let weak = self.me.clone();
        fetch::get_with(&url, image_menu::MAX_COPY_BYTES, COPY_TIMEOUT_MS, opts, move |r| {
            let Some(b) = weak.upgrade() else { return };
            match r {
                Ok(Fetched { status, body, .. })
                    if (200..300).contains(&status) || (status == 0 && !body.is_empty()) =>
                {
                    b.copy_image_bytes(tab, body)
                }
                Ok(Fetched { status, .. }) => {
                    warning!("copy image: HTTP {status}");
                    b.image_toast(tab, &format!("The image could not be copied (the server answered {status})"));
                }
                Err(e) => {
                    warning!("copy image: {e:?}");
                    b.image_toast(tab, "The image could not be copied (it could not be loaded)");
                }
            }
        });
    }

    fn copy_image_bytes(&self, tab: TabId, bytes: Vec<u8>) {
        self.spawn_job(
            "copy-image",
            move || crate::ffi::image::for_clipboard(&bytes),
            move |b, r| match r {
                Ok(img) => {
                    let ok = b.window().is_some_and(|w| crate::ffi::secure::copy_image(w.hwnd(), &img.dib, &img.png));
                    if ok {
                        info!("copy image: on the clipboard ({} bytes as PNG)", img.png.len());
                    } else {
                        b.image_toast(tab, "The image could not be put on the clipboard");
                    }
                }
                Err(e) => {
                    warning!("copy image: {e}");
                    b.image_toast(tab, &format!("The image could not be copied: {e}"));
                }
            },
        );
    }

    fn image_toast(&self, tab: TabId, message: &str) {
        if let Some(win) = self.window_of(tab).or_else(|| self.cur_id()) {
            self.post_shell_to(win, &Event::Toast { message: message.to_string() });
        }
    }
}
