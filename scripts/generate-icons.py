#!/usr/bin/env python3
"""Draw original OpenCad SVG and PNG icons using only the Python standard library.

The source consists of geometric primitives below. No reference images, installed
applications, fonts, or external artwork are read by this generator.
"""

import hashlib
import html
import json
import math
import re
import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
INK = (216, 222, 231)
BLUE = (129, 201, 236)
GOLD = (231, 183, 108)


def artwork(action):
    shapes = []
    key = (action['id'] + ' ' + action['label']).lower()
    color = BLUE if key.endswith('_on') or '_on ' in key else INK

    def line(x, y, x2, y2, ink=color, width=1.35):
        shapes.append(('line', (x, y, x2, y2), ink, width))

    def path(points, ink=color):
        for a, b in zip(points, points[1:]):
            line(*a, *b, ink)

    def rect(x, y, w, h, ink=color):
        path([(x, y), (x + w, y), (x + w, y + h), (x, y + h), (x, y)], ink)

    def circle(x, y, radius, ink=color, fill=False):
        shapes.append(('disc' if fill else 'circle', (x, y, radius), ink, 1.35))

    def arc(x, y, radius, start, end, ink=color):
        path([(x + radius * math.cos(t), y + radius * math.sin(t))
              for t in [start + (end - start) * i / 40 for i in range(41)]], ink)

    def arrow(x, y, x2, y2, ink=color):
        line(x, y, x2, y2, ink)
        angle = math.atan2(y2 - y, x2 - x)
        for offset in [-0.6, 0.6]:
            line(x2, y2, x2 - 3 * math.cos(angle + offset), y2 - 3 * math.sin(angle + offset), ink)

    if 'toolset_collapse' in key or 'toolset_expand' in key:
        flip = 'expand' in key
        for x in [8, 14]:
            path([(x + (4 if flip else 0), 5), (x + (0 if flip else 4), 12), (x + (4 if flip else 0), 19)])
    elif 'save' in key:
        rect(4, 3, 16, 18); rect(7, 3, 9, 6, BLUE); rect(7, 14, 10, 7)
    elif 'open' in key:
        path([(3, 18), (3, 5), (10, 5), (12, 8), (21, 8), (21, 11)])
        path([(3, 20), (7, 11), (22, 11), (18, 20), (3, 20)], BLUE)
    elif 'new drawing' in key or 'id_new ' in key:
        path([(5, 3), (14, 3), (19, 8), (19, 21), (5, 21), (5, 3)])
        path([(14, 3), (14, 8), (19, 8)], BLUE); line(8, 14, 16, 14, BLUE); line(12, 10, 12, 18, BLUE)
    elif 'undo' in key or 'redo' in key:
        redo = 'redo' in key
        points = [(4, 9), (14, 9), (19, 13), (19, 19)]
        path([(24 - x if redo else x, y) for x, y in points])
        x = 20 if redo else 4
        path([(x + (-4 if redo else 4), 5), (x, 9), (x + (-4 if redo else 4), 13)], BLUE)
    elif 'print' in key or 'plot' in key:
        rect(6, 3, 12, 6); rect(3, 9, 18, 9); rect(6, 15, 12, 7); circle(18, 12, 0.8, BLUE, True)
    elif 'zoom' in key:
        circle(10, 10, 7); line(15, 15, 22, 22, BLUE, 2); line(6, 10, 14, 10, BLUE); line(10, 6, 10, 14, BLUE)
    elif 'pan' in key and 'expand' not in key:
        path([(5, 13), (8, 16), (8, 5), (10, 4), (11, 12), (12, 3), (14, 3), (14, 12), (16, 5), (18, 6), (17, 13), (20, 9), (22, 11), (18, 20), (10, 21), (4, 15), (5, 13)])
    elif 'move' in key:
        for x, y in [(3, 12), (21, 12), (12, 3), (12, 21)]:
            arrow(12, 12, x, y, BLUE)
    elif 'copy' in key:
        rect(3, 3, 12, 12); rect(9, 9, 12, 12, BLUE)
    elif 'erase' in key or 'delete' in key or 'purge' in key:
        path([(3, 15), (13, 4), (21, 12), (12, 21), (9, 21), (3, 15)])
        line(7, 11, 16, 18, BLUE)
    elif 'layer' in key:
        for y, ink in [(7, BLUE), (12, color), (17, color)]:
            path([(3, y), (12, y - 4), (21, y), (12, y + 4), (3, y)], ink)
    elif 'lock' in key and 'block' not in key:
        rect(5, 11, 14, 11, GOLD); arc(12, 10, 5, math.pi, 2 * math.pi); line(12, 15, 12, 18)
    elif 'grid' in key or 'table' in key or 'array' in key or 'snapmode' in key:
        rect(3, 3, 18, 18)
        for n in [9, 15]:
            line(3, n, 21, n, BLUE); line(n, 3, n, 21, BLUE)
    elif 'hatch' in key or 'gradient' in key:
        rect(3, 3, 18, 18)
        for n in range(-12, 19, 4):
            line(max(3, 3 + n), min(21, 21 + n), min(21, 21 + n), max(3, 3 + n), BLUE)
    elif 'text' in key or 'style' in key or 'spell' in key or 'attribute' in key:
        path([(3, 21), (11, 3), (13, 3), (21, 21)]); line(7, 14, 17, 14, BLUE)
    elif 'leader' in key:
        arrow(14, 7, 4, 21, BLUE); line(14, 7, 22, 7); circle(18, 7, 4)
    elif 'dim' in key or 'measure' in key or 'dist' in key:
        line(4, 5, 4, 21); line(20, 5, 20, 21); arrow(12, 12, 4, 12, BLUE); arrow(12, 12, 20, 12, BLUE)
        line(8, 3, 16, 3, GOLD)
    elif 'circle' in key or 'radius' in key or 'diameter' in key or 'donut' in key:
        circle(12, 12, 9); line(12, 12, 18, 6, BLUE); circle(12, 12, 1, BLUE, True)
    elif 'arc' in key or 'rotate' in key or 'polar' in key:
        arc(11, 13, 9, math.pi, 2.2 * math.pi); line(11, 13, 19, 7, BLUE); circle(11, 13, 1, BLUE, True)
    elif 'object' in key and 'snap' in key:
        rect(8, 8, 8, 8, BLUE); line(2, 12, 8, 12); line(16, 12, 22, 12); line(12, 2, 12, 8); line(12, 16, 12, 22)
    elif 'ortho' in key:
        path([(4, 3), (4, 20), (21, 20)]); path([(4, 14), (10, 14), (10, 20)], BLUE)
    elif 'rectang' in key or 'viewport' in key:
        rect(3, 5, 18, 14); circle(3, 5, 1.2, BLUE, True); circle(21, 19, 1.2, BLUE, True)
    elif 'block' in key or 'modeling' in key or '3d' in key:
        path([(3, 7), (12, 2), (21, 7), (21, 17), (12, 22), (3, 17), (3, 7)])
        path([(3, 7), (12, 12), (21, 7)], BLUE); line(12, 12, 12, 22, BLUE)
    elif 'pline' in key or 'polyline' in key or 'drafting' in key:
        points = [(3, 20), (8, 5), (16, 16), (21, 3)]
        path(points)
        for x, y in points:
            circle(x, y, 1.2, BLUE, True)
    elif 'line' in key or 'ray' in key:
        line(4, 20, 20, 4); rect(2.5, 18.5, 3, 3, BLUE); rect(18.5, 2.5, 3, 3, BLUE)
    else:
        # Unimplemented commands retain a neutral CAD glyph with an ID-derived node.
        path([(3, 18), (8, 6), (20, 9), (16, 20), (3, 18)])
        variant = hashlib.sha256(action['id'].encode()).digest()[0] % 4
        x, y = [(3, 18), (8, 6), (20, 9), (16, 20)][variant]
        circle(x, y, 1.7, BLUE, True)
    return shapes


def svg(action, shapes):
    lines = ['<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">',
             '<title>' + html.escape(action['label']) + '</title>']
    for kind, values, ink, width in shapes:
        color = '#%02x%02x%02x' % ink
        if kind == 'line':
            x, y, x2, y2 = values
            lines.append(f'<line x1="{x:g}" y1="{y:g}" x2="{x2:g}" y2="{y2:g}" stroke="{color}" stroke-width="{width}" stroke-linecap="round"/>')
        else:
            x, y, radius = values
            lines.append(f'<circle cx="{x:g}" cy="{y:g}" r="{radius:g}" fill="{color if kind == "disc" else "none"}" stroke="{color}" stroke-width="{width}"/>')
    return '\n'.join(lines + ['</svg>', ''])


def png(shapes, size):
    scale = size * 3 / 24
    side = size * 3
    pixels = bytearray(side * side * 4)
    for kind, values, ink, width in shapes:
        if kind == 'line':
            x, y, x2, y2 = [v * scale for v in values]
            radius = width * scale / 2
            bounds = (min(x, x2) - radius, min(y, y2) - radius, max(x, x2) + radius, max(y, y2) + radius)
        else:
            x, y, r = [v * scale for v in values]
            radius = r + width * scale / 2
            bounds = (x - radius, y - radius, x + radius, y + radius)
        for py in range(max(0, math.floor(bounds[1])), min(side, math.ceil(bounds[3]))):
            for px in range(max(0, math.floor(bounds[0])), min(side, math.ceil(bounds[2]))):
                if kind == 'line':
                    length = (x2 - x) ** 2 + (y2 - y) ** 2
                    t = max(0, min(1, ((px + .5 - x) * (x2 - x) + (py + .5 - y) * (y2 - y)) / length)) if length else 0
                    hit = math.hypot(px + .5 - x - t * (x2 - x), py + .5 - y - t * (y2 - y)) <= radius
                else:
                    distance = math.hypot(px + .5 - x, py + .5 - y)
                    hit = distance <= radius if kind == 'disc' else abs(distance - r) <= width * scale / 2
                if hit:
                    i = (py * side + px) * 4
                    pixels[i:i + 4] = bytes((*ink, 255))
    raw = bytearray()
    for y in range(size):
        raw.append(0)
        for x in range(size):
            samples = [pixels[((y * 3 + dy) * side + x * 3 + dx) * 4:((y * 3 + dy) * side + x * 3 + dx) * 4 + 4]
                       for dy in range(3) for dx in range(3)]
            visible = [p for p in samples if p[3]]
            raw.extend(bytes([sum(p[c] for p in visible) // len(visible) if visible else 0 for c in range(3)] + [255 * len(visible) // 9]))

    def chunk(kind, data):
        return struct.pack('!I', len(data)) + kind + data + struct.pack('!I', zlib.crc32(kind + data))

    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('!2I5B', size, size, 8, 6, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw, 9)) + chunk(b'IEND', b'')


def main():
    data = json.loads((ROOT / 'assets/ui/catalog.json').read_text())
    destination = ROOT / 'assets/icons'
    destination.mkdir(parents=True, exist_ok=True)
    count = 0
    for action in data['actions']:
        if not action['icon']:
            continue
        identifier = action['id']
        if not re.fullmatch(r'[A-Za-z0-9_-]+', identifier):
            raise ValueError(f'Unsafe icon ID: {identifier}')
        stem = identifier + '-' + hashlib.sha256(identifier.encode()).hexdigest()[:8]
        shapes = artwork(action)
        (destination / (stem + '.svg')).write_text(svg(action, shapes))
        size = 16 if identifier in ['UI_Toolset_expand', 'UI_Toolset_collapse'] else 32
        (destination / (stem + '.png')).write_bytes(png(shapes, size))
        count += 1
    print(f'Generated {count} original SVG/PNG icon pairs')


if __name__ == '__main__':
    main()
