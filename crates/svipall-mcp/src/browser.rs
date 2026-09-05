//! Chromium tiers (browser / stealth / real / warm), persistent sessions and page actions,
//! all on the vendored CDP client in `svipall-cdp`. Browsers are pooled per (headless, profile, proxy) and closed when idle.

use anyhow::{anyhow, bail, Context, Result};
use futures::StreamExt;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use svipall_cdp::browser::{Browser, BrowserConfig, HeadlessMode};
use svipall_cdp::cdp::browser_protocol::emulation::{
    SetFocusEmulationEnabledParams, SetHardwareConcurrencyOverrideParams, SetLocaleOverrideParams,
    SetTimezoneOverrideParams, SetUserAgentOverrideParams, UserAgentMetadata,
};
use svipall_cdp::cdp::browser_protocol::page::CaptureScreenshotFormat;
use svipall_cdp::cdp::js_protocol::runtime::EventConsoleApiCalled;
use svipall_cdp::handler::viewport::Viewport;
use svipall_cdp::page::{Page, ScreenshotParams};
use svipall_core::{Config, IdentityProfile};
use tokio::sync::Mutex;

/// Init script for stealth/real/warm pages, generated from the process identity.
///
/// Rewritten after measuring the previous version against bot.sannysoft.com, which reported
/// `WebDriver (New): present (failed)` and listed `webdriver`, `hardwareConcurrency` and
/// `deviceMemory` **twice** in the navigator prototype. Both symptoms came from the same mistake:
/// defining properties on the `navigator` *instance*, which leaves the prototype's original
/// accessor in place and produces a visible duplicate.
///
/// So: delete what should not exist, redefine on the prototype where it should, and route every
/// patched function through a `toString` proxy so it still reports `[native code]`.
fn stealth_js(id: &IdentityProfile) -> String {
    format!(
        r#"
(() => {{
  const proto = Object.getPrototypeOf(navigator);
  const nativeToString = Function.prototype.toString;

  // Report [native code] for anything we touch. Without this, a single toString() call on a
  // patched getter gives the whole thing away.
  //
  // Two registries, because one realm is not enough. The WeakSet holds the functions *this* realm
  // installed. `shapes` holds their source text, and that is what closes the hole a same-origin
  // `about:blank` iframe opens: the iframe is a realm of its own, so it runs this script with its
  // own WeakSet, and a page that reaches for `iframe.contentWindow.Function.prototype.toString`
  // and calls it on the *top* realm's accessor gets a proxy that has never seen that object. Every
  // realm builds the same accessors from the same identity, so their source strings match, and
  // recognising the shape works where recognising the object does not. Nothing is stored on the
  // function, so there is nothing on it for a page to enumerate.
  const patched = new WeakSet();
  const shapes = new Set();
  const remember = (fn) => {{
    try {{ patched.add(fn); shapes.add(Reflect.apply(nativeToString, fn, [])); }} catch (e) {{}}
    return fn;
  }};
  Function.prototype.toString = new Proxy(nativeToString, {{
    apply(target, thisArg, args) {{
      let ours = false;
      try {{
        ours = patched.has(thisArg)
          || (typeof thisArg === 'function' && shapes.has(Reflect.apply(target, thisArg, [])));
      }} catch (e) {{}}
      if (ours) return 'function ' + (thisArg.name || '') + '() {{ [native code] }}';
      return Reflect.apply(target, thisArg, args);
    }}
  }});
  patched.add(Function.prototype.toString);

  // The engine names a native accessor "get <prop>"; a function literal is named after whatever it
  // was assigned to, and an object-literal shorthand getter is named "get" or nothing at all. That
  // difference is one `getOwnPropertyDescriptor` away, so every accessor installed here is renamed
  // to what the engine would have called it. `Function.prototype.toString` reads the same name, so
  // the two answers agree.
  const nameAs = (fn, name) => {{
    try {{ Object.defineProperty(fn, 'name', {{ value: name, configurable: true }}); }} catch (e) {{}}
    return fn;
  }};

  const defineOnProto = (key, get) => {{
    try {{
      remember(nameAs(get, 'get ' + key));
      Object.defineProperty(proto, key, {{ get, set: undefined, enumerable: true, configurable: true }});
    }} catch (e) {{}}
  }};

  // `navigator.webdriver` is deliberately left alone. It was removed outright here on the argument
  // that there is then no accessor for the "WebDriver Advanced" probe to find — but every Chrome
  // since 89 carries the property and answers `false`, so a navigator without it is a state no real
  // browser produces, and prototype completeness is one `getOwnPropertyNames` away. `bench tells`
  // measured the unpatched `browser` tier in the same pooled browser reporting
  // `value=false in navigator=true`: `--disable-blink-features=AutomationControlled` in `BASE_ARGS`
  // already delivers the honest answer, and the deletion was the only thing creating a difference.
  // `navigator_webdriver` fails the build if a future Chrome stops delivering it.

  defineOnProto('hardwareConcurrency', () => {hw});
  defineOnProto('deviceMemory', () => {mem});
  // Brave announces itself through navigator.brave while claiming to be Chrome in the UA. That
  // contradiction is worse than anything it hides.
  try {{ delete proto.brave; delete Navigator.prototype.brave; }} catch (e) {{}}

  if (!navigator.plugins || navigator.plugins.length === 0) {{
    const mk = (name, desc, file) => ({{ name, description: desc, filename: file, length: 1 }});
    const plugins = [
      mk('PDF Viewer', 'Portable Document Format', 'internal-pdf-viewer'),
      mk('Chrome PDF Viewer', 'Portable Document Format', 'internal-pdf-viewer'),
      mk('Chromium PDF Viewer', 'Portable Document Format', 'internal-pdf-viewer'),
      mk('Microsoft Edge PDF Viewer', 'Portable Document Format', 'internal-pdf-viewer'),
      mk('WebKit built-in PDF', 'Portable Document Format', 'internal-pdf-viewer'),
    ];
    plugins.item = (i) => plugins[i] || null;
    plugins.namedItem = (n) => plugins.find(p => p.name === n) || null;
    plugins.refresh = () => {{}};
    defineOnProto('plugins', () => plugins);
    const mimes = [{{ type: 'application/pdf', suffixes: 'pdf', description: 'Portable Document Format' }}];
    mimes.item = (i) => mimes[i] || null;
    mimes.namedItem = (n) => mimes.find(m => m.type === n) || null;
    defineOnProto('mimeTypes', () => mimes);
  }}

  // The old stub made chrome.runtime and chrome.webstore throw TypeError on property access, which
  // fingerprinters read as "someone faked window.chrome".
  if (!window.chrome) {{
    window.chrome = {{
      app: {{ isInstalled: false, InstallState: {{ DISABLED: 'disabled', INSTALLED: 'installed', NOT_INSTALLED: 'not_installed' }}, RunningState: {{ CANNOT_RUN: 'cannot_run', READY_TO_RUN: 'ready_to_run', RUNNING: 'running' }} }},
      runtime: {{ OnInstalledReason: {{}}, PlatformOs: {{}}, connect: () => {{}}, sendMessage: () => {{}} }},
      csi: function csi() {{ return {{}}; }},
      loadTimes: function loadTimes() {{ return {{}}; }},
    }};
  }}

  // `navigator.permissions.query` used to be wrapped here so a `notifications` query answered with
  // `Notification.permission`. It was written for an old headless Chrome that answered `prompt`
  // where `Notification.permission` said `denied`. Three things were wrong with it, and `bench
  // tells` measured all three: the answer was an object literal, so
  // `Object.prototype.toString` read `[object Object]` where `[object PermissionStatus]` belongs;
  // `Notification.permission` speaks a different vocabulary whose third value is `default`, which
  // is not a `PermissionState` at all; and it was assigned to the `permissions` *instance*, the
  // same shape `no_duplicate_navigator_getters` forbids elsewhere. The unpatched `browser` tier in
  // the same pooled browser answers correctly on its own, so the wrapper only ever made the three
  // patched tiers worse than plain Chrome. `permission_state_is_valid` fails the build if that
  // stops being true.

  try {{
    const patchGl = (protoObj) => {{
      const gp = protoObj.getParameter;
      const getParameter = function getParameter(p) {{
        if (p === 37445) return {vendor};
        if (p === 37446) return {renderer};
        return gp.apply(this, arguments);
      }};
      remember(getParameter);
      protoObj.getParameter = getParameter;
    }};
    if (window.WebGLRenderingContext) patchGl(WebGLRenderingContext.prototype);
    if (window.WebGL2RenderingContext) patchGl(WebGL2RenderingContext.prototype);
  }} catch (e) {{}}

  // screen.availHeight === screen.height means no taskbar, dock or menu bar — impossible on the
  // desktop OS we claim to be, and exactly what headless reports.
  //
  // Derived from the real screen rather than pinned to a fixed resolution: the host's actual
  // display is a legitimate thing to expose, and hard-coding 1080 here while the browser reports
  // the true height would just trade one contradiction for another.
  const defineOn = (obj, key, value) => {{
    try {{
      const get = nameAs(() => value, 'get ' + key);
      remember(get);
      Object.defineProperty(obj, key, {{ get, configurable: true }});
    }} catch (e) {{}}
  }};
  // Headless reports a fixed 800x600 display, and the launch flags then size the window past it —
  // a window wider than the screen holding it, which no machine produces. When the reported screen
  // is that fiction, the identity's own screen replaces it; when the browser is headful and the
  // number is the host's real display, it is left alone.
  if (screen.width < window.outerWidth || screen.width < 1024) {{
    defineOn(screen, 'width', {screen_w});
    defineOn(screen, 'height', {screen_h});
  }}
  defineOn(screen, 'availWidth', screen.width);
  defineOn(screen, 'availHeight', Math.max(screen.height - {os_chrome}, 1));
  defineOn(screen, 'colorDepth', {depth});
  defineOn(screen, 'pixelDepth', {depth});
  // A window parked off-screen so it stays out of the operator's way still tells the page where it
  // is, and nobody browses at -32000. The announced position is a plausible one on the announced
  // screen; `screenLeft`/`screenTop` are the same numbers under their older names.
  if (window.screenX < 0 || window.screenY < 0
      || window.screenX > screen.width || window.screenY > screen.height) {{
    const px = Math.max(0, Math.min(screen.width - window.outerWidth, 60));
    const py = Math.max(0, Math.min(screen.height - window.outerHeight, 40));
    defineOn(window, 'screenX', px);
    defineOn(window, 'screenLeft', px);
    defineOn(window, 'screenY', py);
    defineOn(window, 'screenTop', py);
  }}
  // Real Chrome has 85-120px of tab strip and address bar above the viewport, never one pixel.
  // Not defined here any more: `launch` sizes the OS window to the viewport plus that chrome, so
  // every realm reads the same honest number and there is nothing to contradict.

  // Deterministic per-identity noise. Noise that changes every load is as identifying as none:
  // the point is to be *a* consistent machine, not a different one each time.
  let seed = {seed} >>> 0;
  const rnd = () => {{ seed ^= seed << 13; seed >>>= 0; seed ^= seed >> 17; seed ^= seed << 5; seed >>>= 0; return seed / 4294967296; }};
  try {{
    const origGetImageData = CanvasRenderingContext2D.prototype.getImageData;
    const getImageData = function getImageData(x, y, w, h) {{
      const data = origGetImageData.apply(this, arguments);
      for (let i = 0; i < data.data.length; i += 997) {{
        data.data[i] = (data.data[i] + ((rnd() * 2) | 0)) & 255;
      }}
      return data;
    }};
    remember(getImageData);
    CanvasRenderingContext2D.prototype.getImageData = getImageData;

    const origToDataURL = HTMLCanvasElement.prototype.toDataURL;
    const toDataURL = function toDataURL() {{
      try {{
        const ctx = this.getContext('2d');
        if (ctx && this.width > 0 && this.height > 0) {{
          const d = origGetImageData.call(ctx, 0, 0, 1, 1);
          d.data[0] = (d.data[0] + ((rnd() * 2) | 0)) & 255;
          ctx.putImageData(d, 0, 0);
        }}
      }} catch (e) {{}}
      return origToDataURL.apply(this, arguments);
    }};
    remember(toDataURL);
    HTMLCanvasElement.prototype.toDataURL = toDataURL;
  }} catch (e) {{}}

  try {{
    const origGetChannelData = AudioBuffer.prototype.getChannelData;
    const getChannelData = function getChannelData() {{
      const out = origGetChannelData.apply(this, arguments);
      for (let i = 0; i < out.length; i += 1000) out[i] += (rnd() - 0.5) * 1e-7;
      return out;
    }};
    remember(getChannelData);
    AudioBuffer.prototype.getChannelData = getChannelData;
  }} catch (e) {{}}

  // Sub-pixel geometry of laid-out text. Font rasterisation differs by machine, so the exact
  // fractional width of a rendered string is a stable identifier — the same idea as canvas, on a
  // surface that needs no canvas. Same seeded noise, so this machine stays one machine.
  try {{
    // Deterministic in the value, not a fresh draw: the same element measured twice has to give
    // the same number. Drawing again per call made `getBoundingClientRect()` unstable across two
    // reads of one element, which no real rasteriser does and which is a far louder signal than
    // the geometry it was meant to blur. Caught by the bench, not by reasoning.
    const jitter = (v) => {{
      if (!v) return v;
      let h = (seed ^ Math.round(v * 1000)) >>> 0;
      h = (h ^ (h << 13)) >>> 0; h = (h ^ (h >>> 17)) >>> 0; h = (h ^ (h << 5)) >>> 0;
      return v + ((h / 4294967296) - 0.5) * 0.0001 * v;
    }};

    // A fresh `DOMRect` rather than accessors bolted onto the one the engine returned. Shadowing
    // `x`, `width` and `height` left `x !== left` and `width !== right - left` on every element —
    // an arithmetic contradiction no rasteriser produces — and put three own properties on an
    // object that has none. Building the rectangle from the jittered numbers keeps all eight
    // sides in agreement, keeps the brand `[object DOMRect]`, and leaves the shape untouched.
    const shift = (r) => {{
      try {{
        return new DOMRect(jitter(r.left), jitter(r.top),
                           jitter(r.right - r.left), jitter(r.bottom - r.top));
      }} catch (e) {{ return r; }}
    }};
    // `getClientRects` has to answer with a `DOMRectList`, which has no constructor. Borrowing its
    // prototype gives an object that reports the right brand, passes `instanceof` and indexes the
    // way the real one does, so the two calls cannot be played against each other.
    const asRectList = (rects) => {{
      try {{
        const list = rects.slice();
        Object.defineProperty(list, 'item', {{
          value: nameAs(function item(i) {{ return list[i] || null; }}, 'item'),
          configurable: true, writable: true,
        }});
        remember(list.item);
        Object.setPrototypeOf(list, DOMRectList.prototype);
        return list;
      }} catch (e) {{ return rects; }}
    }};
    const patchRect = (protoObj, name) => {{
      const orig = protoObj[name];
      if (!orig) return;
      const wrapped = function () {{
        const r = orig.apply(this, arguments);
        if (name === 'getClientRects') return asRectList(Array.from(r, shift));
        return shift(r);
      }};
      remember(nameAs(wrapped, name));
      protoObj[name] = wrapped;
    }};
    patchRect(Element.prototype, 'getBoundingClientRect');
    patchRect(Element.prototype, 'getClientRects');
    patchRect(Range.prototype, 'getBoundingClientRect');
  }} catch (e) {{}}

  // Surfaces that were answering with whatever the real machine had. None of them is damning on
  // its own; two of them disagreeing with the announced identity is.
  try {{
    // A new array on every read, which is what the engine does. One frozen array handed out twice
    // makes `navigator.languages === navigator.languages` true, and identity is the one thing a
    // page can test without knowing what the right answer is.
    const langs = {languages};
    defineOnProto('languages', () => langs.slice());
  }} catch (e) {{}}
  try {{
    // Shadow the properties of the `NetworkInformation` the engine already handed out rather than
    // replacing it with an object literal: `Object.prototype.toString` reads the brand, and
    // `[object Object]` where `[object NetworkInformation]` belongs is a one-line catch.
    const conn = navigator.connection;
    if (conn) {{
      defineOn(conn, 'effectiveType', {eff_type});
      defineOn(conn, 'downlink', {downlink});
      defineOn(conn, 'rtt', {rtt});
      defineOn(conn, 'saveData', false);
    }} else {{
      const stub = {{ effectiveType: {eff_type}, downlink: {downlink}, rtt: {rtt}, saveData: false,
                     onchange: null, type: 'wifi', downlinkMax: Infinity,
                     addEventListener() {{}}, removeEventListener() {{}}, dispatchEvent() {{ return false; }} }};
      defineOnProto('connection', () => stub);
    }}
  }} catch (e) {{}}
  try {{
    // A page that finds *labelled* devices without permission has found a patch, so the labels
    // stay empty exactly as Chrome leaves them.
    const devices = [];
    const add = (kind, n) => {{ for (let i = 0; i < n; i++) devices.push(
      {{ deviceId: i === 0 ? 'default' : String(i), groupId: 'g' + i, kind: kind, label: '',
         toJSON() {{ return this; }} }}); }};
    add('audioinput', {audio_in}); add('audiooutput', {audio_out}); add('videoinput', {video_in});
    if (navigator.mediaDevices) {{
      const enumerateDevices = function enumerateDevices() {{ return Promise.resolve(devices.slice()); }};
      remember(enumerateDevices);
      navigator.mediaDevices.enumerateDevices = enumerateDevices;
    }}
  }} catch (e) {{}}
  try {{
    if (navigator.storage) {{
      const estimate = function estimate() {{
        return Promise.resolve({{ quota: {quota}, usage: 0, usageDetails: {{}} }});
      }};
      remember(estimate);
      navigator.storage.estimate = estimate;
    }}
  }} catch (e) {{}}
  try {{
    // Chrome caps the heap per platform; a limit derived from installed RAM is the tell. Shadowed
    // on the `MemoryInfo` the engine returns, for the same reason as `connection`: replacing the
    // object changes what it is, and the brand is cheaper to read than the numbers.
    // On the prototype, not the instance: `performance.memory` hands out a fresh `MemoryInfo` on
    // every read, so properties shadowed on one of them are gone by the next call and the real
    // numbers come back. The bench caught exactly that.
    if (window.performance && performance.memory) {{
      const mproto = Object.getPrototypeOf(performance.memory);
      defineOn(mproto, 'jsHeapSizeLimit', {heap});
      defineOn(mproto, 'totalJSHeapSize', {heap} / 4);
      defineOn(mproto, 'usedJSHeapSize', {heap} / 8);
    }}
  }} catch (e) {{}}
  try {{
    defineOn(window, 'devicePixelRatio', {dpr});
  }} catch (e) {{}}

}})();
"#,
        hw = id.hardware_concurrency,
        mem = id.device_memory,
        vendor =
            serde_json::to_string(&id.webgl_vendor).unwrap_or_else(|_| "\"Intel Inc.\"".into()),
        renderer = serde_json::to_string(&id.webgl_renderer)
            .unwrap_or_else(|_| "\"Intel Iris OpenGL Engine\"".into()),
        os_chrome = id
            .screen
            .height
            .saturating_sub(id.screen.avail_height)
            .max(1),
        depth = id.screen.color_depth,
        screen_w = id.screen.width,
        screen_h = id.screen.height,
        seed = (id.noise_seed & 0xffff_ffff) as u32,
        languages = serde_json::to_string(&id.language_tags())
            .unwrap_or_else(|_| "[\"en-US\",\"en\"]".into()),
        eff_type =
            serde_json::to_string(id.connection.effective_type).unwrap_or_else(|_| "\"4g\"".into()),
        downlink = id.connection.downlink,
        rtt = id.connection.rtt,
        audio_in = id.media_devices.audio_inputs,
        audio_out = id.media_devices.audio_outputs,
        video_in = id.media_devices.video_inputs,
        quota = id.storage_quota,
        heap = id.js_heap_limit,
        dpr = id.device_pixel_ratio,
    )
}

/// The part of the identity every realm has to agree on, in any realm.
///
/// A worker has no `screen`, no `window.chrome`, no plugins and no canvas of the kind the document
/// script patches — but it does have a `WorkerNavigator`, and these values are exactly the ones a
/// page can compare against the document's in one line. Anything a realm cannot show is
/// deliberately absent rather than stubbed: inventing a surface the realm does not have is its own
/// tell.
///
/// It runs in three places: in every worker, and in the document on the `browser` tier, which has
/// no stealth script of its own. That last one is not stealth, it is arithmetic — `browser` and
/// `stealth` are both headless and share a pooled browser, so the worker script is browser-wide
/// and a document that did not follow it would contradict its own workers.
fn identity_core_js(id: &IdentityProfile) -> String {
    format!(
        r#"
(() => {{
  const proto = Object.getPrototypeOf(navigator);
  // The accessors below are arrow functions, and an arrow function stringifies to its own source.
  // Without this, one `Object.getOwnPropertyDescriptor(Navigator.prototype, 'deviceMemory').get`
  // followed by one `toString` reads `() => 8` — the patch and the value it hides, in two calls,
  // in the document on the `browser` tier and inside every worker on every tier. `shapes` carries
  // the source text rather than the function object so a second realm recognises the first
  // realm's accessors; `stealth_js` explains the reasoning in full.
  const nativeToString = Function.prototype.toString;
  const patched = new WeakSet();
  const shapes = new Set();
  Function.prototype.toString = new Proxy(nativeToString, {{
    apply(target, thisArg, args) {{
      let ours = false;
      try {{
        ours = patched.has(thisArg)
          || (typeof thisArg === 'function' && shapes.has(Reflect.apply(target, thisArg, [])));
      }} catch (e) {{}}
      if (ours) return 'function ' + (thisArg.name || '') + '() {{ [native code] }}';
      return Reflect.apply(target, thisArg, args);
    }}
  }});
  patched.add(Function.prototype.toString);
  const define = (key, get) => {{
    try {{
      patched.add(get);
      shapes.add(Reflect.apply(nativeToString, get, []));
      Object.defineProperty(get, 'name', {{ value: 'get ' + key, configurable: true }});
      Object.defineProperty(proto, key, {{ get, set: undefined, enumerable: true, configurable: true }});
    }} catch (e) {{}}
  }};
  define('hardwareConcurrency', () => {hw});
  define('deviceMemory', () => {mem});
  const langs = {languages};
  define('languages', () => langs.slice());
  define('language', () => langs[0]);
}})();
"#,
        hw = id.hardware_concurrency,
        mem = id.device_memory,
        languages = serde_json::to_string(&id.language_tags())
            .unwrap_or_else(|_| "[\"en-US\",\"en\"]".into()),
    )
}

/// The available area of the screen, for the `browser` tier's document.
///
/// Not stealth, arithmetic — the same argument `identity_core_js` makes. Every tier launches with
/// `Viewport { screen: Some((identity.screen.width, identity.screen.height)), .. }`, which becomes
/// a `setDeviceMetricsOverride`; Blink then reports `availWidth`/`availHeight` equal to that
/// overridden display, so the `browser` tier announces a desktop with no taskbar, dock or menu bar.
/// That is not headless being candid about the host: it is an override we send with the available
/// area forgotten. `svipall_core::coherence` calls the result a violation on every identity, and
/// `bench tells` asserts it at every tier.
///
/// It stops there. No canvas, audio or text-geometry noise, no plugin list, no WebGL spoof and no
/// timezone override reach the `browser` tier — those are what make `stealth` a different tier.
///
/// Document-only, and therefore separate from `identity_core_js`: a worker has no `screen`, and
/// inventing a surface a realm does not have is its own tell.
fn document_geometry_js(id: &IdentityProfile) -> String {
    format!(
        r#"
(() => {{
  const define = (key, value) => {{
    try {{
      const get = () => value;
      Object.defineProperty(get, 'name', {{ value: 'get ' + key, configurable: true }});
      Object.defineProperty(screen, key, {{ get, set: undefined, configurable: true }});
    }} catch (e) {{}}
  }};
  define('availWidth', screen.width);
  define('availHeight', Math.max(screen.height - {os_chrome}, 1));
}})();
"#,
        os_chrome = id
            .screen
            .height
            .saturating_sub(id.screen.avail_height)
            .max(1),
    )
}

/// What each page logged, kept on this side of the protocol.
///
/// This used to be a ring buffer on `window`, under a name that spelled out the product, with the
/// five console methods wrapped around it. Enumerating `window`'s own properties for a name no
/// real browser session produces is the cheapest check a detector runs, and the name was a direct
/// hit. `Runtime.enable` is already sent, so `Runtime.consoleAPICalled` is already arriving:
/// buffering it here costs nothing extra on the wire, patches nothing in the page, and survives a
/// navigation that would have wiped a page-side array.
static CONSOLE: once_cell::sync::Lazy<std::sync::Mutex<HashMap<String, Vec<Value>>>> =
    once_cell::sync::Lazy::new(Default::default);

/// Oldest entries fall off: a page in a loop must not be able to grow this without bound.
const CONSOLE_KEEP: usize = 200;

/// Subscribe to what this page logs. One task per page, ending when the page closes and the event
/// stream with it.
async fn watch_console(page: &Page) {
    let key = page.target_id().inner().clone();
    let Ok(mut events) = page.event_listener::<EventConsoleApiCalled>().await else {
        return;
    };
    tokio::spawn(async move {
        while let Some(ev) = events.next().await {
            // A console argument arrives as a remote object: a primitive carries its value, and
            // anything the protocol would not serialise carries the description DevTools shows.
            let text = ev
                .args
                .iter()
                .map(|a| match (&a.value, &a.description) {
                    (Some(Value::String(s)), _) => s.clone(),
                    (Some(v), _) => v.to_string(),
                    (None, Some(d)) => d.clone(),
                    _ => String::new(),
                })
                .collect::<Vec<_>>()
                .join(" ");
            let level = serde_json::to_value(&ev.r#type)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_else(|| "log".into());
            console_push(
                &key,
                json!({ "level": level, "text": text.chars().take(500).collect::<String>() }),
            );
        }
    });
}

fn console_push(key: &str, entry: Value) {
    if let Ok(mut all) = CONSOLE.lock() {
        let ring = all.entry(key.to_string()).or_default();
        if ring.len() >= CONSOLE_KEEP {
            ring.remove(0);
        }
        ring.push(entry);
    }
}

/// Chromium switches for every tier (replaces the CDP client defaults, which include
/// `--enable-automation`, an obvious bot signal).
///
/// Three switches that used to be here are gone on purpose. `--disable-extensions`,
/// `--disable-default-apps` and `--disable-popup-blocking` are all readable from the page and none
/// of them describes a browser a person uses: real Chrome has extensions enabled, has its default
/// apps, and blocks popups. They were inherited from the "make automation predictable" school of
/// flags, which is the opposite of what this project wants.
const BASE_ARGS: &[&str] = &[
    "--disable-background-networking",
    "--disable-background-timer-throttling",
    "--disable-backgrounding-occluded-windows",
    "--disable-breakpad",
    "--disable-client-side-phishing-detection",
    "--disable-component-extensions-with-background-pages",
    "--disable-dev-shm-usage",
    "--disable-features=TranslateUI,IsolateOrigins,site-per-process,AutomationControlled",
    "--disable-blink-features=AutomationControlled",
    "--disable-hang-monitor",
    "--disable-ipc-flooding-protection",
    "--disable-prompt-on-repost",
    "--disable-renderer-backgrounding",
    "--disable-sync",
    "--disable-infobars",
    "--disable-search-engine-choice-screen",
    "--force-color-profile=srgb",
    "--metrics-recording-only",
    "--no-first-run",
    "--no-default-browser-check",
    "--password-store=basic",
    "--use-mock-keychain",
];

/// How WebRTC is allowed to discover addresses.
///
/// This is the switch that decides whether `web_route` works at all. WebRTC gathers candidates
/// straight from the network interfaces, below the HTTP proxy, so a page that opens a peer
/// connection reads the real address of the machine no matter what proxy the request went through.
/// Without this, routing a domain through another country announces the country it actually came
/// from.
/// Split a proxy URL into the URL Chrome should be launched with and the credentials it cannot
/// read from it.
///
/// `http://user:pass@host:3128` becomes `http://host:3128` plus `{user, pass}`. A URL with no
/// userinfo comes back unchanged and `None`. This is the one thing standing between svipall and
/// the authenticated commercial proxies almost everyone actually has.
pub(crate) fn split_proxy_auth(proxy: &str) -> (String, Option<svipall_cdp::auth::Credentials>) {
    let (scheme, rest) = match proxy.split_once("://") {
        Some((s, r)) => (Some(s), r),
        None => (None, proxy),
    };
    let Some((userinfo, host)) = rest.split_once('@') else {
        return (proxy.to_string(), None);
    };
    let (username, password) = match userinfo.split_once(':') {
        Some((u, p)) => (u.to_string(), p.to_string()),
        None => (userinfo.to_string(), String::new()),
    };
    let clean = match scheme {
        Some(s) => format!("{s}://{host}"),
        None => host.to_string(),
    };
    let creds =
        (!username.is_empty()).then_some(svipall_cdp::auth::Credentials { username, password });
    (clean, creds)
}

fn webrtc_args(has_proxy: bool) -> &'static [&'static str] {
    if has_proxy {
        // Nothing may leave over UDP that is not proxied. Costs peer-to-peer media, which no
        // scraping tier uses, and closes the leak completely.
        &[
            "--force-webrtc-ip-handling-policy=disable_non_proxied_udp",
            "--webrtc-ip-handling-policy=disable_non_proxied_udp",
        ]
    } else {
        // No proxy to hide behind, but private addresses still say more than they should: the
        // shape of a home LAN is a fingerprint. Public interface only, which is also what Chrome
        // does when a user turns on the privacy setting.
        &[
            "--force-webrtc-ip-handling-policy=default_public_interface_only",
            "--webrtc-ip-handling-policy=default_public_interface_only",
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserTier {
    /// Headless, no stealth patches, throwaway context.
    Browser,
    /// Headless + stealth patches, throwaway context.
    Stealth,
    /// Headful (offscreen) + stealth + persistent per-domain profile.
    Real,
    /// `Real` plus patient waiting for challenges to clear.
    Warm,
}

impl BrowserTier {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "browser" => Some(Self::Browser),
            "stealth" => Some(Self::Stealth),
            "real" => Some(Self::Real),
            "warm" => Some(Self::Warm),
            _ => None,
        }
    }
    /// The name this tier goes by everywhere else: the ladder, the request log, the cost table.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Browser => "browser",
            Self::Stealth => "stealth",
            Self::Real => "real",
            Self::Warm => "warm",
        }
    }
    pub fn headless(self) -> bool {
        matches!(self, Self::Browser | Self::Stealth)
    }
    pub fn stealth(self) -> bool {
        !matches!(self, Self::Browser)
    }
}

#[derive(Debug, Clone)]
pub struct PageOpts {
    /// Ask the site for its mobile layout: fewer widgets, less navigation, fewer tokens.
    pub mobile: bool,
    pub tier: BrowserTier,
    pub profile_dir: Option<PathBuf>,
    pub proxy: Option<String>,
    /// Show the window (web_login). Otherwise headful tiers are parked offscreen.
    pub visible: bool,
    /// Wear a different machine from the fleet for this page.
    ///
    /// `None` is the process identity, which is what every ordinary page wears and must keep
    /// wearing: a profile whose hardware changes between visits has identified itself. `Some`
    /// goes with an isolated profile, where nothing is carried in — the machine included.
    pub identity_seed: Option<u64>,
}

pub struct Pooled {
    browser: Mutex<Browser>,
    last_used: Mutex<Instant>,
}

pub struct Session {
    pub id: String,
    pub browser: Arc<Pooled>,
    pub page: Page,
    pub profile_dir: PathBuf,
    pub created: Instant,
    /// The machine this session opened as. Kept so closing it can name the same browser the
    /// pool filed it under; the seed is part of that key.
    pub identity_seed: Option<u64>,
}

/// A cleared page parked for the next fetch of the same domain.
///
/// The `Arc<Pooled>` is held so the browser cannot be dropped while one of its pages is parked in
/// here; the page itself is what the next fetch takes.
pub struct KeptPage {
    browser: Arc<Pooled>,
    page: Page,
}

pub struct BrowserPool {
    exe: Option<PathBuf>,
    browser_major: Option<u16>,
    /// Newest Chrome major seen on this machine, for the staleness advice. See `browser_advice`.
    known_major: Option<u16>,
    identity: IdentityProfile,
    cfg: Config,
    pool: Mutex<HashMap<String, Arc<Pooled>>>,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    /// Cleared pages held open between fetches, so a clearance that lives in the page's runtime is
    /// spent more than once. See `svipall_core::warm`.
    ///
    /// Deliberately **not** `sessions`. Those are the MCP `browser_open` surface: `browser_close`
    /// can close anything in there, and `reap_idle` exempts their browsers — folding these in would
    /// let a caller close a page a fetch is about to use, and would pin Chrome processes open. A
    /// held page shares the pooled browser and never touches its `last_used`, so it can never be
    /// the reason a browser stays alive.
    ///
    /// Lock order everywhere: `kept`, then `pool`.
    kept: Mutex<svipall_core::warm::Kept<KeptPage>>,
}

/// Which browser a path is, because it decides how much it will give us away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Brand {
    /// Downloaded and managed by svipall: known version, no vendor patches.
    Managed,
    Chrome,
    Edge,
    Chromium,
    /// Ships its own anti-fingerprinting, which contradicts the identity we advertise.
    SelfDefending,
}

impl Brand {
    /// Lower sorts first. Measured reason for the ordering: with Brave selected, sannysoft saw
    /// `navigator.brave` and randomised plugin names next to a User-Agent claiming Chrome — a
    /// contradiction no stealth script can undo, because it is the binary talking.
    /// What this brand is called, for a report a person reads.
    pub fn name(self) -> &'static str {
        match self {
            Brand::Managed => "managed",
            Brand::Chrome => "chrome",
            Brand::Edge => "edge",
            Brand::Chromium => "chromium",
            Brand::SelfDefending => "self-defending",
        }
    }

    fn rank(self) -> u8 {
        match self {
            Brand::Managed => 0,
            Brand::Chrome => 1,
            Brand::Edge | Brand::Chromium => 2,
            Brand::SelfDefending => 3,
        }
    }

    /// Public because `doctor` reports the brand: which browser is selected decides how much of
    /// the identity survives contact with a page, and an installer that cannot say so is guessing.
    pub fn of(path: &std::path::Path) -> Brand {
        let p = path.to_string_lossy().to_ascii_lowercase();
        if p.contains(".svipall") {
            Brand::Managed
        } else if p.contains("brave") || p.contains("vivaldi") || p.contains("opera") {
            Brand::SelfDefending
        } else if p.contains("edge") {
            Brand::Edge
        } else if p.contains("chrome") {
            Brand::Chrome
        } else {
            Brand::Chromium
        }
    }
}

/// Where svipall keeps a browser it downloaded itself.
pub fn managed_browser_dir() -> PathBuf {
    svipall_core::config::home_dir().join("browser")
}

/// Executable of the managed Chrome for Testing install, if one is present.
pub fn managed_browser() -> Option<PathBuf> {
    let root = managed_browser_dir().join("cft");
    let mut best: Option<(u16, PathBuf)> = None;
    for entry in std::fs::read_dir(&root).ok()?.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let Some(major) = major_of(&entry.file_name().to_string_lossy()) else {
            continue;
        };
        for rel in [
            "chrome-win64/chrome.exe",
            "chrome-win32/chrome.exe",
            "chrome-linux64/chrome",
            "chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
            "chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
        ] {
            let exe = dir.join(rel);
            if exe.is_file() && best.as_ref().is_none_or(|(m, _)| major > *m) {
                best = Some((major, exe));
            }
        }
    }
    best.map(|(_, p)| p)
}

/// Candidate Chromium binaries, ordered by how faithfully they can carry our identity.
fn candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    // Explicit choice always wins: if someone names a binary, that is the answer.
    for var in [
        "SVIPALL_BROWSER",
        "CHROME_PATH",
        "CHROME_BIN",
        "PUPPETEER_EXECUTABLE_PATH",
    ] {
        if let Ok(p) = std::env::var(var) {
            if !p.trim().is_empty() {
                v.push(PathBuf::from(p));
            }
        }
    }
    if let Some(p) = managed_browser() {
        v.push(p);
    }

    let mut found: Vec<PathBuf> = Vec::new();

    #[cfg(target_os = "windows")]
    {
        // The registry is the authoritative answer on Windows; the fixed paths below are a
        // fallback for installs that did not register.
        for hive in ["HKLM", "HKCU"] {
            for exe in [
                "chrome.exe",
                "msedge.exe",
                "brave.exe",
                "vivaldi.exe",
                "opera.exe",
            ] {
                if let Some(p) = registry_app_path(hive, exe) {
                    found.push(p);
                }
            }
        }
        let roots = [
            std::env::var("ProgramFiles").ok(),
            std::env::var("ProgramFiles(x86)").ok(),
            std::env::var("LOCALAPPDATA").ok(),
            std::env::var("LOCALAPPDATA")
                .ok()
                .map(|p| format!("{p}\\Programs")),
        ];
        for root in roots.iter().flatten() {
            for rel in [
                "Google/Chrome/Application/chrome.exe",
                "Google/Chrome Beta/Application/chrome.exe",
                "Google/Chrome Dev/Application/chrome.exe",
                "Microsoft/Edge/Application/msedge.exe",
                "Microsoft/Edge Beta/Application/msedge.exe",
                "Chromium/Application/chrome.exe",
                "BraveSoftware/Brave-Browser/Application/brave.exe",
                "Vivaldi/Application/vivaldi.exe",
                "Opera/opera.exe",
            ] {
                found.push(PathBuf::from(root).join(rel));
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        for rel in [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Google Chrome Beta.app/Contents/MacOS/Google Chrome Beta",
            "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Arc.app/Contents/MacOS/Arc",
            "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
            "/Applications/Vivaldi.app/Contents/MacOS/Vivaldi",
            "/Applications/Opera.app/Contents/MacOS/Opera",
        ] {
            found.push(PathBuf::from(rel));
            // Per-user installs are common on macOS and were missing entirely.
            if !home.is_empty() {
                found.push(PathBuf::from(format!("{home}{rel}")));
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        for p in [
            "/usr/bin/google-chrome",
            "/usr/bin/google-chrome-stable",
            "/opt/google/chrome/chrome",
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
            "/snap/bin/chromium",
            "/usr/bin/microsoft-edge",
            "/usr/bin/microsoft-edge-stable",
            "/opt/microsoft/msedge/msedge",
            "/usr/bin/brave-browser",
            "/usr/bin/brave-browser-stable",
            "/opt/brave.com/brave/brave-browser",
            "/usr/bin/vivaldi",
            "/opt/vivaldi/vivaldi",
            "/usr/bin/opera",
        ] {
            found.push(PathBuf::from(p));
        }
        // Flatpak wrappers sandbox the process, so --user-data-dir under ~/.svipall lands outside the
        // sandbox and profiles silently do not persist. Kept as a last resort, below everything.
        for base in [
            "/var/lib/flatpak/exports/bin".to_string(),
            format!(
                "{}/.local/share/flatpak/exports/bin",
                std::env::var("HOME").unwrap_or_default()
            ),
        ] {
            for id in [
                "com.google.Chrome",
                "org.chromium.Chromium",
                "com.brave.Browser",
            ] {
                found.push(PathBuf::from(&base).join(id));
            }
        }
    }

    // Stable ordering by fingerprint quality, preserving discovery order within a rank.
    found.sort_by_key(|p| Brand::of(p).rank());
    v.extend(found);
    v
}

#[cfg(target_os = "windows")]
fn registry_app_path(hive: &str, exe: &str) -> Option<PathBuf> {
    let key = format!(r"{hive}\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{exe}");
    let out = std::process::Command::new("reg")
        .args(["query", &key, "/ve"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().find(|l| l.contains("REG_SZ"))?;
    let path = line.split("REG_SZ").nth(1)?.trim();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// First number of a `152.0.7977.75`-shaped string.
/// Public alias for the provisioner, which sorts version directories by the same rule.
pub fn major_of_public(v: &str) -> Option<u16> {
    major_of(v)
}

fn major_of(version: &str) -> Option<u16> {
    let head: String = version
        .trim()
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    head.parse().ok().filter(|v| *v > 0)
}

/// Chrome major of a binary. Prefers the free answer (a version directory beside the executable,
/// which Chrome, Edge, Brave and Vivaldi all keep) over spawning `--version`.
pub fn browser_version(exe: &std::path::Path) -> Option<u16> {
    version_from_sibling_dir(exe)
        .or_else(|| version_from_ancestor_dir(exe))
        .or_else(|| version_from_cli(exe))
}

/// The managed browser lives under `cft/<version>/chrome-win64/chrome.exe`: the version is an
/// ancestor, not a sibling, and `--version` prints nothing on Windows.
///
/// Measured: without this the managed browser reported `Chrome ?`, the identity fell back to a
/// default major, and the user agent named a Chrome five versions older than the engine actually
/// running — a mismatch every vendor's client script can check in one line.
fn version_from_ancestor_dir(exe: &std::path::Path) -> Option<u16> {
    exe.ancestors()
        .skip(1)
        .filter_map(|a| a.file_name())
        .map(|n| n.to_string_lossy().to_string())
        // Only a name that *is* a version. `major_of` reads the first number it finds, and
        // `chrome-win64` has one.
        .filter(|n| n.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .find_map(|n| major_of(&n))
}

/// Chromium keeps `Application\<version>\` beside `chrome.exe`. Reading the directory name is free.
fn version_from_sibling_dir(exe: &std::path::Path) -> Option<u16> {
    let dir = exe.parent()?;
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| major_of(&e.file_name().to_string_lossy()))
        .max()
}

fn version_from_cli(exe: &std::path::Path) -> Option<u16> {
    let out = std::process::Command::new(exe)
        .arg("--version")
        .output()
        .ok()?;
    major_of(&String::from_utf8_lossy(&out.stdout))
}

/// Every usable browser on this machine, best first.
///
/// One scan rather than two: the registry lookups spawn a process each, and both the choice of
/// binary and the survey of what else is installed want the same list.
pub fn detect_all(cfg: &Config) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if !cfg.browser_path.trim().is_empty() {
        let p = PathBuf::from(cfg.browser_path.trim());
        if p.is_file() {
            out.push(p);
        } else {
            tracing::warn!(
                "browser_path {} not found, falling back to auto-detect",
                p.display()
            );
        }
    }
    out.extend(candidates().into_iter().filter(|p| p.is_file()));
    out.dedup();
    out
}

pub fn detect_browser(cfg: &Config) -> Option<PathBuf> {
    detect_all(cfg).into_iter().next()
}

/// The newest Chrome major this machine has any evidence of, read from directory names only.
///
/// Free on purpose: `version_from_cli` would spawn every installed browser at startup to learn
/// something that is only ever used to phrase a warning. A version that can only be had by running
/// the binary is simply not counted.
pub fn newest_installed_major(paths: &[PathBuf]) -> Option<u16> {
    paths
        .iter()
        .filter_map(|p| version_from_sibling_dir(p).or_else(|| version_from_ancestor_dir(p)))
        .max()
}

/// How many Chrome majors behind the newest known build is worth saying something about.
///
/// One is an ordinary rollout: a major takes weeks to reach every machine, so warning at one would
/// fire on a healthy install every four weeks, and advice that always fires is advice nobody reads.
const STALE_MAJORS: u16 = 2;

/// What is wrong with the browser that would be launched, in one sentence, or `None`.
///
/// Two problems live on this machine rather than on the site, both invisible until a page will not
/// open, and both measured here the hard way. A browser that defends its own fingerprint
/// contradicts the Chrome identity every other layer states, and no stealth script can undo it
/// because it is the binary talking. A build left far behind the stable channel announces a Chrome
/// that no longer exists in the wild, which is a one-line check for anyone who cares.
///
/// One sentence, not two: the identity contradiction is the one an update cannot fix, so when both
/// are true it is the one worth spending.
///
/// `best_known` is the newest major this installation has any evidence of — another browser found
/// on this machine, or the stable channel the provisioner last looked at. `None` when there is no
/// evidence, and no evidence can never make something stale.
pub fn browser_advice(
    exe: Option<&std::path::Path>,
    in_use: Option<u16>,
    best_known: Option<u16>,
) -> Option<String> {
    // No browser at all has its own error path with its own instructions.
    let exe = exe?;
    if Brand::of(exe) == Brand::SelfDefending {
        return Some(format!(
            "The browser in use ({}) ships its own anti-fingerprinting, which contradicts the \
             Chrome identity svipall states everywhere else and is detectable on sight. \
             browser_setup(action=\"install\") fetches Chrome for Testing, which the pool prefers \
             once it is there.",
            exe.display()
        ));
    }
    let (in_use, best) = (in_use?, best_known?);
    let behind = best.checked_sub(in_use).filter(|b| *b >= STALE_MAJORS)?;
    Some(format!(
        "The browser in use is Chrome {in_use}, {behind} majors behind the newest this machine \
         knows of ({best}); a user agent naming a Chrome that old is itself a signal. \
         browser_setup(action=\"update\") refreshes the managed install."
    ))
}

fn safe_name(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

pub fn profiles_dir() -> PathBuf {
    svipall_core::profiles::profiles_dir()
}

/// The flags that make the browser resolve names over HTTPS.
///
/// `secure` rather than `automatic`: automatic falls back to plaintext DNS whenever the template
/// fails, which means the leak this exists to close reopens itself on the first hiccup and says
/// nothing. A resolver that is down should look like a network that is down.
///
/// `None` when no template is configured, which is the default.
pub fn doh_args(template: &str) -> Option<[String; 2]> {
    let t = template.trim();
    if t.is_empty() {
        return None;
    }
    // Only https. A template on any other scheme is not DNS over HTTPS, whatever it is.
    if !t.starts_with("https://") {
        tracing::warn!("dns_over_https must be an https URL; ignoring {t}");
        return None;
    }
    Some([
        format!("--dns-over-https-templates={t}"),
        "--dns-over-https-mode=secure".to_string(),
    ])
}

pub fn sessions_dir() -> PathBuf {
    svipall_core::config::home_dir().join("sessions")
}

pub fn named_profile(name: &str) -> PathBuf {
    profiles_dir().join(safe_name(name))
}

impl BrowserPool {
    pub fn new(cfg: Config) -> Self {
        let (keep_max, keep_secs) = (cfg.warm_keep_max, cfg.warm_keep_secs);
        let found = detect_all(&cfg);
        let known_major = newest_installed_major(&found);
        let exe = found.into_iter().next();
        let major = exe.as_deref().and_then(browser_version);
        match &exe {
            Some(p) => {
                tracing::info!(
                    "browser tiers enabled: {} (Chrome {})",
                    p.display(),
                    major.map(|m| m.to_string()).unwrap_or_else(|| "?".into())
                );
                if let Some(advice) = browser_advice(Some(p), major, known_major) {
                    tracing::warn!("{advice}");
                }
            }
            None => tracing::warn!("no Chromium-based browser found; browser tiers disabled (set browser_path in ~/.svipall/config.toml, or call browser_setup)"),
        }
        // Every tier states the same identity. Built once here so the browser and the http tier
        // cannot drift apart.
        let identity = IdentityProfile::resolve(major, &cfg);
        // The isolated world the CDP client evaluates in takes its name from the same seed as the
        // canvas noise, so one identity really does drive everything. Upstream shipped a constant
        // spelling out the automation library, which is free identification for anything that
        // enumerates execution-context and script names. Only the first call counts, which is what
        // we want: the name has to hold still for the life of a browser.
        svipall_cdp::world::seed(identity.noise_seed);
        // A worker is a realm the document's init script never reaches. Handing it the same
        // numbers closes the cheapest cross-realm check there is: eight cores in the document and
        // the host's real thirty-two in a worker is one identity contradicting itself.
        svipall_cdp::worker::set_init_script(identity_core_js(&identity));
        Self {
            exe,
            browser_major: major,
            known_major,
            identity,
            cfg,
            pool: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            kept: Mutex::new(svipall_core::warm::Kept::new(
                keep_max,
                Duration::from_secs(keep_secs),
            )),
        }
    }

    pub fn identity(&self) -> &IdentityProfile {
        &self.identity
    }

    pub fn available(&self) -> bool {
        self.exe.is_some()
    }

    /// Chrome major version of the binary that would be launched, or None when there is none.
    pub fn browser_major(&self) -> Option<u16> {
        self.browser_major
    }

    /// What is wrong with the browser that would be launched, in one sentence, or `None`.
    ///
    /// `latest_stable` is what the provisioner last learned from the release channel, which the
    /// caller reads from the store; without it the advice still has the survey of this machine to
    /// work from, and simply knows less.
    pub fn advice(&self, latest_stable: Option<u16>) -> Option<String> {
        let best = match (self.known_major, latest_stable) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        browser_advice(self.exe.as_deref(), self.browser_major, best)
    }

    pub fn executable(&self) -> Option<String> {
        self.exe.as_ref().map(|p| p.to_string_lossy().to_string())
    }

    pub fn nav_timeout(&self) -> Duration {
        Duration::from_millis(self.cfg.browser_timeout_ms)
    }

    pub fn warm_wait(&self) -> Duration {
        Duration::from_millis(self.cfg.warm_wait_ms)
    }

    fn key(opts: &PageOpts) -> String {
        // The machine and the form factor belong in the key alongside the profile and the exit.
        // The window and the screen are decided when the process starts and cannot be changed per
        // page, so a browser shared between two machines would hand the second one a window that
        // contradicts the screen it claims — and a desktop browser reused for a phone request
        // would leave the viewport disagreeing with the user agent.
        format!(
            "{}|{}|{}|{}|{}|{}",
            if opts.tier.headless() {
                "headless"
            } else {
                "headful"
            },
            opts.profile_dir
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
            opts.proxy.clone().unwrap_or_default(),
            opts.visible,
            opts.mobile,
            opts.identity_seed
                .map(|s| format!("{s:016x}"))
                .unwrap_or_default(),
        )
    }

    /// The window a machine opens with.
    ///
    /// `fleet` draws a window for every machine — a fraction of its screen, never larger than it —
    /// and until now it was drawn and discarded: every browser opened at a constant 1366x768
    /// whatever screen the identity went on to claim. Beyond being one more constant shared by
    /// every visit, it quietly disabled the screen override itself, because `stealth_js` refuses
    /// to claim a screen the real window would not fit inside.
    ///
    /// Only the geometry comes from the identity. The emulation flags stay as they were: turning
    /// on CDP's mobile emulation is a separate decision with its own tells.
    fn window_of(id: &IdentityProfile) -> Viewport {
        Viewport {
            width: id.viewport.width,
            height: id.viewport.height,
            device_scale_factor: Some(id.device_pixel_ratio as f64),
            emulating_mobile: false,
            is_landscape: true,
            has_touch: false,
            // The identity's display, so `screen.width` is not the 800x600 headless default
            // sitting behind the window, and a plausible corner of it, so a window parked
            // off-screen does not report `screenX = -32000`.
            screen: Some((id.screen.width, id.screen.height)),
            position: Some((
                id.screen.width.saturating_sub(id.viewport.width).min(60),
                id.screen.height.saturating_sub(id.viewport.height).min(40),
            )),
        }
    }

    async fn launch(&self, opts: &PageOpts) -> Result<Arc<Pooled>> {
        let exe = self.exe.clone().ok_or_else(|| {
            anyhow!("no Chromium-based browser found (set browser_path in ~/.svipall/config.toml)")
        })?;
        // The machine this process will be, decided here because the window and the screen are
        // launch-time facts. `key` carries the seed for exactly that reason.
        let id = self.identity_for(opts.proxy.as_deref(), opts.mobile, opts.identity_seed);
        let window = Self::window_of(&id);
        let mut b = BrowserConfig::builder()
            .chrome_executable(exe)
            .disable_default_args()
            .args(BASE_ARGS.iter().map(|s| s.to_string()))
            .args(
                webrtc_args(opts.proxy.is_some())
                    .iter()
                    .map(|s| s.to_string()),
            )
            // On the command line as well as over CDP: the override lands after the browser is up,
            // so without these the very first navigation would still go out as the real binary.
            .arg(format!("--user-agent={}", id.user_agent))
            // `--lang` takes language tags, not an `Accept-Language` header: handing it the header
            // put `en;q=0.9` into `navigator.languages`, where a quality value can never appear.
            .arg(format!("--lang={}", id.language_tags().join(",")))
            // The window is the viewport plus the browser UI above it, and the device-metrics
            // override then sizes the viewport back to `window.height`. That makes
            // `outerHeight - innerHeight` the real thing rather than a number defined over the top
            // of it: a spoof only holds in the realm that installed it, and one `about:blank`
            // iframe reads the honest value beside it (`bench tells`, `iframe_realm_agrees`).
            .window_size(window.width, window.height + id.viewport.outer_extra_height)
            .viewport(window)
            .launch_timeout(Duration::from_secs(30))
            .request_timeout(self.nav_timeout() + Duration::from_secs(5));
        b = if opts.tier.headless() {
            b.headless_mode(HeadlessMode::New)
        } else {
            b.with_head()
        };
        if !opts.tier.headless() && !opts.visible {
            b = b.arg("--window-position=-32000,-32000");
        }
        if let Some(dir) = &opts.profile_dir {
            let _ = std::fs::create_dir_all(dir);
            b = b.user_data_dir(dir);
        }
        if let Some(proxy) = &opts.proxy {
            // Credentials go to CDP in `prepare`, not onto the command line: Chrome does not read
            // userinfo from `--proxy-server` (it pops a 407 dialog instead), and the argument
            // vector is visible to every other process on the machine.
            let (clean, _) = split_proxy_auth(proxy);
            b = b.arg(format!("--proxy-server={}", clean));
        } else if let Some(flags) = doh_args(&self.cfg.dns_over_https) {
            // Only without a proxy. A proxy already resolves the name at its own end, and asking
            // the browser to do both is asking it to make a DNS request it does not need.
            for flag in flags {
                b = b.arg(flag);
            }
        }
        let config = b.build().map_err(|e| anyhow!("browser config: {}", e))?;
        let (browser, mut handler) = Browser::launch(config).await.context("launching browser")?;
        tokio::spawn(async move {
            // Newer Chromium builds emit CDP events this protocol version cannot decode; those
            // arrive as Err items but the connection is intact, so keep driving the handler.
            // The stream ends (None) once the browser really goes away.
            while let Some(ev) = handler.next().await {
                if let Err(e) = ev {
                    tracing::debug!("cdp event skipped: {}", e);
                }
            }
        });
        Ok(Arc::new(Pooled {
            browser: Mutex::new(browser),
            last_used: Mutex::new(Instant::now()),
        }))
    }

    async fn get(&self, opts: &PageOpts) -> Result<Arc<Pooled>> {
        let key = Self::key(opts);
        // Hold the pool lock across the launch: two concurrent callers on the same profile
        // directory would otherwise both start Chromium, and the second one fails.
        let mut pool = self.pool.lock().await;
        if let Some(p) = pool.get(&key).cloned() {
            // A crashed or wedged browser never answers; bound the probe so it cannot hang.
            let alive = {
                let b = p.browser.lock().await;
                matches!(
                    tokio::time::timeout(Duration::from_secs(3), b.pages()).await,
                    Ok(Ok(_))
                )
            };
            if alive {
                *p.last_used.lock().await = Instant::now();
                return Ok(p);
            }
            pool.remove(&key);
            let _ = p.browser.lock().await.kill().await;
        }
        let p = self.launch(opts).await?;
        pool.insert(key, p.clone());
        Ok(p)
    }

    /// A fresh page for `opts`, with stealth patches applied when the tier asks for them.
    /// What a held page is filed under: everything that would make one page wrong for another
    /// fetch.
    ///
    /// `Self::key` already carries the profile, the exit, headful/headless, the machine and the
    /// form factor, so `(domain, exit)` is inside it. Two things it cannot know are added here.
    /// `domain`, because a named profile is shared across domains and a clearance is not. And
    /// `text_only`, because blocking heavy resources is page state that survives navigation — a
    /// prose page handed to a fetch that wanted pictures would drop them in silence.
    pub fn kept_key(opts: &PageOpts, domain: &str, text_only: bool) -> String {
        format!("{}|{}|{}", Self::key(opts), domain, text_only)
    }

    /// A page for this fetch: the one parked for this key if there is a live one, otherwise a fresh
    /// one. The `bool` says which, so the caller can report it.
    ///
    /// A reused page is **never** re-prepared. `prepare` installs the identity and stealth script
    /// with `evaluate_on_new_document`, which persists across navigations; calling it twice would
    /// install a second copy of the patches, and two copies are readable from the page. `bench
    /// tells` asserts this offline.
    pub async fn warm_page(&self, opts: &PageOpts, key: &str) -> Result<(Arc<Pooled>, Page, bool)> {
        if let Some(k) = self.kept.lock().await.take(key, Instant::now()) {
            // A browser can die between fetches, and a dead page answers nothing. Bounded, so a
            // hung browser costs three seconds rather than the fetch — the same probe `get` uses.
            match tokio::time::timeout(Duration::from_secs(3), k.page.url()).await {
                Ok(Ok(_)) => return Ok((k.browser, k.page, true)),
                _ => {
                    tracing::debug!("a kept page was no longer alive; opening a fresh one");
                    self.close_page(k.page).await;
                }
            }
        }
        let (pooled, page) = self.page(opts).await?;
        Ok((pooled, page, false))
    }

    /// Park a cleared page for the next fetch of the same domain, closing whatever that displaces.
    pub async fn keep_page(&self, key: &str, browser: Arc<Pooled>, page: Page) {
        let freed = self
            .kept
            .lock()
            .await
            .park(key, KeptPage { browser, page }, Instant::now());
        for k in freed {
            self.close_page(k.page).await;
        }
    }

    /// Let go of every held page whose key the predicate names, and close them.
    pub async fn release_kept(&self, f: impl Fn(&str) -> bool) {
        let gone = self.kept.lock().await.drain_where(f);
        for k in gone {
            self.close_page(k.page).await;
        }
    }

    /// Tell a held page how its fetch went. Two blocks retire it, on the shared rule.
    pub async fn record_kept(&self, key: &str, v: svipall_core::session::Verdict) {
        let retired = self.kept.lock().await.record(key, v);
        if let Some(k) = retired {
            tracing::debug!(key, "a kept page was refused twice; letting it go");
            self.close_page(k.page).await;
        }
    }

    /// Close the pages that have gone unused for too long. Driven by the same sweep that reaps
    /// idle browsers, so there is no second timer.
    pub async fn expire_kept(&self) {
        let gone = self.kept.lock().await.expire(Instant::now());
        for k in gone {
            self.close_page(k.page).await;
        }
    }

    /// What is being held, for the status report.
    pub async fn kept_pages(&self) -> Vec<String> {
        self.kept
            .lock()
            .await
            .keys()
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    pub async fn page(&self, opts: &PageOpts) -> Result<(Arc<Pooled>, Page)> {
        let pooled = self.get(opts).await?;
        let page = {
            pooled
                .browser
                .lock()
                .await
                .new_page("about:blank")
                .await
                .context("new page")?
        };
        self.prepare(
            &page,
            opts.tier,
            opts.proxy.as_deref(),
            opts.mobile,
            opts.identity_seed,
        )
        .await?;
        Ok((pooled, page))
    }

    /// Apply the process identity to a page.
    ///
    /// The order matters: anything CDP can set is set through CDP, because those overrides live
    /// below JavaScript and leave nothing for a script to find. Only what CDP cannot reach —
    /// `screen.availHeight`, window chrome height, canvas and audio noise, the plugin list — is
    /// done in the init script.
    /// The identity to wear on a page, given where its traffic leaves from.
    ///
    /// A proxy with a declared country moves the timezone and the language list with it. Without
    /// that, routing a domain through Germany still announces this machine's timezone, and
    /// comparing the two is the cheapest cross-check a site can run.
    pub fn identity_for(
        &self,
        proxy: Option<&str>,
        mobile: bool,
        seed: Option<u64>,
    ) -> IdentityProfile {
        // A different machine first, then the same country and form-factor rules as always: a
        // rotated identity that forgot where its traffic leaves from would be worse than none.
        let base = match seed {
            Some(seed) => self.identity.clone().as_machine(seed),
            None => self.identity.clone(),
        };
        let id = match proxy.and_then(svipall_core::store::region_for_proxy) {
            Some(region) => base.in_country(region.country),
            None => base,
        };
        if mobile {
            id.as_phone()
        } else {
            id
        }
    }

    async fn prepare(
        &self,
        page: &Page,
        tier: BrowserTier,
        proxy: Option<&str>,
        mobile: bool,
        seed: Option<u64>,
    ) -> Result<()> {
        let owned = self.identity_for(proxy, mobile, seed);
        let id = &owned;

        watch_console(page).await;

        // A headful window parked off-screen is never the foreground window, so the page it holds
        // reports `document.hasFocus() === false` and `:focus-within` never matches — for the whole
        // life of the session. Nobody reads a page they have not clicked into, and a challenge that
        // waits for interaction on an unfocused document waits forever. This tells the renderer to
        // behave as though the window were in front, which is what it would be if the operator were
        // not being kept out of the way.
        let _ = page
            .execute(SetFocusEmulationEnabledParams::new(true))
            .await;

        // userAgentMetadata is what actually populates navigator.userAgentData and the Sec-CH-UA
        // request headers. The previous code set neither, so the browser tiers announced a Chrome
        // version in the UA and a different, un-spoofed one in the client hints.
        // Chrome fills `navigator.languages` from this string by splitting it on commas, quality
        // values and all, so the header form puts `en;q=0.9` into an array that may only hold
        // language tags. The stealth tiers redefine `navigator.languages` and can afford the real
        // header; the `browser` tier has no script to correct it, so it gets the tags.
        let accept_language = if tier.stealth() {
            id.accept_language.clone()
        } else {
            id.language_tags().join(",")
        };
        let mut ua_params = SetUserAgentOverrideParams::builder()
            .user_agent(id.user_agent.clone())
            .accept_language(accept_language)
            .platform(id.platform_js);
        match serde_json::from_value::<UserAgentMetadata>(id.ua_metadata()) {
            Ok(meta) => ua_params = ua_params.user_agent_metadata(meta),
            // Not fatal: the UA string still applies, but client hints would then come from the
            // real binary and contradict it, so say so rather than fail quietly.
            Err(e) => tracing::warn!("userAgentMetadata rejected by this CDP version: {e}"),
        }
        let ua_params = ua_params.build().map_err(|e| anyhow!(e))?;
        page.execute(ua_params).await.context("set user agent")?;

        // A proxy that wants a username gets one, over the protocol, before the first navigation.
        // Without this every browser-tier fetch through a `user:pass` proxy stalls on a 407 the
        // page can never answer.
        if let Some((_, Some(creds))) = proxy.map(split_proxy_auth) {
            page.authenticate(creds)
                .await
                .context("proxy authentication")?;
        }

        if tier.stealth() {
            // Timezone and locale must agree with the exit IP; a proxy in Frankfurt with a New York
            // clock is a contradiction that costs nothing to avoid.
            if let Ok(tz) = SetTimezoneOverrideParams::builder()
                .timezone_id(id.timezone.clone())
                .build()
            {
                let _ = page.execute(tz).await;
            }
            let locale = SetLocaleOverrideParams::builder()
                .locale(id.locale_tag())
                .build();
            let _ = page.execute(locale).await;
            let _ = page
                .execute(SetHardwareConcurrencyOverrideParams::new(
                    id.hardware_concurrency as i64,
                ))
                .await;
            page.evaluate_on_new_document(stealth_js(id))
                .await
                .context("stealth init script")?;
        } else {
            // The `browser` tier carries no stealth script, but it shares a pooled browser with
            // `stealth` and therefore shares its worker init script. Without this, its documents
            // report the host's hardware while its own workers report the identity's.
            page.evaluate_on_new_document(identity_core_js(id))
                .await
                .context("identity init script")?;
            // The same tier launches with the identity's display as a device-metrics override, and
            // Blink then reports the whole of it as available. Correcting the available area is
            // arithmetic on a number this session already chose, not a stealth surface.
            page.evaluate_on_new_document(document_geometry_js(id))
                .await
                .context("document geometry init script")?;
        }
        Ok(())
    }

    /// Navigate and wait for the load event (bounded by the configured timeout). Returns the
    /// main document's HTTP status when Chromium reports it, else 200.
    pub async fn navigate(&self, page: &Page, url: &str) -> Result<u16> {
        page.goto(url).await.context("goto")?;
        let status =
            match tokio::time::timeout(self.nav_timeout(), page.wait_for_navigation_response())
                .await
            {
                Ok(Ok(req)) => req
                    .and_then(|r| r.response.as_ref().map(|resp| resp.status as u16))
                    .unwrap_or(200),
                Ok(Err(_)) => 200,
                Err(_) => bail!(
                    "navigation timed out after {}ms",
                    self.cfg.browser_timeout_ms
                ),
            };
        self.settle(page, Duration::from_millis(1200)).await;
        Ok(status)
    }

    /// Wait for readyState=complete (bounded) plus a short pause for late scripts.
    pub async fn settle(&self, page: &Page, extra: Duration) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            let ready = page
                .evaluate("document.readyState")
                .await
                .ok()
                .and_then(|r| r.value().and_then(|v| v.as_str().map(|s| s == "complete")))
                .unwrap_or(false);
            if ready {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        tokio::time::sleep(extra).await;
    }

    pub async fn content(&self, page: &Page) -> Result<(String, String)> {
        let html = page.content().await.context("page content")?;
        let url = page.url().await.ok().flatten().unwrap_or_default();
        Ok((html, url))
    }

    /// The names of the cookies this page currently holds. **Names only** — a value is a session
    /// secret, and a name is all a vendor sign needs.
    ///
    /// From the jar rather than from `Set-Cookie`: the header map `Network.responseReceived`
    /// reports is the one the renderer sees, and it drops `Set-Cookie` in several cases. The jar is
    /// what actually happened. Best effort — a page that cannot answer simply has no cookies to
    /// report, which is not a reason to fail a fetch.
    pub async fn cookie_names(&self, page: &Page) -> Vec<String> {
        page.get_cookies()
            .await
            .map(|cs| cs.into_iter().map(|c| c.name).collect())
            .unwrap_or_default()
    }

    /// Scroll a page that loads as you go until it stops growing, or the round budget or the
    /// deadline runs out. Every scroll is wheel input through `behavior`; the only script run is
    /// a measurement. Returns the number of rounds and whether the document grew at all.
    ///
    /// A "load more" control is clicked once, when growth has stalled, because a listing that
    /// paginates with a button is the same listing with one extra step.
    pub async fn scroll_until_stable(
        &self,
        page: &Page,
        max_rounds: u32,
        deadline: Instant,
    ) -> Result<(u32, bool)> {
        use svipall_core::growth::{Decision, GrowthWatch};
        const MEASURE: &str = "(() => [document.documentElement.scrollHeight, \
            (document.body ? document.body.innerText.length : 0)])()";
        async fn measure(page: &Page) -> (u64, u64, u64, u64) {
            let v = page.evaluate(MEASURE).await.ok();
            let arr = v
                .as_ref()
                .and_then(|r| r.value())
                .and_then(|v| v.as_array().cloned());
            let at = |i: usize| {
                arr.as_ref()
                    .and_then(|a| a.get(i))
                    .and_then(|x| x.as_u64())
                    .unwrap_or(0)
            };
            (at(0), at(1), at(2), at(3))
        }
        // A reader after the next batch flicks to the bottom, not one screen down: the page
        // loads more when the end comes into view, so each round goes there. Long distances
        // are covered in several wheel bursts (one burst is capped at sixty notches).
        let mut watch = GrowthWatch::new(max_rounds);
        let (h0, t0, _, _) = measure(page).await;
        let mut grew = false;
        loop {
            if Instant::now() >= deadline {
                break;
            }
            let (height, _, y, inner) = measure(page).await;
            let mut remaining = height.saturating_sub(y + inner) as f64 + inner as f64 * 0.5;
            while remaining > 0.0 {
                let burst = remaining.min(5_000.0);
                self.human_scroll(page, burst).await?;
                remaining -= burst;
            }
            // Give the page a moment to fetch and render what the scroll asked for: poll rather
            // than sleep a fixed time, so a fast site is not made slow and a slow one is not
            // mistaken for a finished one.
            let before = measure(page).await;
            let settle_until = Instant::now() + Duration::from_millis(2_500);
            let mut now = before;
            while Instant::now() < settle_until {
                tokio::time::sleep(Duration::from_millis(250)).await;
                now = measure(page).await;
                if now != before {
                    break;
                }
            }
            if now.0 > h0 || now.1 > t0 {
                grew = true;
            }
            match watch.observe(now.0, now.1) {
                Decision::Continue => {}
                Decision::TryLoadMore => {
                    let found = page
                        .evaluate(svipall_core::pagination::LOAD_MORE_JS)
                        .await
                        .ok()
                        .and_then(|r| r.value().and_then(|v| v.as_str().map(str::to_string)))
                        .unwrap_or_default();
                    if found.is_empty() {
                        break;
                    }
                    if self.human_click(page, &found).await.is_err() {
                        break;
                    }
                    self.settle(page, Duration::from_millis(800)).await;
                }
                Decision::Stop => break,
            }
        }
        Ok((watch.rounds(), grew))
    }

    /// Small human-like activity used while waiting for a challenge to clear.
    ///
    /// This used to be two `scrollBy` calls and nothing else. A page that scrolls without a single
    /// preceding `mousemove` is one of the cheapest bot signals there is, so the pointer moves
    /// first; see `behavior`.
    pub async fn nudge(&self, page: &Page) {
        let seed = self.identity.noise_seed;
        let mut cursor = crate::behavior::Cursor::at_page(page);
        // Pointer first: a scroll with no `mousemove` before it is the cheapest tell there is.
        let (x, y) = crate::behavior::aim(200.0, 200.0, 600.0, 300.0, seed);
        let _ = cursor.move_to(page, x, y, seed).await;
        // Real wheel events rather than `window.scrollBy`, which changes the position without
        // anything having scrolled.
        let _ = self.human_scroll(page, 260.0).await;
        let _ = self.human_scroll(page, -120.0).await;
        // A tab that never loses focus and never fires `visibilitychange` in twenty minutes is not
        // one anybody is looking at — but the events used to be dispatched from JavaScript, and a
        // `visibilitychange` with `isTrusted: false` beside a `document.hidden` that never moved is
        // worse than silence: it is a page telling on itself. The protocol drives the real thing,
        // so `document.hidden` actually changes and the event the page receives is the engine's.
        //
        // …and yet nothing here fires one, because of where this is called from.
        //
        // The obvious way to produce a real `visibilitychange` is `Page.setWebLifecycleState`,
        // frozen then active, and that is what this did for one round. `Page.setWebLifecycleState`
        // does what it says: it **stops the page's JavaScript**. This function is called from the
        // warm loop, which runs while a challenge is on screen — so a widget measuring how long a
        // button was held had its own timers frozen underneath it mid-hold. Zillow's press-and-hold
        // went from passing in six seconds to failing at sixty-three, twice, and the benchmark is
        // the only reason that was noticed at all.
        //
        // So the tab stays awake. What remains is the pointer and the wheel above, which are real
        // input and cost the page nothing, plus the focus emulation `prepare` turns on — a window
        // parked off-screen otherwise reports `document.hasFocus() === false` for the life of the
        // session, which is the contradiction actually worth fixing. A forged `visibilitychange`
        // is not an option either: `isTrusted: false` beside a `document.hidden` that never moved
        // is a page telling on itself.
    }

    /// Click a selector the way a hand would: approach along a curve, land off-centre, hold the
    /// button for a human interval. Falls back to the CDP element click when the element has no
    /// box to aim at (display:none, detached, zero-sized).
    pub async fn human_click(&self, page: &Page, selector: &str) -> Result<()> {
        if let Some((x, y, w, h)) = crate::behavior::box_of(page, selector).await {
            let seed = self.identity.noise_seed;
            let (tx, ty) = crate::behavior::aim(x, y, w, h, seed);
            return crate::behavior::Cursor::at_page(page)
                .click_at(page, tx, ty, seed)
                .await;
        }
        let el = page
            .find_element(selector)
            .await
            .context("element not found")?;
        let _ = el.scroll_into_view().await;
        el.click().await?;
        Ok(())
    }

    /// Stop the page loading what will never be read.
    ///
    /// When the caller wants text, images, fonts and stylesheets are pure cost: bandwidth, time,
    /// and memory in a browser pool that is already the tightest resource here. On an image-heavy
    /// page this is most of the bytes.
    ///
    /// Deliberately *not* on by default. A page whose images fail to load renders differently, and
    /// some anti-bot scripts notice, so this is a lever the caller pulls when it knows it only
    /// wants prose.
    pub async fn block_heavy_resources(&self, page: &Page) -> Result<()> {
        use svipall_cdp::cdp::browser_protocol::network::{EnableParams, SetBlockedUrLsParams};
        page.execute(EnableParams::default()).await?;
        page.execute(SetBlockedUrLsParams::new(vec![
            "*.png".into(),
            "*.jpg".into(),
            "*.jpeg".into(),
            "*.gif".into(),
            "*.webp".into(),
            "*.avif".into(),
            "*.svg".into(),
            "*.woff".into(),
            "*.woff2".into(),
            "*.ttf".into(),
            "*.otf".into(),
            "*.mp4".into(),
            "*.webm".into(),
            "*.css".into(),
        ]))
        .await?;
        Ok(())
    }

    /// Scroll with real wheel events, in notches, the way a mouse does.
    pub async fn human_scroll(&self, page: &Page, pixels: f64) -> Result<()> {
        use svipall_cdp::cdp::browser_protocol::input::{
            DispatchMouseEventParams, DispatchMouseEventType,
        };
        let cursor = crate::behavior::Cursor::at_page(page);
        for step in crate::behavior::scrolling(pixels, self.identity.noise_seed) {
            page.execute(
                DispatchMouseEventParams::builder()
                    .r#type(DispatchMouseEventType::MouseWheel)
                    .x(cursor.x)
                    .y(cursor.y)
                    .delta_x(0.0)
                    .delta_y(step.delta_y)
                    .build()
                    .map_err(|e| anyhow!("{e}"))?,
            )
            .await?;
            tokio::time::sleep(Duration::from_millis(step.delay_ms)).await;
        }
        Ok(())
    }

    /// Press a selector and hold it. `WallKind::Hold` challenges measure the hold and reject a
    /// synthetic click, which is why they used to escalate to a human every single time.
    pub async fn press_and_hold(&self, page: &Page, selector: &str, ms: u64) -> Result<()> {
        let (x, y, w, h) = crate::behavior::box_of(page, selector)
            .await
            .ok_or_else(|| anyhow!("no element to hold at {selector}"))?;
        let seed = self.identity.noise_seed;
        let (tx, ty) = crate::behavior::aim(x, y, w, h, seed);
        crate::behavior::Cursor::at_page(page)
            .press_and_hold(page, tx, ty, ms, seed)
            .await
    }

    /// Refuse the advertising and tracking hosts on the list.
    ///
    /// Same mechanism as `block_heavy_resources` and the same caveat: a page whose third parties
    /// all fail loads differently from one where they succeed. That is why this is a lever rather
    /// than a default, and why an empty list is a no-op instead of an error.
    pub async fn block_tracking(
        &self,
        page: &Page,
        list: &svipall_core::blocklist::Blocklist,
    ) -> Result<()> {
        use svipall_cdp::cdp::browser_protocol::network::{EnableParams, SetBlockedUrLsParams};
        let patterns = crate::blocklists::patterns(list);
        if patterns.is_empty() {
            return Ok(());
        }
        page.execute(EnableParams::default()).await?;
        page.execute(SetBlockedUrLsParams::new(patterns)).await?;
        Ok(())
    }

    /// Take the consent overlay off the page.
    ///
    /// Not by clicking "accept": that would be answering on the operator's behalf, and it is not
    /// needed. The article is already in the DOM under the banner.
    pub async fn hide_consent(&self, page: &Page) -> usize {
        page.evaluate(svipall_core::blocklist::HIDE_CONSENT_JS)
            .await
            .ok()
            .and_then(|r| r.value().and_then(serde_json::Value::as_u64))
            .unwrap_or(0) as usize
    }

    /// Put text into a field at a speed a person types at.
    ///
    /// `type_str` on the whole string arrives as one instantaneous burst, which is a stronger
    /// signal than anything the answer itself carries: nobody types eight characters in the same
    /// millisecond. See `behavior::typing` for where the cadence comes from.
    pub async fn human_type(&self, page: &Page, selector: &str, text: &str) -> Result<()> {
        let el = page
            .find_element(selector)
            .await
            .context("no element to type into")?;
        el.click().await?;
        for key in crate::behavior::typing(text, self.identity.noise_seed) {
            tokio::time::sleep(Duration::from_millis(key.delay_ms)).await;
            el.type_str(&key.ch.to_string()).await?;
        }
        Ok(())
    }

    pub async fn screenshot(&self, page: &Page, full_page: bool) -> Result<Vec<u8>> {
        let params = ScreenshotParams::builder()
            .format(CaptureScreenshotFormat::Png)
            .full_page(full_page)
            .build();
        page.screenshot(params).await.context("screenshot")
    }

    pub async fn close_page(&self, page: Page) {
        if let Ok(mut all) = CONSOLE.lock() {
            all.remove(page.target_id().inner());
        }
        crate::behavior::Cursor::forget(&page);
        let _ = page.close().await;
    }

    /// Execute action objects in order. Never aborts the batch: each action reports ok/error.
    pub async fn run_actions(&self, page: &Page, actions: &[Value]) -> Vec<Value> {
        // References are stamped onto elements by the snapshot walk, and that happened on a page
        // that has since been closed. Re-run the walk here so `ref` names the same element it named
        // when the model saw it. Once per batch, and only when a reference is actually used.
        if actions.iter().any(|a| a.get("ref").is_some()) {
            let _ = page.evaluate(crate::snapshot::WALK_JS).await;
        }
        let mut out = Vec::with_capacity(actions.len());
        for a in actions {
            let kind = a
                .get("do")
                .or(a.get("action"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let t0 = Instant::now();
            let res = self.run_action(page, &kind, a).await;
            let ms = t0.elapsed().as_millis();
            out.push(match res {
                Ok(v) => json!({"do": kind, "ok": true, "ms": ms, "value": v}),
                Err(e) => json!({"do": kind, "ok": false, "ms": ms, "error": e.to_string()}),
            });
        }
        out
    }

    async fn run_action(&self, page: &Page, kind: &str, a: &Value) -> Result<Value> {
        let s = |k: &str| a.get(k).and_then(|v| v.as_str()).map(|v| v.to_string());
        // A reference from `web_snapshot` is accepted anywhere a selector is, which is the point of
        // handing them out: the model names what it saw instead of inventing CSS and hoping.
        let selector = s("ref")
            .and_then(|r| crate::snapshot::selector_for(&r))
            .or_else(|| s("selector"));
        match kind {
            "click" => {
                let sel = selector.ok_or_else(|| anyhow!("click needs selector"))?;
                if let Ok(el) = page.find_element(sel.as_str()).await {
                    let _ = el.scroll_into_view().await;
                }
                self.human_click(page, &sel).await?;
                tokio::time::sleep(Duration::from_millis(300)).await;
                Ok(Value::Null)
            }
            "verify" => {
                // Closing the loop after an action without paying for the whole page again. The
                // alternative is returning the document and asking the model to read it, which is
                // thousands of tokens to answer a yes/no question.
                let sel = selector.ok_or_else(|| anyhow!("verify needs selector or ref"))?;
                let expect = s("value");
                let js = format!(
                    r#"(() => {{
                        const el = document.querySelector({sel:?});
                        if (!el) return {{found: false}};
                        const r = el.getBoundingClientRect();
                        const st = getComputedStyle(el);
                        return {{
                            found: true,
                            visible: r.width > 0 && r.height > 0 && st.display !== 'none'
                                     && st.visibility !== 'hidden',
                            text: (el.innerText || el.textContent || '').trim().slice(0, 200),
                            value: ('value' in el && el.value != null) ? String(el.value) : null,
                        }};
                    }})()"#
                );
                let mut v = page
                    .evaluate(js.as_str())
                    .await?
                    .into_value::<Value>()
                    .unwrap_or(Value::Null);
                if let (Some(want), Some(obj)) = (expect, v.as_object_mut()) {
                    let got = obj
                        .get("value")
                        .and_then(Value::as_str)
                        .or_else(|| obj.get("text").and_then(Value::as_str))
                        .unwrap_or_default();
                    obj.insert("matches".into(), json!(got == want));
                }
                Ok(v)
            }
            "console" => {
                // What the page logged. Useful for diagnosing a page that will not render, and for
                // the errors a challenge script emits when it decides it does not like the visitor.
                // Collected from `Runtime.consoleAPICalled`, so the page carries no logger of ours.
                let key = page.target_id().inner().clone();
                let logged = CONSOLE
                    .lock()
                    .ok()
                    .and_then(|all| all.get(&key).cloned())
                    .unwrap_or_default();
                let tail = logged.iter().rev().take(100).rev().cloned().collect();
                Ok(Value::Array(tail))
            }
            "hold" | "press_and_hold" => {
                let sel = selector.ok_or_else(|| anyhow!("hold needs selector"))?;
                let ms = a.get("ms").and_then(|v| v.as_u64()).unwrap_or(2_000);
                self.press_and_hold(page, &sel, ms).await?;
                Ok(Value::Null)
            }
            "hover" => {
                let sel = selector.ok_or_else(|| anyhow!("hover needs selector"))?;
                page.find_element(sel.as_str()).await.context("element not found")?.hover().await?;
                Ok(Value::Null)
            }
            "type" | "fill" => {
                let sel = selector.ok_or_else(|| anyhow!("type needs selector"))?;
                // `${NAME}` is resolved here, on the way to the browser, so a password reaches the
                // page without ever having been in the tool call, the transcript or the model's
                // context. See `crate::secrets`.
                let text = crate::secrets::expand(
                    &s("text").or(s("value")).unwrap_or_default(),
                    &crate::secrets::load(),
                );
                let el = page.find_element(sel.as_str()).await.context("element not found")?;
                el.click().await?;
                if kind == "fill" {
                    let _ = el.press_key("Control+a").await;
                    let _ = el.press_key("Backspace").await;
                }
                // One `type_str` puts the whole string in at once, which is not a speed anyone
                // types at. Emit the keys with a human cadence instead; see `behavior::typing`.
                for key in crate::behavior::typing(&text, self.identity.noise_seed) {
                    tokio::time::sleep(Duration::from_millis(key.delay_ms)).await;
                    el.type_str(&key.ch.to_string()).await?;
                }
                Ok(Value::Null)
            }
            "press" => {
                let key = s("key").ok_or_else(|| anyhow!("press needs key"))?;
                let target = selector.unwrap_or_else(|| "body".to_string());
                page.find_element(target.as_str()).await.context("element not found")?.press_key(&key).await?;
                tokio::time::sleep(Duration::from_millis(300)).await;
                Ok(Value::Null)
            }
            "select" => {
                let sel = selector.ok_or_else(|| anyhow!("select needs selector"))?;
                let value = s("value").or(s("text")).unwrap_or_default();
                let js = format!(
                    "(() => {{ const el = document.querySelector({}); if (!el) return 'not found'; el.value = {}; el.dispatchEvent(new Event('input', {{bubbles:true}})); el.dispatchEvent(new Event('change', {{bubbles:true}})); return 'ok'; }})()",
                    serde_json::to_string(&sel)?, serde_json::to_string(&value)?
                );
                let r = page.evaluate(js.as_str()).await?;
                Ok(r.value().cloned().unwrap_or(Value::Null))
            }
            "scroll" => {
                if a.get("until").and_then(|v| v.as_str()) == Some("stable") {
                    let rounds = a
                        .get("rounds")
                        .and_then(|v| v.as_u64())
                        .map(|r| r as u32)
                        .unwrap_or(svipall_core::growth::DEFAULT_MAX_ROUNDS);
                    let deadline = Instant::now() + Duration::from_secs(90);
                    let (done, grew) = self.scroll_until_stable(page, rounds, deadline).await?;
                    return Ok(serde_json::json!({"rounds": done, "grew": grew}));
                }
                let px = a.get("pixels").and_then(|v| v.as_i64()).unwrap_or(600);
                // A wheel dispatches real events in notches. `window.scrollBy` moves the page
                // without a single one, so the position changes and nothing was scrolled — which
                // is trivially distinguished from a person.
                self.human_scroll(page, px as f64).await?;
                Ok(Value::Null)
            }
            "wait" => {
                if let Some(sel) = selector {
                    let ms = a.get("ms").and_then(|v| v.as_u64()).unwrap_or(10_000);
                    let deadline = Instant::now() + Duration::from_millis(ms);
                    loop {
                        if page.find_element(sel.as_str()).await.is_ok() {
                            return Ok(json!("found"));
                        }
                        if Instant::now() > deadline {
                            bail!("selector {} not found within {}ms", sel, ms);
                        }
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    }
                }
                let ms = a.get("ms").and_then(|v| v.as_u64()).unwrap_or(1000).min(60_000);
                tokio::time::sleep(Duration::from_millis(ms)).await;
                Ok(Value::Null)
            }
            "eval" | "evaluate" => {
                let script = s("script").or(s("js")).ok_or_else(|| anyhow!("eval needs script"))?;
                let r = page.evaluate(script.as_str()).await?;
                Ok(r.value().cloned().unwrap_or(Value::Null))
            }
            "goto" | "navigate" => {
                let url = s("url").ok_or_else(|| anyhow!("goto needs url"))?;
                let status = self.navigate(page, &url).await?;
                Ok(json!({"status": status}))
            }
            "screenshot" => {
                let png = self.screenshot(page, a.get("full_page").and_then(|v| v.as_bool()).unwrap_or(false)).await?;
                let path = save_png(&page.url().await.ok().flatten().unwrap_or_default(), &png)?;
                Ok(json!({"path": path, "bytes": png.len()}))
            }
            other => bail!("unknown action '{}' (click|hover|type|fill|press|select|scroll|wait|eval|goto|screenshot)", other),
        }
    }

    // ---- persistent sessions --------------------------------------------------------------

    pub async fn open_session(
        &self,
        profile: Option<&str>,
        proxy: Option<String>,
        visible: bool,
    ) -> Result<Arc<Session>> {
        let id = uuid::Uuid::new_v4().to_string()[..12].to_string();
        let profile_dir = match profile {
            Some(name) => named_profile(name),
            None => sessions_dir().join(&id),
        };
        let identity_seed =
            svipall_core::profiles::seed_for_profile(Some(&profile_dir), profile.unwrap_or(&id));
        let opts = PageOpts {
            identity_seed,
            mobile: false,
            tier: BrowserTier::Real,
            profile_dir: Some(profile_dir.clone()),
            proxy,
            visible,
        };
        let (pooled, page) = self.page(&opts).await?;
        let session = Arc::new(Session {
            id: id.clone(),
            browser: pooled,
            page,
            profile_dir,
            created: Instant::now(),
            identity_seed,
        });
        self.sessions.lock().await.insert(id, session.clone());
        Ok(session)
    }

    pub async fn session(&self, id: &str) -> Option<Arc<Session>> {
        self.sessions.lock().await.get(id).cloned()
    }

    pub async fn close_session(&self, id: &str) -> Result<()> {
        let Some(s) = self.sessions.lock().await.remove(id) else {
            bail!("unknown session_id {}", id)
        };
        let _ = s.page.clone().close().await;
        // Session browsers are dedicated (unique profile dir), so close the browser too.
        let key = Self::key(&PageOpts {
            mobile: false,
            tier: BrowserTier::Real,
            profile_dir: Some(s.profile_dir.clone()),
            proxy: None,
            visible: false,
            identity_seed: s.identity_seed,
        });
        let dedicated = self.pool.lock().await.remove(&key);
        if let Some(p) = dedicated {
            let _ = p.browser.lock().await.close().await;
        }
        if s.profile_dir.starts_with(sessions_dir()) {
            let _ = std::fs::remove_dir_all(&s.profile_dir);
        }
        Ok(())
    }

    pub async fn session_ids(&self) -> Vec<String> {
        self.sessions.lock().await.keys().cloned().collect()
    }

    // ---- login (visible window) -------------------------------------------------------------

    /// Open `url` in a visible window on `profile_dir` and return once the user closes the
    /// window or `timeout` elapses. Cookies live in the profile afterwards.
    pub async fn login(&self, url: &str, profile_dir: PathBuf, timeout: Duration) -> Result<bool> {
        // The same machine the profile wears everywhere else: signing in as one visitor and
        // returning as another is the contradiction this whole path exists to avoid.
        let identity_seed =
            svipall_core::profiles::identity_seed_for(Some(&profile_dir), url, None);
        let opts = PageOpts {
            mobile: false,
            tier: BrowserTier::Real,
            profile_dir: Some(profile_dir),
            proxy: None,
            visible: true,
            identity_seed,
        };
        let pooled = self.launch(&opts).await?;
        let page = { pooled.browser.lock().await.new_page("about:blank").await? };
        self.prepare(
            &page,
            BrowserTier::Real,
            opts.proxy.as_deref(),
            opts.mobile,
            opts.identity_seed,
        )
        .await?;
        let _ = page.goto(url).await;
        let _ = page.bring_to_front().await;
        let deadline = Instant::now() + timeout;
        let mut closed_by_user = false;
        while Instant::now() < deadline {
            tokio::time::sleep(Duration::from_secs(2)).await;
            match pooled.browser.lock().await.pages().await {
                Ok(pages) if pages.is_empty() => {
                    closed_by_user = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => {
                    closed_by_user = true;
                    break;
                }
            }
        }
        if !closed_by_user {
            let _ = pooled.browser.lock().await.close().await;
        }
        Ok(closed_by_user)
    }

    // ---- housekeeping ------------------------------------------------------------------

    pub async fn open_browsers(&self) -> usize {
        self.pool.lock().await.len()
    }

    /// Close pooled browsers idle beyond `browser_idle_secs`; session browsers are exempt.
    pub async fn reap_idle(&self) {
        // Held pages first: one that has aged out must be closed before its browser is judged idle,
        // or the sweep would look at a browser that is only busy holding something already dead.
        self.expire_kept().await;
        let idle = Duration::from_secs(self.cfg.browser_idle_secs);
        let session_keys: Vec<String> = self
            .sessions
            .lock()
            .await
            .values()
            .map(|s| {
                Self::key(&PageOpts {
                    mobile: false,
                    tier: BrowserTier::Real,
                    profile_dir: Some(s.profile_dir.clone()),
                    proxy: None,
                    visible: false,
                    identity_seed: None,
                })
            })
            .collect();
        let mut stale = Vec::new();
        {
            let pool = self.pool.lock().await;
            for (k, p) in pool.iter() {
                if session_keys.contains(k) {
                    continue;
                }
                if p.last_used.lock().await.elapsed() > idle {
                    stale.push(k.clone());
                }
            }
        }
        for k in stale {
            // The held page goes with the browser that serves it — `kept_key` is prefixed by the
            // browser key, so this is the browser's own pages and nobody else's.
            self.release_kept(|key| key.starts_with(&k)).await;
            if let Some(p) = self.pool.lock().await.remove(&k) {
                let _ = p.browser.lock().await.close().await;
                tracing::info!("closed idle browser {}", k);
            }
        }
    }

    /// Close whatever browser holds this profile, then delete the profile.
    ///
    /// For a profile a wall has learned to refuse: carrying it into the next fetch earns the same
    /// refusal. The browser has to go first — on Windows the directory is locked while it runs —
    /// and the lock is released a moment after the process is, hence the short retry.
    pub async fn retire_profile(&self, dir: &std::path::Path) -> bool {
        let needle = dir.to_string_lossy().to_string();
        let keys: Vec<String> = self
            .pool
            .lock()
            .await
            .keys()
            .filter(|k| k.split('|').nth(1) == Some(needle.as_str()))
            .cloned()
            .collect();
        for k in keys {
            // A page held on a profile the wall remembers is exactly the page not to reuse.
            self.release_kept(|key| key.starts_with(&k)).await;
            if let Some(p) = self.pool.lock().await.remove(&k) {
                let _ = p.browser.lock().await.close().await;
                tracing::info!("closed browser on a retired profile {}", k);
            }
        }
        for _ in 0..8 {
            if !dir.exists() || std::fs::remove_dir_all(dir).is_ok() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        false
    }

    pub async fn shutdown(&self) {
        self.release_kept(|_| true).await;
        for (_, p) in self.pool.lock().await.drain() {
            let _ = p.browser.lock().await.close().await;
        }
    }
}

pub fn save_png(url: &str, png: &[u8]) -> Result<String> {
    let dir = svipall_core::profiles::screenshots_dir();
    std::fs::create_dir_all(&dir)?;
    let domain = svipall_core::domain_from_url(url);
    let name = format!(
        "{}-{}.png",
        if domain.is_empty() {
            "page".to_string()
        } else {
            safe_name(&domain)
        },
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    );
    let path = dir.join(name);
    std::fs::write(&path, png)?;
    Ok(path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Flags that no browser a person drives would ever carry, and which a page can read.
    ///
    /// Test-only data: the production list is `BASE_ARGS`, and this is what it must never grow
    /// back into. Adding one of these is the kind of change that looks harmless in a diff and
    /// quietly undoes a tier.
    #[test]
    fn resolving_over_https_is_all_or_nothing() {
        // `automatic` falls back to plaintext DNS the moment the template fails, which reopens the
        // leak this exists to close and says nothing about it.
        let args = doh_args("https://dns.example/dns-query").expect("configured");
        assert!(
            args[0].contains("https://dns.example/dns-query"),
            "{args:?}"
        );
        assert_eq!(args[1], "--dns-over-https-mode=secure");
    }

    #[test]
    fn no_template_means_the_browser_is_left_alone() {
        assert!(doh_args("").is_none());
        assert!(doh_args("   ").is_none());
    }

    #[test]
    fn a_template_that_is_not_https_is_refused_rather_than_passed_through() {
        // Sending DNS queries in the clear to a resolver named in a config that says "over HTTPS"
        // is worse than not configuring it at all.
        assert!(doh_args("http://dns.example/dns-query").is_none());
        assert!(doh_args("8.8.8.8").is_none());
    }

    /// The flags a page can read must never describe a driver. This list is the regression guard:
    /// each one was either never there or was removed on purpose, and a diff that adds one back
    /// looks perfectly harmless.
    const FORBIDDEN_ARGS: &[&str] = &[
        "--enable-automation",
        "--disable-extensions",
        "--disable-default-apps",
        "--disable-popup-blocking",
        "--disable-component-update",
        "--headless",
    ];

    fn opts_with(identity_seed: Option<u64>, mobile: bool) -> PageOpts {
        PageOpts {
            mobile,
            tier: BrowserTier::Stealth,
            profile_dir: None,
            proxy: None,
            visible: false,
            identity_seed,
        }
    }

    /// The window is fixed when the browser process starts, so two machines cannot share one.
    /// Without this the cheap tiers — which keep no profile directory — would run every domain in
    /// a single process: one window, one screen, and a different machine claimed per page.
    #[test]
    fn two_machines_never_share_a_browser() {
        assert_ne!(
            BrowserPool::key(&opts_with(Some(1), false)),
            BrowserPool::key(&opts_with(Some(2), false)),
            "two seeds keyed to the same browser"
        );
        assert_ne!(
            BrowserPool::key(&opts_with(Some(1), false)),
            BrowserPool::key(&opts_with(Some(1), true)),
            "a phone and a desktop keyed to the same browser"
        );
        assert_eq!(
            BrowserPool::key(&opts_with(Some(7), false)),
            BrowserPool::key(&opts_with(Some(7), false)),
        );
    }

    /// The window that opens is the window the machine was drawn with.
    ///
    /// `fleet` draws a plausible window for every machine and it used to be thrown away: the
    /// browser launched at a constant 1366x768 whatever screen the identity claimed. That is not
    /// only one more constant across every visit — it silently disabled the screen override for
    /// any machine narrower than the window, because `stealth_js` refuses to claim a screen the
    /// real window would not fit inside.
    #[test]
    fn the_window_is_the_machine_that_was_drawn() {
        let base = IdentityProfile::for_major(140, svipall_core::Os::Windows);
        for seed in 0..500u64 {
            let id = base.clone().as_machine(seed.wrapping_mul(0x9E37_79B9));
            let w = BrowserPool::window_of(&id);
            assert_eq!(w.width, id.viewport.width, "window is not the drawn window");
            assert_eq!(w.height, id.viewport.height);
            assert_eq!(w.device_scale_factor, Some(id.device_pixel_ratio as f64));
            assert_eq!(w.screen, Some((id.screen.width, id.screen.height)));
            // The two conditions under which the screen override gives up (browser.rs, stealth_js):
            // a screen narrower than the real window, or narrower than 1024.
            assert!(
                id.screen.width >= w.width && id.screen.width >= 1024,
                "screen {} would disable its own override behind a {}px window",
                id.screen.width,
                w.width
            );
        }
    }
    #[test]
    fn no_launch_flag_announces_automation() {
        for bad in FORBIDDEN_ARGS {
            assert!(
                !BASE_ARGS.contains(bad),
                "{bad} is readable from the page and no human's browser carries it"
            );
        }
    }

    /// The init scripts must leave nothing on `window` that spells out what is driving the browser.
    /// Enumerating own properties for a name no real session produces is the cheapest check a
    /// detector runs, and a ring buffer called `__svipall_console` was a direct hit — it is now
    /// collected from `Runtime.consoleAPICalled` instead, on this side of the protocol.
    /// `svipall-bench tells` asserts the same thing against a live browser; this catches it at
    /// compile-time speed, where the string is written.
    #[test]
    fn no_init_script_leaves_a_named_global_behind() {
        let id = IdentityProfile::for_major(140, svipall_core::Os::Windows);
        for (what, js) in [
            ("stealth", stealth_js(&id)),
            ("identity", identity_core_js(&id)),
        ] {
            let lower = js.to_lowercase();
            assert!(
                !lower.contains("svipall"),
                "the {what} script names the product, which is free identification"
            );
            for assignment in ["window.__", "self.__", "globalthis.__"] {
                assert!(
                    !lower.contains(assignment),
                    "the {what} script defines {assignment}… on the global object"
                );
            }
        }
    }

    /// `navigator.languages` holds language tags. The `Accept-Language` header holds quality values
    /// too, and the two were the same string: `en;q=0.9` reached an array that may only ever hold
    /// tags. Both scripts read the tags now.
    #[test]
    fn init_scripts_carry_language_tags_not_a_header() {
        let mut id = IdentityProfile::for_major(140, svipall_core::Os::Windows);
        id.accept_language = "en-US,en;q=0.9".into();
        for js in [stealth_js(&id), identity_core_js(&id)] {
            assert!(
                js.contains(r#"["en-US","en"]"#),
                "language tags are missing"
            );
            assert!(
                !js.contains("q=0.9"),
                "a quality value reached a language list"
            );
        }
    }

    #[test]
    fn proxy_credentials_are_split_off_for_cdp_and_never_reach_the_command_line() {
        let (clean, creds) = split_proxy_auth("http://user:pa55@gate.example:3128");
        assert_eq!(clean, "http://gate.example:3128");
        let c = creds.expect("credentials");
        assert_eq!(c.username, "user");
        assert_eq!(c.password, "pa55");
        // The launch argument Chrome actually sees carries no secret.
        assert!(!format!("--proxy-server={clean}").contains("pa55"));

        // A user with no password, and socks.
        let (clean, creds) = split_proxy_auth("socks5://only@127.0.0.1:1080");
        assert_eq!(clean, "socks5://127.0.0.1:1080");
        assert_eq!(creds.unwrap().username, "only");

        // No userinfo: unchanged, and nothing to authenticate.
        let (clean, creds) = split_proxy_auth("http://plain.example:8080");
        assert_eq!(clean, "http://plain.example:8080");
        assert!(creds.is_none());
        // A bare host:port with no scheme.
        let (clean, creds) = split_proxy_auth("u:p@1.2.3.4:9");
        assert_eq!(clean, "1.2.3.4:9");
        assert_eq!(creds.unwrap().password, "p");
    }

    #[test]
    fn the_switch_that_makes_web_route_work_is_always_set() {
        // WebRTC gathers candidates from the network interfaces, underneath the HTTP proxy. With
        // no policy, routing a domain through another country still announces the real address.
        for proxied in [true, false] {
            let args = webrtc_args(proxied);
            assert!(
                args.iter().any(|a| a.contains("webrtc-ip-handling-policy")),
                "no WebRTC policy for proxied={proxied}"
            );
        }
        assert!(
            webrtc_args(true)
                .iter()
                .any(|a| a.contains("disable_non_proxied_udp")),
            "behind a proxy nothing may leave over unproxied UDP"
        );
        assert!(
            webrtc_args(false)
                .iter()
                .any(|a| a.contains("default_public_interface_only")),
            "without a proxy, private addresses still describe the LAN"
        );
    }

    #[test]
    fn the_managed_browser_reports_the_version_it_actually_is() {
        // Its version is an ancestor directory, not a sibling, and `--version` prints nothing on
        // Windows. Reporting `?` here made the user agent name a Chrome five versions older than
        // the engine running, which is one line for a vendor's script to check.
        let exe = std::path::Path::new(
            "C:/Users/x/.svipall/browser/cft/152.0.7977.75/chrome-win64/chrome.exe",
        );
        assert_eq!(version_from_ancestor_dir(exe), Some(152));
        assert_eq!(
            version_from_ancestor_dir(std::path::Path::new("C:/Program Files/Chrome/chrome.exe")),
            None,
            "a path with no version in it must not invent one"
        );
    }

    #[test]
    fn the_stealth_patches_are_still_present_in_the_base_flags() {
        // The other half of the same story: these two have to stay.
        assert!(BASE_ARGS
            .iter()
            .any(|a| a.contains("--disable-blink-features=AutomationControlled")));
        assert!(BASE_ARGS
            .iter()
            .any(|a| a.contains("AutomationControlled") && a.starts_with("--disable-features")));
    }

    #[test]
    fn brands_are_ranked_by_how_faithfully_they_carry_our_identity() {
        assert!(Brand::Managed.rank() < Brand::Chrome.rank());
        assert!(Brand::Chrome.rank() < Brand::Edge.rank());
        // The measured reason this ordering exists: Brave was picked over Chrome and exposed
        // navigator.brave alongside a Chrome User-Agent.
        assert!(Brand::Edge.rank() < Brand::SelfDefending.rank());
    }

    #[test]
    fn brand_is_recognised_from_the_path() {
        assert_eq!(
            Brand::of(std::path::Path::new(
                r"C:\Program Files\Google\Chrome\Application\chrome.exe"
            )),
            Brand::Chrome
        );
        assert_eq!(
            Brand::of(std::path::Path::new(
                r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe"
            )),
            Brand::SelfDefending
        );
        assert_eq!(
            Brand::of(std::path::Path::new("/usr/bin/vivaldi")),
            Brand::SelfDefending
        );
        assert_eq!(
            Brand::of(std::path::Path::new("/usr/bin/microsoft-edge-stable")),
            Brand::Edge
        );
        assert_eq!(
            Brand::of(std::path::Path::new("/usr/bin/chromium")),
            Brand::Chromium
        );
    }

    #[test]
    fn a_machine_with_no_browser_has_nothing_to_advise() {
        // The "no browser at all" case has its own error path with its own instructions; repeating
        // it here would put two different sentences on the same problem.
        assert_eq!(browser_advice(None, None, Some(158)), None);
    }

    #[test]
    fn a_current_browser_is_left_alone() {
        let exe = PathBuf::from("/usr/bin/google-chrome");
        assert_eq!(browser_advice(Some(&exe), Some(158), Some(158)), None);
    }

    #[test]
    fn a_self_defending_browser_is_named_in_the_advice() {
        let exe = PathBuf::from("/usr/bin/brave-browser");
        let advice = browser_advice(Some(&exe), Some(158), Some(158)).expect("advice");
        assert!(
            advice.contains("brave-browser"),
            "names the binary: {advice}"
        );
        assert!(advice.contains("anti-fingerprinting"), "says why: {advice}");
        assert!(
            advice.contains("browser_setup"),
            "says how to fix it: {advice}"
        );
    }

    #[test]
    fn a_browser_two_majors_behind_is_reported() {
        let exe = PathBuf::from("/usr/bin/google-chrome");
        let advice = browser_advice(Some(&exe), Some(156), Some(158)).expect("advice");
        assert!(advice.contains("156"), "names what is running: {advice}");
        assert!(advice.contains("158"), "names what it is behind: {advice}");
        assert!(
            advice.contains("browser_setup"),
            "says how to fix it: {advice}"
        );
    }

    #[test]
    fn one_major_behind_is_an_ordinary_rollout_and_stays_quiet() {
        // Chrome takes weeks to reach every machine. Warning at one major behind would fire on a
        // perfectly ordinary install every four weeks, and advice that always fires is ignored.
        let exe = PathBuf::from("/usr/bin/google-chrome");
        assert_eq!(browser_advice(Some(&exe), Some(157), Some(158)), None);
    }

    #[test]
    fn a_browser_newer_than_anything_known_is_not_reported() {
        let exe = PathBuf::from("/usr/bin/google-chrome");
        assert_eq!(browser_advice(Some(&exe), Some(160), Some(158)), None);
    }

    #[test]
    fn an_unknown_version_cannot_be_called_stale() {
        let exe = PathBuf::from("/usr/bin/google-chrome");
        assert_eq!(browser_advice(Some(&exe), None, Some(158)), None);
        assert_eq!(browser_advice(Some(&exe), Some(150), None), None);
    }

    #[test]
    fn the_self_defending_problem_outranks_the_stale_one() {
        // Both are true of the same binary here. The identity contradiction is the one no update
        // fixes, so it is the sentence worth spending.
        let exe = PathBuf::from("/usr/bin/brave-browser");
        let advice = browser_advice(Some(&exe), Some(150), Some(158)).expect("advice");
        assert!(advice.contains("anti-fingerprinting"), "{advice}");
        assert!(
            !advice.contains("behind"),
            "one problem, one sentence: {advice}"
        );
    }

    #[test]
    fn a_self_defending_browser_never_outranks_chrome() {
        let mut paths = [
            PathBuf::from("/usr/bin/brave-browser"),
            PathBuf::from("/usr/bin/google-chrome"),
            PathBuf::from("/usr/bin/vivaldi"),
            PathBuf::from("/usr/bin/microsoft-edge"),
        ];
        paths.sort_by_key(|p| Brand::of(p).rank());
        assert_eq!(paths[0], PathBuf::from("/usr/bin/google-chrome"));
        assert_eq!(
            Brand::of(&paths[3]),
            Brand::SelfDefending,
            "self-defending browsers must sort last"
        );
    }

    #[test]
    fn version_is_read_from_a_sibling_directory_without_running_anything() {
        let tmp = std::env::temp_dir().join(format!("svipall-ver-{}", std::process::id()));
        let app = tmp.join("Application");
        std::fs::create_dir_all(app.join("152.0.7977.75")).unwrap();
        std::fs::create_dir_all(app.join("151.0.1.2")).unwrap();
        let exe = app.join("chrome.exe");
        std::fs::write(&exe, b"not really an exe").unwrap();
        assert_eq!(
            version_from_sibling_dir(&exe),
            Some(152),
            "should pick the newest version directory"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn major_of_handles_the_shapes_chrome_reports() {
        assert_eq!(major_of("152.0.7977.75"), Some(152));
        assert_eq!(major_of("Google Chrome 147.0.0.0 "), Some(147));
        assert_eq!(major_of("Chromium 120.0.6099.109"), Some(120));
        assert_eq!(major_of("no digits here"), None);
        assert_eq!(major_of("0.1.2"), None, "zero is not a real major");
    }
}
