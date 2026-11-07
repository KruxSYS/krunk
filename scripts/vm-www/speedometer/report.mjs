// Krunk VM harness (not part of Speedometer): when a run ends, posts Speedometer's own JSON
// export (the "modern" format of the results page) to the local server as results/latest.json.
// (Speedometer refuses unknown URL parameters, so the harness names the file afterwards.)
const client = globalThis.benchmarkClient;
const finish = client.didFinishLastIteration.bind(client);
client.didFinishLastIteration = (metrics) => {
    finish(metrics);
    fetch("/result", { method: "POST", body: client._formattedJSONResult({ modern: true }) });
};
// Why a run failed: Speedometer only says "produced no duration".
const fail = client.handleError.bind(client);
const note = (what) => fetch("/error", { method: "POST", body: String(what) });
client.handleError = (error) => {
    note(`handleError: ${error && error.stack ? error.stack : error}`);
    fail(error);
};
let iteration = 0;
const suiteDone = client.didRunSuites.bind(client);
client.didRunSuites = (values) => {
    iteration++;
    suiteDone(values);
};
// The suite page's state when a step fails (TodoMVC-WebComponents: how many items exist).
function frameState() {
    const f = document.querySelector("iframe");
    if (!f) return "no iframe";
    try {
        const d = f.contentDocument;
        const app = d.querySelector("todo-app");
        const list = app && app.shadowRoot && app.shadowRoot.querySelector("todo-list");
        const items = list && list.shadowRoot ? list.shadowRoot.querySelectorAll("todo-item").length : -1;
        const text = (d.body ? d.body.innerText : "").replace(/\s+/g, " ").slice(0, 160);
        return `iteration ${iteration} url ${d.location.href} ready ${d.readyState} visibility ${d.visibilityState} items ${items} focus ${d.hasFocus()} title ${JSON.stringify(d.title)} text ${JSON.stringify(text)}`;
    } catch (e) {
        return `frame state: ${e}`;
    }
}
addEventListener("error", (e) => note(`error: ${e.message} @ ${e.filename}:${e.lineno} | ${frameState()}`));
addEventListener("unhandledrejection", (e) => note(`rejection: ${e.reason && e.reason.stack ? e.reason.stack : e.reason}`));
// Errors inside the suite's iframe (they do not reach this window's listeners).
new MutationObserver(() => {
    for (const f of document.querySelectorAll("iframe")) {
        if (f.__krunkHooked) continue;
        f.__krunkHooked = true;
        f.addEventListener("load", () => {
            try {
                f.contentWindow.addEventListener("error", (e) => note(`frame error: ${e.message} @ ${e.filename}:${e.lineno}`));
                f.contentWindow.addEventListener("unhandledrejection", (e) => note(`frame rejection: ${e.reason}`));
            } catch (e) {
                note(`frame hook: ${e}`);
            }
        });
    }
}).observe(document.documentElement, { childList: true, subtree: true });
