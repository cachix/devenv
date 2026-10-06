#!/usr/bin/env bash
set -ex

wait_for_processes

sendmail -S "unix:$MAILPIT_SMTP_SOCKET" john@example.com <<MAIL
Subject: Hello

Hello world!
MAIL

curl --fail --silent --unix-socket "$MAILPIT_UI_SOCKET" \
	http://localhost/api/v1/messages
