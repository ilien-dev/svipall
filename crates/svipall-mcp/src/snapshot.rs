//! The page as a structure an agent can act on, instead of prose it can only read.
//!
//! Everything svipall returned until now was *content*: markdown, text, extracted fields. That is the
//! right answer for "what does this page say" and the wrong one for "click the second result",
//! because prose has no handles. The agent's only recourse was to guess a CSS selector from the
//! markdown and hope, or to take a screenshot and reason about pixels.
//!
//! What is produced here is the page's own accessible structure: every interactive element with its
//! role, its accessible name, and a short reference the click and type tools take directly. No
//! vision model, no invented selectors, and a fraction of the tokens the HTML would cost.
//!
//! It is walked in JavaScript rather than through the protocol's Accessibility domain. That domain
//! needs the browser to have built a tree for assistive technology and answers `uninteresting` when
//! it has not, which is the normal state — measured, not assumed. Walking the DOM works on every
//! page with no domain to enable and no flag to set, and it can stamp the reference onto the
//! element, so turning a reference back into something clickable is an attribute lookup rather than
//! a second protocol round trip.
//!
//! The shaping below is pure, so what the agent sees is settled by tests rather than by squinting
//! at a real page.

use serde_json::Value;

/// One node worth showing to an agent.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// Handle the interaction tools accept, e.g. `e12`. Also stamped on the element itself.
    pub reference: String,
    pub role: String,
    pub name: String,
    /// Current value, for inputs and the like.
    pub value: Option<String>,
    pub depth: usize,
}

/// Roles worth a reference because something can be done to them.
const INTERACTIVE: &[&str] = &[
    "button",
    "link",
    "textbox",
    "searchbox",
    "checkbox",
    "radio",
    "combobox",
    "listbox",
    "option",
    "menuitem",
    "slider",
    "spinbutton",
    "switch",
    "tab",
    "textarea",
];

/// Roles that carry meaning even though nothing can be done to them: they are what makes the
/// snapshot readable as a page rather than a flat list of controls.
const STRUCTURAL: &[&str] = &[
    "heading",
    "main",
    "navigation",
    "article",
    "banner",
    "contentinfo",
    "form",
    "search",
    "dialog",
    "alert",
    "table",
    "list",
    "region",
    "img",
];

pub fn is_interactive(role: &str) -> bool {
    INTERACTIVE.contains(&role)
}

fn is_structural(role: &str) -> bool {
    STRUCTURAL.contains(&role)
}

/// The walk, run inside the page. Returns candidates in document order.
///
/// Roles are the implicit ones the HTML already carries, with an explicit `role=` winning. The
/// accessible name follows the order a screen reader uses — `aria-label`, `aria-labelledby`, the
/// associated `<label>`, then `alt`, `title`, `placeholder`, then the element's own text — which is
/// why the result reads like what a person sees rather than like markup.
///
/// Invisible elements are skipped here rather than in Rust, because only the page knows what its
/// stylesheets did.
pub const WALK_JS: &str = r#"(() => {
    const ROLES = {
        A: 'link', BUTTON: 'button', INPUT: 'textbox', TEXTAREA: 'textarea', SELECT: 'combobox',
        H1: 'heading', H2: 'heading', H3: 'heading', H4: 'heading', H5: 'heading', H6: 'heading',
        MAIN: 'main', NAV: 'navigation', ARTICLE: 'article', HEADER: 'banner', FOOTER: 'contentinfo',
        FORM: 'form', TABLE: 'table', UL: 'list', OL: 'list', SECTION: 'region', IMG: 'img',
        DIALOG: 'dialog', SUMMARY: 'button',
    };
    const INPUT_ROLES = {
        checkbox: 'checkbox', radio: 'radio', range: 'slider', number: 'spinbutton',
        search: 'searchbox', submit: 'button', button: 'button', reset: 'button', image: 'button',
        file: 'button', hidden: null,
    };
    const visible = (el) => {
        try {
            const s = getComputedStyle(el);
            if (s.display === 'none' || s.visibility === 'hidden') return false;
            if (parseFloat(s.opacity) === 0) return false;
            const r = el.getBoundingClientRect();
            return r.width > 0 && r.height > 0;
        } catch (e) { return false; }
    };
    const roleOf = (el) => {
        const explicit = el.getAttribute('role');
        if (explicit && explicit.trim()) return explicit.trim().split(/\s+/)[0];
        if (el.tagName === 'INPUT') {
            const t = (el.getAttribute('type') || 'text').toLowerCase();
            return (t in INPUT_ROLES) ? INPUT_ROLES[t] : 'textbox';
        }
        return ROLES[el.tagName] || null;
    };
    const text = (el) => (el.innerText || el.textContent || '');
    // A container's text is everything inside it, so taking it as the container's name repeats the
    // whole subtree one line above where it is already listed. Containers are named only by what
    // was written to name them.
    const CONTAINERS = new Set(['main','navigation','article','banner','contentinfo','form',
                                'table','list','region','dialog','search']);
    const nameOf = (el, role) => {
        const aria = el.getAttribute('aria-label');
        if (aria && aria.trim()) return aria;
        const by = el.getAttribute('aria-labelledby');
        if (by) {
            const parts = by.split(/\s+/).map(function (id) {
                const t = document.getElementById(id);
                return t ? text(t) : '';
            }).join(' ');
            if (parts.trim()) return parts;
        }
        if (el.tagName === 'INPUT' || el.tagName === 'SELECT' || el.tagName === 'TEXTAREA') {
            if (el.labels && el.labels.length) {
                const t = text(el.labels[0]);
                if (t.trim()) return t;
            }
        }
        for (const attr of ['alt', 'title', 'placeholder']) {
            const v = el.getAttribute(attr);
            if (v && v.trim()) return v;
        }
        if (CONTAINERS.has(role)) return '';
        return text(el);
    };

    const out = [];
    let seen = 0;
    const walk = (el, depth) => {
        if (seen > 4000) return;
        for (const child of el.children) {
            // One odd node must not cost the page. Measured on a verification wall: a single
            // element threw inside the walk, and the whole snapshot came back as an error.
            try {
            seen++;
            const role = roleOf(child);
            let next = depth;
            if (role && visible(child)) {
                out.push({
                    index: out.length,
                    role: role,
                    name: nameOf(child, role),
                    value: ('value' in child && child.value != null) ? String(child.value) : null,
                    depth: depth,
                });
                // Stamp the handle so a reference resolves without a second round trip.
                try { child.setAttribute('data-svipall-ref', 'e' + (out.length - 1)); } catch (e) {}
                next = depth + 1;
            }
            walk(child, next);
            } catch (e) {}
        }
    };
    walk(document.body || document.documentElement, 0);
    return out;
})()"#;

/// Shape the walk's output: drop what cannot be used, cap the size, keep the references.
///
/// A control with no accessible name is dropped: it cannot be referred to, and offering a handle to
/// it invites a click nobody can describe. Containers survive without a name because they are the
/// page's skeleton.
pub fn prune(raw: &[Value], max_depth: Option<usize>, limit: usize) -> Vec<Node> {
    let mut out = Vec::new();
    for n in raw {
        let role = n["role"].as_str().unwrap_or_default().to_string();
        if role.is_empty() {
            continue;
        }
        let interactive = is_interactive(&role);
        if !interactive && !is_structural(&role) {
            continue;
        }
        let name = squeeze(n["name"].as_str().unwrap_or_default());
        if name.is_empty() && interactive {
            continue;
        }
        let depth = n["depth"].as_u64().unwrap_or(0) as usize;
        if max_depth.is_some_and(|m| depth > m) {
            continue;
        }
        // The reference is the walk's own index, so it still matches the attribute stamped on the
        // element after filtering removes its neighbours. Renumbering here would send clicks
        // somewhere else entirely.
        let index = n["index"].as_u64().unwrap_or(0);
        out.push(Node {
            reference: format!("e{index}"),
            role,
            name,
            value: n
                .get("value")
                .and_then(Value::as_str)
                .map(squeeze)
                .filter(|v| !v.is_empty() && v.len() < 200),
            depth,
        });
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// Accessible names arrive with the page's own whitespace in them, which is noise in a listing.
fn squeeze(s: &str) -> String {
    let joined = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.chars().count() > 120 {
        let cut: String = joined.chars().take(117).collect();
        format!("{cut}...")
    } else {
        joined
    }
}

/// Render for a model: one line per node, indented by depth, reference last.
pub fn render(nodes: &[Node]) -> String {
    let mut out = String::with_capacity(nodes.len() * 48);
    for n in nodes {
        out.push_str(&"  ".repeat(n.depth.min(8)));
        out.push_str(&n.role);
        if !n.name.is_empty() {
            out.push_str(" \"");
            out.push_str(&n.name);
            out.push('"');
        }
        if let Some(v) = &n.value {
            out.push_str(" = \"");
            out.push_str(v);
            out.push('"');
        }
        if is_interactive(&n.role) {
            out.push_str(" [");
            out.push_str(&n.reference);
            out.push(']');
        }
        out.push('\n');
    }
    out
}

/// Nodes whose role or name matches. Capturing the whole page to locate one button is the
/// expensive way to answer a cheap question.
pub fn find(nodes: &[Node], needle: &str) -> Vec<Node> {
    let needle = needle.to_lowercase();
    nodes
        .iter()
        .filter(|n| {
            n.name.to_lowercase().contains(&needle) || n.role.to_lowercase().contains(&needle)
        })
        .cloned()
        .collect()
}

/// The selector that turns a reference back into an element, for the click and type tools.
///
/// Anything that is not a plain reference is refused rather than escaped: a reference comes from
/// this module's own output, so a strange one means the caller invented it, and quietly repairing
/// it would let an invented value reach a selector.
pub fn selector_for(reference: &str) -> Option<String> {
    let ok = reference.len() >= 2
        && reference.starts_with('e')
        && reference[1..].chars().all(|c| c.is_ascii_digit());
    ok.then(|| format!("[data-svipall-ref=\"{reference}\"]"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn node(index: u64, depth: u64, role: &str, name: &str) -> Value {
        json!({"index": index, "depth": depth, "role": role, "name": name, "value": null})
    }

    #[test]
    fn only_things_an_agent_can_use_survive() {
        let raw = vec![
            node(0, 0, "main", ""),
            node(1, 1, "link", "Read more"),
            node(2, 1, "listitem", "not useful"),
            node(3, 1, "button", "Buy"),
        ];
        let roles: Vec<String> = prune(&raw, None, 100).into_iter().map(|n| n.role).collect();
        assert_eq!(roles, vec!["main", "link", "button"]);
    }

    #[test]
    fn only_interactive_nodes_get_a_reference_in_the_rendering() {
        // A reference on a heading invites a click on something that does nothing.
        let raw = vec![node(0, 0, "heading", "Title"), node(1, 0, "button", "Send")];
        let text = render(&prune(&raw, None, 100));
        assert!(text.contains("heading \"Title\"\n"), "{text}");
        assert!(text.contains("button \"Send\" [e1]"), "{text}");
    }

    #[test]
    fn a_control_with_no_name_is_left_out_but_a_container_is_not() {
        let raw = vec![
            node(0, 0, "button", "   "),
            node(1, 0, "main", ""),
            node(2, 0, "button", "Save"),
        ];
        let out = prune(&raw, None, 100);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].role, "main");
        assert_eq!(out[1].name, "Save");
    }

    #[test]
    fn a_reference_survives_its_neighbours_being_filtered_out() {
        // The handle is the walk's index, so it still matches the attribute on the element even
        // though the nodes around it were dropped. Renumbering would send clicks elsewhere.
        let raw = vec![
            node(0, 0, "listitem", "dropped"),
            node(1, 0, "button", "Kept"),
        ];
        let out = prune(&raw, None, 100);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].reference, "e1", "reference must not be renumbered");
    }

    #[test]
    fn depth_and_limit_are_what_make_this_a_token_budget() {
        let raw = vec![
            node(0, 0, "main", ""),
            node(1, 1, "list", ""),
            node(2, 2, "button", "deep"),
        ];
        assert_eq!(prune(&raw, Some(1), 100).len(), 2, "depth ignored");
        assert_eq!(prune(&raw, None, 2).len(), 2, "limit ignored");
    }

    #[test]
    fn accessible_names_are_squeezed_and_capped() {
        let raw = vec![
            node(0, 0, "button", "  lots\n\tof   space  "),
            node(1, 0, "button", &"x".repeat(300)),
        ];
        let out = prune(&raw, None, 100);
        assert_eq!(out[0].name, "lots of space");
        assert!(out[1].name.ends_with("..."));
        assert!(out[1].name.chars().count() <= 120);
    }

    #[test]
    fn find_locates_by_name_or_role_without_the_whole_page() {
        let nodes = prune(
            &[
                node(0, 0, "button", "Add to cart"),
                node(1, 0, "link", "Privacy policy"),
                node(2, 0, "button", "Remove"),
            ],
            None,
            100,
        );
        assert_eq!(find(&nodes, "cart").len(), 1);
        assert_eq!(find(&nodes, "button").len(), 2);
        assert_eq!(
            find(&nodes, "CART").len(),
            1,
            "matching is case-insensitive"
        );
        assert!(find(&nodes, "nothing here").is_empty());
    }

    #[test]
    fn an_input_shows_what_it_currently_holds() {
        let mut n = node(0, 0, "textbox", "Search");
        n["value"] = json!("rust");
        let out = prune(&[n], None, 100);
        assert_eq!(out[0].value.as_deref(), Some("rust"));
        assert!(render(&out).contains("= \"rust\""));
    }

    #[test]
    fn a_reference_turns_back_into_something_clickable() {
        assert_eq!(
            selector_for("e7").as_deref(),
            Some("[data-svipall-ref=\"e7\"]")
        );
    }

    #[test]
    fn an_invented_reference_is_refused_rather_than_repaired() {
        // References come from this module. A strange one means the caller made it up, and quietly
        // escaping it would let an invented value reach a selector.
        for bad in ["", "e", "x7", "e7\"] , script", "e7; drop", "7"] {
            assert!(selector_for(bad).is_none(), "{bad:?} was accepted");
        }
    }

    #[test]
    fn an_empty_page_renders_to_nothing_rather_than_panicking() {
        assert!(prune(&[], None, 100).is_empty());
        assert_eq!(render(&[]), "");
    }
}
