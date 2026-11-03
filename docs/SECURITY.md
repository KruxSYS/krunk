# LightBrowser security notes

*Draft. The full vault threat model comes with the vault milestone.*

## Trusted UI vs. web content
- The browser chrome (shell), the popup and the built-in pages run on two internal origins under
  the reserved `.invalid` TLD. They are served from bytes embedded in the exe, with a strict CSP.
- The host accepts privileged messages only after `core::protocol::classify` has checked:
  - the channel (which WebView the message arrived on);
  - the exact origin and document;
  - that it came from the top-level frame.
- Each sender has a command allow-list (`Sender::permits`). For example, the new-tab page can't
  reach the vault, and the shell can't list passwords.
- Web pages can only send `ContentMessage`, which is strictly parsed and size-capped.
- Tabs can never load the shell origin. A web page can't navigate a tab to a built-in page on its
  own; only the host can (`core::assets::allow_navigation`).

## Password prompts live outside the page
- The "Save password?" and "Fill saved login?" prompts are drawn in the **popup window**. It is a
  separate top-level window owned by the browser frame, and its document is on the shell origin.
- Page content can't draw over it, style it, read it, or send it input. That rules out
  spoofing the prompt or click-jacking it with overlays or transparent iframes.
- Every prompt carries the `tab_id` and exact origin it was raised for, and the reply echoes both.
  Before filling or saving, the host re-checks that the tab is still the active tab and is still
  on that origin; otherwise the reply is dropped.
- Hiding the popup for any reason (focus loss, Escape, window move or resize, tab switch) counts
  as a dismissal, never a confirmation.
