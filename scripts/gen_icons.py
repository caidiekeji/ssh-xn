#!/usr/bin/env python3
"""生成 AI-SSH 应用图标（纯 stdlib：zlib + struct）。
在终端台架底色上画一个电光蓝终端窗 + 青绿光标 + 琥珀扫描线。
icns/ico 请用 `npx tauri icon icons/128x128.png` 生成。"""
import struct, zlib, os

BG = (20, 18, 16, 255)          # --bg 深炭暖墨
BLUE = (77, 141, 255, 255)      # --acc 电光蓝
GREEN = (34, 212, 140, 255)     # --acc2 青绿
AMBER = (240, 166, 74, 255)     # --warn 琥珀
LINE = (59, 51, 42, 255)        # 发丝线

def png(size):
    W = H = size
    rows = []
    for y in range(H):
        row = bytearray([0])
        for x in range(W):
            # 圆角台架底
            r = size * 0.16
            c = BG
            # 终端窗边框
            mx, my = x / W, y / H
            if (0.16 <= mx <= 0.84 and 0.20 <= my <= 0.80):
                c = (10, 9, 7, 255)  # 终端内深底
            if (0.14 <= mx <= 0.16 or 0.84 <= mx <= 0.86 or 0.18 <= my <= 0.20 or 0.80 <= my <= 0.82):
                c = BLUE
            # 顶栏标题条
            if (0.16 <= mx <= 0.84 and 0.20 <= my <= 0.27):
                c = LINE
            # 三个小圆点（窗控）
            if (0.20 <= mx <= 0.24 and 0.225 <= my <= 0.265):
                c = AMBER
            if (0.27 <= mx <= 0.31 and 0.225 <= my <= 0.265):
                c = GREEN
            if (0.34 <= mx <= 0.38 and 0.225 <= my <= 0.265):
                c = BLUE
            # 提示符 + 命令线（等宽行流）
            if (0.24 <= mx <= 0.26 and 0.40 <= my <= 0.46):
                c = GREEN
            if (0.30 <= mx <= 0.72 and 0.415 <= my <= 0.445):
                c = (232, 228, 218, 255)
            if (0.30 <= mx <= 0.64 and 0.52 <= my <= 0.55):
                c = (125, 114, 98, 255)
            if (0.30 <= mx <= 0.58 and 0.625 <= my <= 0.655):
                c = (125, 114, 98, 255)
            # 块状光标（签名元素）
            if (0.78 <= mx <= 0.84 and 0.70 <= my <= 0.78):
                c = GREEN
            # 扫描线（琥珀）
            if abs(my - 0.885) < 0.012 and 0.16 <= mx <= 0.84:
                c = AMBER
            row += bytes(c)
        rows.append(bytes(row))
    raw = b"".join(rows)
    def chunk(tag, data):
        c = tag + data
        return struct.pack(">I", len(data)) + c + struct.pack(">I", zlib.crc32(c))
    ihdr = struct.pack(">IIBBBBB", W, H, 8, 6, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr)
            + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))

out = os.path.join(os.path.dirname(__file__), "..", "app", "src-tauri", "icons")
os.makedirs(out, exist_ok=True)
for name, size in [("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256)]:
    with open(os.path.join(out, name), "wb") as f:
        f.write(png(size))
    print("wrote", name)

# 1024 源图：供 `npx tauri icon app-icon.png` 生成 icns/ico 全套（CI 打包必需）
with open(os.path.join(os.path.dirname(__file__), "..", "app", "app-icon.png"), "wb") as f:
    f.write(png(1024))
print("wrote app-icon.png")
