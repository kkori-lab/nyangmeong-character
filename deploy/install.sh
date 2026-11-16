#!/bin/bash
# 2번에서 root로 실행 (deploy.yml이 올린 /tmp/nyangmeong-character-deploy 기준)
set -euo pipefail

SRC=/tmp/nyangmeong-character-deploy
APP=/opt/nyangmeong-character
UNIT=/etc/systemd/system/nyangmeong-character.service

# 서비스 전용 계정 (로그인 불가)
id -u nmchar >/dev/null 2>&1 || useradd --system --no-create-home --shell /usr/sbin/nologin nmchar

install -d -o root -g root -m 755 "$APP"
install -o root -g root -m 755 "$SRC/nyangmeong-character" "$APP/nyangmeong-character.new"
mv -f "$APP/nyangmeong-character.new" "$APP/nyangmeong-character"
install -o root -g root -m 644 "$SRC/nyangmeong-character.service" "$UNIT"

systemctl daemon-reload
systemctl enable nyangmeong-character >/dev/null
systemctl restart nyangmeong-character

# 뜰 때까지 최대 10초 기다린 뒤 /health 확인, 실패하면 배포 실패
for _ in $(seq 1 10); do
  if curl -fsS --max-time 2 http://127.0.0.1:8100/health; then
    echo
    echo "배포 완료"
    rm -rf "$SRC"
    exit 0
  fi
  sleep 1
done
journalctl -u nyangmeong-character -n 30 --no-pager
exit 1
