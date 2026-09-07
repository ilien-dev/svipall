"""One-shot, bounded README corrections from the 2026-09-07 factual audit."""
from pathlib import Path

p = Path("README.md")
s = p.read_text(encoding="utf-8")

def replace(old, new):
    global s
    assert s.count(old) == 1, (old[:100], s.count(old))
    s = s.replace(old, new)

def section(start, end, new):
    global s
    a = s.index(start)
    b = s.index(end, a)
    s = s[:a] + new + s[b:]

replace("Two tags, and the difference between them is real. `latest` carries a browser **and** the captcha", "The Dockerfile defines full and slim builds. The full version tag above carries a browser **and** the captcha")
replace("**nearly half of that list never needs a browser at all**", "**44 of 93 recorded target visits passed at HTTP**")
replace("Gated for every tool the public benchmark measured, too", "Fetching a panel does not prove that it judged the visitor human")
section("| `medium.com`, `canadianinsider.com` |", "\n", "| `medium.com`, `canadianinsider.com` | Historical rule/manual-inspection disagreement: saved responses had site titles and substantial bodies, while the rule matched `cdn-cgi/challenge-platform`. Neither status, title, size nor that script alone establishes useful content; the later native/auto audit separately reviews content |")
section("- **Cross-page boilerplate removal", "  it fired on 2 of 11 sites", "- **Cross-page boilerplate removal — built, measured, and shipped *off*.** Cached sibling pages\n  help identify repeated site text, which can also be useful content. This heuristic can remove\n  the wrong material. In the recorded TeCo evaluation, at the shipping threshold\n")
replace("length. **Never a bare `click()`, `scrollBy` or forged event.**", "length. Built-in pointer, keyboard and wheel actions use this layer; caller-supplied `eval`\n  JavaScript is outside that guarantee.")
section("   loop every turn", "6. **Use native", "   loop during its bounded wait and avoids pointer activity on recognized self-verifying\n   interstitials. This does not guarantee clearance.\n")
section("8. **Tell the truth when it fails.**", "\n---", "8. **Report the observed failure.** Where available, a blocked result includes `blocked_reason`,\n   the classified wall, recognized vendor/evidence and a suggested next step. Classification is\n   heuristic; transport errors and local budget deferrals may have less page evidence.\n")
section("- **A host with no usable GPU", "\n---", "- **Software rendering can affect fingerprint consistency.** A browser may report `SwiftShader`\n  or `llvmpipe` without hardware acceleration; this does not uniquely identify a VM. Changing a\n  renderer string does not reproduce the claimed hardware's output. `web_status` reports detected\n  GPU limitations. Supplied model paths support CPU execution; speed depends on the machine.\n- **Injected page content can affect detection.** In a recorded run, a local security product\n  injected resources into pages. Svipall can report recognized injection evidence in a blocked\n  result, but cannot reliably identify every injecting product or remove it.\n")
section("Read from each project's own README", "\n---", "The following describes project scope from primary documentation checked on **2026-09-07**.\nIt is not a feature-exhaustive comparison or a head-to-head performance test.\n\n| Project | Documented focus |\n|---|---|\n| Svipall | Local Rust CLI, MCP and REST server; bounded automatic routing, content labels and local challenge attempts with human fallback |\n| [Firecrawl](https://github.com/firecrawl/firecrawl) | Web scraping/crawling API with hosted and self-hosted options; the open-source and cloud offerings differ |\n| [Crawl4AI](https://github.com/unclecode/crawl4ai) | Python crawler with browser extraction and a Docker server offering API and MCP access |\n| [Scrapling](https://github.com/D4Vinci/Scrapling) | Python adaptive parsing, fetchers and spiders, with session/proxy controls and MCP integration |\n| [Playwright MCP](https://github.com/microsoft/playwright-mcp) | Browser automation through MCP using structured accessibility snapshots |\n\nThe historical benchmark below and above does not establish current superiority over these\nprojects. Choose based on your required integration and validate your own target pages.\n")
section("Sometimes, and the [comparison table]", "</details>", "There is overlapping functionality, but this repository has not established a current\nhead-to-head winner. The [comparison](#how-svipall-compares) describes documented project scope.\nSvipall focuses on local operation, content labels, bounded routing and local challenge attempts;\ncompatibility, completeness and success still need validation on your workload.\n")
section("No. Every model ships and runs on the CPU.", "</details>", "No GPU is required for the supplied CPU model paths. Availability depends on the build and\ninstalled weights. Browser software rendering can affect fingerprint consistency; `web_status`\nreports detected limitations without proving how a site will classify them.\n")
section("**AGPL-3.0-only.** Free to run", "\n`crates/svipall-extract`", "**AGPL-3.0-only**, subject to the terms in [`LICENSE`](LICENSE), including its conditions for\ndistribution and section 13 on remote network interaction. The component licences and linking\nexception below also apply; this paragraph is not a substitute for those terms.\n")
replace("Nominative use needs no permission and never did: saying that your project uses Svipall, works with\nSvipall, or is a fork of Svipall is fine.", "The project permits descriptive references such as saying that your project uses Svipall,\nworks with Svipall, or is a fork of Svipall, without implying endorsement.")
p.write_text(s, encoding="utf-8", newline="\n")
