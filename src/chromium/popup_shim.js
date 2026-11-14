// Runs in an extension's popup before its own scripts. Chromium's tabs API can't see the
// app's tabs (they are embedded pages, not tabs of a Chrome window), so the popup is told
// here which tab it was opened over, as Chrome tells a popup opened from its toolbar.
(() => {
  const over = __TAB__;
  if (!over || !globalThis.chrome || !chrome.tabs) return;
  const tab = {
    id: over.id, index: 0, windowId: over.windowId, active: true, highlighted: true, selected: true,
    pinned: false, incognito: false, audible: false, discarded: false, frozen: false,
    autoDiscardable: true, groupId: -1, splitViewId: -1, mutedInfo: { muted: false },
    status: "complete", url: over.url, title: over.title, favIconUrl: "",
    width: over.width, height: over.height, lastAccessed: Date.now(),
  };
  // The API answers through a callback when given one, and with a promise otherwise.
  const answer = (value, callback) => {
    if (typeof callback === "function") { queueMicrotask(() => callback(value)); return undefined; }
    return Promise.resolve(value);
  };
  const ask = (request) => console.log("rune-popup:" + JSON.stringify(request));
  const ours = (id) => id === undefined || id === null || id === tab.id;
  const tabs = chrome.tabs;
  const native = { get: tabs.get && tabs.get.bind(tabs), reload: tabs.reload && tabs.reload.bind(tabs), update: tabs.update && tabs.update.bind(tabs) };

  tabs.query = (info, callback) => answer(info && info.active === false ? [] : [{ ...tab }], callback);
  tabs.get = (id, callback) => (id === tab.id ? answer({ ...tab }, callback) : native.get(id, callback));
  tabs.reload = (...args) => {
    const callback = typeof args[args.length - 1] === "function" ? args.pop() : undefined;
    const [id, properties] = typeof args[0] === "object" && args[0] !== null ? [undefined, args[0]] : args;
    if (!ours(id)) return native.reload(id, properties, callback);
    ask({ do: "reload", hard: !!(properties && properties.bypassCache) });
    return answer(undefined, callback);
  };
  tabs.update = (...args) => {
    const callback = typeof args[args.length - 1] === "function" ? args.pop() : undefined;
    const [id, properties] = typeof args[0] === "object" && args[0] !== null ? [undefined, args[0]] : args;
    if (!ours(id)) return native.update(id, properties, callback);
    if (properties && typeof properties.url === "string") ask({ do: "go", url: properties.url });
    return answer({ ...tab }, callback);
  };
  if (chrome.windows) {
    const here = (options) => ({
      id: over.windowId, focused: true, type: "normal", state: "normal", incognito: false, alwaysOnTop: false,
      ...(options && options.populate ? { tabs: [{ ...tab }] } : {}),
    });
    const current = (...args) => {
      const callback = typeof args[args.length - 1] === "function" ? args.pop() : undefined;
      return answer(here(args[0]), callback);
    };
    chrome.windows.getCurrent = current;
    chrome.windows.getLastFocused = current;
  }
})();
