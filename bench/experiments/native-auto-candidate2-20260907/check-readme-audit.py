"""Offline structural/source reconciliation for the README factual audit."""
import hashlib
import html
import json
from pathlib import Path
import re
import time
import tomllib
from urllib.parse import unquote

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parents[2]
readme = (REPO / "README.md").read_text(encoding="utf-8")
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()

def anchors(text):
    result = set(re.findall(r'<a\s+(?:name|id)="([^"]+)"', text))
    used = {}
    fence = False
    for line in text.splitlines():
        if line.startswith("```"):
            fence = not fence
        if fence:
            continue
        if re.match(r"^#{1,6} ", line):
            heading = re.sub(r"^#+ ", "", line).strip()
            heading = re.sub(r"<[^>]+>", "", html.unescape(heading)).lower()
            slug = re.sub(r"[^\w\- ]", "", heading).replace(" ", "-")
            n = used.get(slug, 0)
            used[slug] = n + 1
            result.add(slug + (f"-{n}" if n else ""))
    return result

links = re.findall(r"\]\(([^\s]+?)(?:\s+\"[^\"]*\")?\)", readme)
links += re.findall(r'(?:href|src|srcset)="([^"]+)"', readme)
missing = []
local_count = 0
anchor_count = 0
for link in sorted(set(links)):
    if re.match(r"^[a-zA-Z][a-zA-Z0-9+.-]*:", link):
        continue
    path, _, fragment = unquote(link).partition("#")
    target = REPO / (path or "README.md")
    local_count += 1
    if not target.exists():
        missing.append(link)
    elif fragment and target.suffix == ".md":
        anchor_count += 1
        if fragment not in anchors(target.read_text(encoding="utf-8")):
            missing.append(link)
assert not missing, missing
fences = [line for line in readme.splitlines() if line.startswith("```")]
assert len(fences) % 2 == 0
assert readme.count("<details>") == readme.count("</details>")

server = (REPO / "crates/svipall-mcp/src/server.rs").read_text(encoding="utf-8")
names = re.findall(r"#\[tool\([\s\S]*?\)\]\s*(?:pub )?async fn (\w+)", server)
tool_table = readme.split("## MCP tools", 1)[1].split("### The CLI", 1)[0]
assert len(names) == 29 and all(f"`{n}`" in tool_table for n in names)
rest = (REPO / "crates/svipall-mcp/src/rest.rs").read_text(encoding="utf-8")
routes = re.findall(r'"(/v1/[^\"]+)"', rest.split("pub const ROUTES:", 1)[1].split("];", 1)[0])
assert len(routes) == 19 and all(p in readme for p in routes)

config = tomllib.loads(re.search(r"```toml\n(.*?)\n```", readme, re.S)[1])
rust = (REPO / "crates/svipall-core/src/config.rs").read_text(encoding="utf-8")
defaults = rust.split("impl Default for Config", 1)[1].split("/// Where", 1)[0]
verified = []
for key, value in config.items():
    assert re.search(rf"pub {key}:", rust), key
    if key == "blocklist_sources":
        assert all(v in defaults for v in value)
    elif key == "reputation_budget":
        assert value == 250
        assert "DEFAULT_BUDGET: f32 = 250.0" in (REPO / "crates/svipall-core/src/reputation.rs").read_text()
    elif key == "reputation_half_life_hours":
        assert value == 6
        assert "DEFAULT_HALF_LIFE_HOURS: u32 = 6" in (REPO / "crates/svipall-core/src/reputation.rs").read_text()
    else:
        expression = re.search(rf"\b{key}: ([^\n]+)", defaults)[1].rstrip(",")
        if expression == "String::new()": actual = ""
        elif expression == "Vec::new()": actual = []
        elif expression.endswith(".into()"): actual = json.loads(expression[:-7])
        elif expression in ("true", "false"): actual = expression == "true"
        else: actual = int(expression.replace("_", ""))
        assert actual == value, (key, actual, value)
    verified.append(key)

comparison = json.loads((ROOT / "variant-comparison.json").read_text())
for entry in comparison["experiments"]:
    auto, native = (entry["arms"][k] for k in ("auto", "native"))
    assert entry["total_calls"] == 918
    assert f'{auto["useful"]} / {native["useful"]}' in readme
    assert f'{auto["available"]} / {native["available"]}' in readme
    assert f'{auto["seconds_per_useful"]:.2f} / {native["seconds_per_useful"]:.2f}' in readme
manifest = json.loads((ROOT / "manifest.json").read_text())
changed = sorted(name for name, expected in manifest["source_sha256"].items()
                 if not (REPO / name).exists() or sha(REPO / name) != expected)
assert changed == [".gitattributes", "README.md", "scripts/measure_automatic.py", "scripts/test_measure_automatic.py"], changed
qc = json.loads((ROOT / "narrow-validation/qc-execution.json").read_text())
assert qc["exit_code"] == qc["qc_exit_code"] == qc["local_browser_exit_code"] == 0
result = {
    "checked_unix": time.time(), "readme_sha256": sha(REPO / "README.md"),
    "local_targets_checked": local_count, "anchors_checked": anchor_count,
    "missing_targets_or_anchors": missing, "code_fences": len(fences),
    "mcp_tools_documented": len(names), "rest_tool_routes_documented": len(routes),
    "documented_config_defaults_matched": verified,
    "comparison_versions_reconciled": len(comparison["experiments"]),
    "changed_frozen_inputs": changed, "product_inputs_match_measured_snapshot": True,
    "qc_record_exit_code": qc["exit_code"],
    "scope": "Documentation validation; does not rerun QC, live benchmarks or installations. Historical audit hashes remain historical.",
}
(ROOT / "readme-factual-audit.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
print(json.dumps(result, indent=2))
