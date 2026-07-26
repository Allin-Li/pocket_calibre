#!/usr/bin/env bash
# Сборка + установка на читалку по SSH одной командой.
#
# Требует поднятого тоннеля (./tunnel.sh) и SSH-алиаса `pocketbook`.
# Копирование идёт через `cat`-over-ssh, а не scp: dropbear на pbjb собран без
# sftp-сервера, а обычный поток по ssh работает всегда.
set -euo pipefail

cd "$(dirname "$0")"

NAME=pocket_calibre
REMOTE_DIR=/mnt/ext1/applications
REMOTE="$REMOTE_DIR/$NAME.app"
SSH_HOST=pocketbook

echo "== Сборка =="
./build-arm.sh

APP="dist/$NAME.app"
[ -f "$APP" ] || { echo "Нет $APP" >&2; exit 1; }

if ! ssh -o ConnectTimeout=8 "$SSH_HOST" true 2>/dev/null; then
    echo "Нет связи с читалкой по SSH. Запустите ./tunnel.sh" >&2
    exit 1
fi

echo "== Установка ($(stat -c %s "$APP") байт) =="
# Пишем во временный файл и переименовываем: оборванная передача не оставит
# полузалитый бинарник на месте рабочего.
ssh "$SSH_HOST" "cat > $REMOTE_DIR/.$NAME.app.tmp && mv $REMOTE_DIR/.$NAME.app.tmp $REMOTE" < "$APP"

echo "== Проверка целостности =="
local_md5=$(md5sum "$APP" | cut -d' ' -f1)
remote_md5=$(ssh "$SSH_HOST" "md5sum $REMOTE | cut -d' ' -f1")

if [ "$local_md5" = "$remote_md5" ]; then
    echo "OK: $remote_md5"
    echo "Установлено в $REMOTE — запускайте из меню «Приложения»."
else
    echo "MD5 не совпал! локально $local_md5, на читалке $remote_md5" >&2
    exit 1
fi
