#!/usr/bin/env bash
# Тоннель до SSH читалки через телефон.
#
# Роутер (Xiaomi) изолирует беспроводных клиентов друг от друга, поэтому
# напрямую с ноутбука до читалки не достучаться. Но телефон подключён к
# ноутбуку по USB (adb) и раздаёт хотспот, к которому подключена читалка, —
# хотспот клиентов НЕ изолирует. Получается мост:
#
#   ноутбук --(adb forward)--> телефон:2222 --(busybox nc)--> читалка:22
#
# После запуска: `ssh pocketbook` (алиас в ~/.ssh/config) и ./deploy.sh.
set -euo pipefail

LOCAL_PORT=2222
READER_IP="${READER_IP:-10.20.29.96}"   # адрес читалки на хотспоте телефона
READER_SSH_PORT=22
BB=/data/adb/ksu/bin/busybox            # busybox с рабочим `nc -lk -e`

if ! adb get-state >/dev/null 2>&1; then
    echo "Телефон не виден по adb. Подключите USB и включите отладку." >&2
    exit 1
fi

# Релей на телефоне: слушает LOCAL_PORT и на каждое подключение поднимает
# отдельный nc до читалки. -lk держит его персистентным, setsid отвязывает
# от нашей adb-сессии, иначе он умрёт вместе с ней.
echo "Поднимаю релей на телефоне (:$LOCAL_PORT → $READER_IP:$READER_SSH_PORT)…"
adb shell "su -c 'pkill -f \"nc -lk -p $LOCAL_PORT\" 2>/dev/null; setsid $BB nc -lk -p $LOCAL_PORT -e $BB nc $READER_IP $READER_SSH_PORT >/dev/null 2>&1 < /dev/null &'"

sleep 1
adb forward tcp:$LOCAL_PORT tcp:$LOCAL_PORT >/dev/null

# Проверка: должен прийти баннер dropbear.
banner=$( { printf '\n'; sleep 2; } | nc -w 3 127.0.0.1 $LOCAL_PORT 2>/dev/null | head -1 || true)
if [[ "$banner" == SSH-* ]]; then
    echo "Тоннель готов: $banner"
    echo "Теперь: ssh pocketbook   или   ./deploy.sh"
else
    echo "Тоннель поднят, но читалка не ответила. Проверьте, что она в сети" >&2
    echo "хотспота ($READER_IP) и не уснула." >&2
    exit 1
fi
