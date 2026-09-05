#!/bin/sh
# One job: make `-p 8787:8787` mean what everybody already assumes it means.
#
# svipall binds the dashboard and the REST API to loopback, which is the right default on a
# machine. Inside a container, loopback is the container, so a published port reaches a socket
# that is listening somewhere else entirely — the port is up, the page never loads, and nothing
# says why. The container's own network is the boundary here instead: nothing is reachable until
# somebody publishes a port, and the REST API stays off until SVIPALL_REST_PORT says otherwise
# and then still wants a bearer key.
#
# Written once, only when there is no config at all. A file the operator wrote is never touched.
set -e

: "${SVIPALL_HOME:=/data}"
config="$SVIPALL_HOME/config.toml"

if [ ! -e "$config" ]; then
    mkdir -p "$SVIPALL_HOME"
    cat > "$config" <<'TOML'
# Written by the container entrypoint on first start, because loopback inside a container means
# the container. Delete these two lines to go back to loopback-only, or edit anything you like —
# this file is only ever created when it is missing, never rewritten.
dashboard_bind = "0.0.0.0"
rest_bind = "0.0.0.0"
TOML
fi

exec "$@"
