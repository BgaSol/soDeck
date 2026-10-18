#!/bin/sh
set -eu
ssh-keygen -A
export DOCKER_TLS_CERTDIR=
dockerd-entrypoint.sh --host=unix:///var/run/docker.sock --group=docker > /tmp/dockerd.log 2>&1 &
exec /usr/sbin/sshd -D -e
