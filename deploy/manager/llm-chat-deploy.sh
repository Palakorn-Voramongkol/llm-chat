#!/bin/bash
# Forced-command target for the deploy SSH key. Restricts what an inbound
# session can do to a small allowlist of commands.
set -euo pipefail
cmd="${SSH_ORIGINAL_COMMAND:-}"

case "$cmd" in
  restart-manager)
    sudo -n /usr/bin/systemctl restart llm-chat-manager
    sudo -n /usr/bin/systemctl is-active llm-chat-manager
    ;;
  restart-worker)
    sudo -n /usr/bin/systemctl restart llm-chat-worker
    sudo -n /usr/bin/systemctl is-active llm-chat-worker
    ;;
  install-manager-binary)
    install -m 0755 /var/lib/deploy/llm-chat/manager/llm-chat-manager \
                    /usr/local/bin/llm-chat-manager
    ;;
  install-worker-binary)
    install -m 0755 /var/lib/deploy/llm-chat/worker/llm-chat-worker \
                    /usr/local/bin/llm-chat-worker
    ;;
  *)
    # Permit unrestricted scp/sftp into the staging dirs only.
    if [[ "$cmd" == scp* || "$cmd" == "internal-sftp"* ]]; then
      exec $cmd
    fi
    echo "deploy: command not allowed: $cmd" >&2
    exit 1
    ;;
esac
