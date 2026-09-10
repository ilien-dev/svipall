#!/usr/bin/env bash
# Tell crates.io that `release.yml` in this repository may publish every crate the workspace
# publishes. The `crates` job in that workflow authenticates over OIDC, and crates.io refuses it
# with "No Trusted Publishing config found" until each crate carries this.
#
#   scripts/crates-trusted-publishing.sh
#
# Needs a crates.io API token with the `trusted-publishing` scope (crates.io -> Account Settings ->
# API Tokens), asked for without echo unless CRATES_IO_TOKEN is set. It is sent to crates.io and
# nowhere else, on stdin rather than argv. A crate already configured is left alone, so running it
# again, or after a new crate joins the workspace, is safe.
set -euo pipefail

owner="ilien-dev"
repo="svipall"
api="https://crates.io/api/v1/trusted_publishing/github_configs"
agent="svipall-trusted-publishing (github.com/$owner/$repo)"
py=$(command -v python3 || command -v python) || { echo "python is required" >&2; exit 1; }

token="${CRATES_IO_TOKEN:-}"
if [ -z "$token" ]; then
    read -rs -p "crates.io token (trusted-publishing scope): " token
    echo >&2
fi
[ -n "$token" ] || { echo "no token given" >&2; exit 1; }

# The token goes in a curl config read from stdin, so it never shows in a process list.
call() {
    printf 'header = "Authorization: %s"\n' "$token" | curl -sS -K - -A "$agent" "$@"
}

# Every member without `publish = false`, by package name: the same set `release.yml` publishes,
# which `release_build.rs` holds it to.
crates=$(cargo metadata --no-deps --format-version 1 | "$py" -c '
import json, sys
for p in json.load(sys.stdin)["packages"]:
    if p["publish"] != []:
        print(p["name"])')

failed=0
for crate in $crates; do
    configured=$(call "$api?crate=$crate" | "$py" -c '
import json, sys
body = json.load(sys.stdin)
if "errors" in body:
    sys.exit("error: " + body["errors"][0]["detail"])
print(any(c["repository_owner"].lower() == sys.argv[1] and c["repository_name"].lower() == sys.argv[2]
          and c["workflow_filename"] == "release.yml" for c in body["github_configs"]))' "$owner" "$repo") \
        || { echo "$crate: could not read its configuration" >&2; failed=1; continue; }
    if [ "$configured" = "True" ]; then
        echo "$crate: already trusts $owner/$repo release.yml"
        continue
    fi
    body=$(printf '{"github_config": {"crate": "%s", "repository_owner": "%s", "repository_name": "%s", "workflow_filename": "release.yml", "environment": null}}' "$crate" "$owner" "$repo")
    if call -f -X POST -H "Content-Type: application/json" -d "$body" "$api" >/dev/null; then
        echo "$crate: now trusts $owner/$repo release.yml"
    else
        echo "$crate: crates.io refused the configuration" >&2
        failed=1
    fi
done
exit "$failed"
