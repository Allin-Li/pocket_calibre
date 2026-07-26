#!/usr/bin/env bash
# Запустить приложение на переднем плане и снять скриншот — без участия
# пользователя. Полный агентский цикл поверх тоннеля (tunnel.sh).
#
# Как это работает:
#   - приложение запускается по SSH (голый exec не выходит на передний план);
#   - iv2sh SetActiveTask <pid> 0 делает его активной задачей PocketBook;
#   - /dev/fb0 (8-бит серый, 1448×1072 landscape) содержит кадр панели;
#   - собираем PNG локально, повернув в портрет.
#
# Использование: ./shot.sh [выходной.png]
set -euo pipefail
cd "$(dirname "$0")"

OUT="${1:-shot.png}"
SSH_HOST=pocketbook
RAW=/mnt/ext1/fb.raw

ssh "$SSH_HOST" '
  for p in $(pidof pocket_calibre.app); do kill $p 2>/dev/null; done
  sleep 1
  cd /mnt/ext1/applications
  ./pocket_calibre.app >/tmp/pc.out 2>&1 &
  sleep 5
  PID=$(pidof pocket_calibre.app)
  iv2sh SetActiveTask $PID 0 >/dev/null 2>&1
  sleep 2
  dd if=/dev/fb0 of='"$RAW"' bs=1024 2>/dev/null
' 2>&1 | grep -vE "Warning|Permanently" || true

ssh "$SSH_HOST" "cat $RAW" 2>/dev/null > /tmp/pc_fb.raw

python3 - "$OUT" <<'PY'
import sys
from PIL import Image
raw = open("/tmp/pc_fb.raw", "rb").read()
W, H = 1448, 1072            # fb0 landscape; приложение работает в портрете
img = Image.frombytes("L", (W, H), raw[:W*H]).rotate(-90, expand=True)
img.save(sys.argv[1])
print(f"сохранено: {sys.argv[1]} ({img.size[0]}×{img.size[1]})")
PY