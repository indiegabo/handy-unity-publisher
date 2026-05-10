#!/bin/sh
set -eu

ensure_docker_socket_access() {
  socket_path=/var/run/docker.sock

  if [ ! -S "$socket_path" ]; then
    return 0
  fi

  socket_gid=$(stat -c '%g' "$socket_path")
  if [ -z "$socket_gid" ]; then
    return 0
  fi

  if id -G app | tr ' ' '\n' | grep -Fx "$socket_gid" >/dev/null 2>&1; then
    return 0
  fi

  socket_group=$(getent group "$socket_gid" | cut -d: -f1 || true)
  if [ -z "$socket_group" ]; then
    socket_group="dockersock-$socket_gid"
    groupadd --gid "$socket_gid" "$socket_group"
  fi

  usermod -aG "$socket_group" app
}

mkdir -p \
  /data \
  /data/logs \
  /data/artifacts \
  /data/workspaces \
  /var/lib/handy-unity-bulder

chown -R app:app /var/lib/handy-unity-bulder
chown app:app /data /data/logs /data/artifacts /data/workspaces

ensure_docker_socket_access

exec gosu app "$@"