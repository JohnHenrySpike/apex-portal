#!/usr/bin/env python3
"""Flydigi Apex 5 lighting control over the vendor hidraw channel.

Protocol from openflydigi (github.com/mkaliaha/openflydigi, MIT), PROTOCOL.md §2, §9.
Only lighting commands are allowed; vibration (0x12) and everything else is refused.

Usage:
  apex5_led.py show
  apex5_led.py color RRGGBB [--brightness N]
  apex5_led.py brightness N            # 0..100
  apex5_led.py rainbow|flow|breath|wave C1 [C2 ...] [--speed N]
  apex5_led.py off
  apex5_led.py backup FILE | restore FILE
  add --save to any write to commit to flash (survives sleep/power-off)
"""
import argparse, glob, os, select, sys, time

REPORT_OUT = 0x03
MAGIC = (0x5A, 0xA5)
CMD_STATUS, CMD_LED_READ, CMD_LED_START, CMD_LED_PACK, CMD_SAVE = 161, 167, 168, 169, 166
ALLOWED = {CMD_STATUS, CMD_LED_READ, CMD_LED_START, CMD_LED_PACK, CMD_SAVE}
PKG = 20

OFF_LOOP_START, OFF_LOOP_END, OFF_CYCLE, OFF_BRIGHT, OFF_LEDS, OFF_MODE = 3, 4, 5, 6, 7, 8
OFF_FRAMES = 20


def find_dev():
    for d in sorted(glob.glob('/sys/class/hidraw/hidraw*')):
        if '000037D7' in open(d + '/device/uevent').read() and \
                open(d + '/device/report_descriptor', 'rb').read()[:3] == b'\x06\xa0\xff':
            return '/dev/' + os.path.basename(d)
    sys.exit('Apex 5 vendor interface not found (is it connected by cable/dongle?)')


class Pad:
    def __init__(self):
        self.fd = os.open(find_dev(), os.O_RDWR)
        self.drain()

    def drain(self):
        while select.select([self.fd], [], [], 0)[0]:
            os.read(self.fd, 64)

    def send(self, cmd, payload, wait=0.5, until=None):
        if cmd not in ALLOWED:
            raise SystemExit(f'refusing command {cmd}: not a lighting command')
        buf = bytearray(32)
        buf[0:4] = bytes([REPORT_OUT, *MAGIC, cmd])
        ln = len(payload) + 2
        buf[4] = ln
        buf[5:5 + len(payload)] = payload
        buf[3 + ln] = sum(buf[3:3 + ln]) & 0xFF
        os.write(self.fd, bytes(buf))
        out, end = [], time.time() + wait
        while (left := end - time.time()) > 0 and select.select([self.fd], [], [], left)[0]:
            r = os.read(self.fd, 64)
            if len(r) > 7 and r[1:3] == bytes(MAGIC) and r[3] == cmd:
                out.append(r[1:])
                if until and until(out):
                    break
        return out

    def acked(self, cmd, payload):
        return bool(self.send(cmd, payload, until=lambda o: True))

    def read_led(self):
        for _ in range(3):
            chunks, total = {}, None

            def done(out):
                nonlocal total
                total = out[-1][3]
                chunks[out[-1][4]] = bytes(out[-1][6:6 + PKG])
                return len(chunks) >= total

            self.send(CMD_LED_READ, bytes([0, PKG]), wait=1.5, until=done)
            if total and len(chunks) == total:
                return bytearray(b''.join(chunks[i] for i in range(total)))
        sys.exit('no lighting reply (pad asleep? press a button)')

    def write_led(self, blob, old):
        new_p = [blob[i:i + PKG] for i in range(0, len(blob), PKG)]
        old_p = [old[i:i + PKG] for i in range(0, len(old), PKG)]
        changed = [i for i in range(len(new_p)) if new_p[i] != old_p[i]]
        if not changed:
            return 0
        start, end = changed[0], changed[-1] + 1
        if not self.acked(CMD_LED_START, bytes([0, start, end - start, PKG])):
            sys.exit('pad rejected write header')
        for off, i in enumerate(range(start, end)):
            if not self.acked(CMD_LED_PACK, bytes([off]) + bytes(new_p[i])):
                sys.exit(f'pad rejected packet {i}')
        return end - start

    def save(self):
        st = self.send(CMD_STATUS, b'', until=lambda o: True)
        if not st:
            sys.exit('no status reply, not saved')
        slot = st[0][4] & 3
        tag = st[0][5 + 2 * slot:7 + 2 * slot]  # active profile's version tag, kept as is
        r = self.send(CMD_SAVE, bytes(tag), wait=10, until=lambda o: True)
        print('saved to flash' if r else 'save: no ACK')


def geometry(blob):
    leds = blob[OFF_LEDS] or 10
    return leds, (len(blob) - OFF_FRAMES) // (3 * leds)


def put(blob, frame, led, rgb):
    leds, _ = geometry(blob)
    o = OFF_FRAMES + (frame * leds + led) * 3
    blob[o:o + 3] = bytes(max(0, min(255, int(c))) for c in rgb)


def fill(blob, fn, mode):
    leds, frames = geometry(blob)
    for f in range(frames):
        for l in range(leds):
            put(blob, f, l, fn(f / frames, l / leds))
    blob[OFF_LOOP_START], blob[OFF_LOOP_END], blob[OFF_MODE] = 0, frames - 1, mode


def sample(cols, p):
    n = len(cols)
    x = (p % 1.0) * n
    a, b, t = cols[int(x) % n], cols[(int(x) + 1) % n], x - int(x)
    return tuple(a[i] + (b[i] - a[i]) * t for i in range(3))


def scale(c, k):
    return tuple(v * k for v in c)


def tri(p):
    return 1 - abs(2 * (p % 1.0) - 1)


NAMES = {'red': 'ff0000', 'green': '00ff00', 'blue': '0000ff', 'white': 'ffffff',
         'cyan': '00ffff', 'magenta': 'ff00ff', 'yellow': 'ffff00', 'orange': 'ff6000',
         'purple': '8000ff', 'pink': 'ff4080'}


def colour(s):
    s = NAMES.get(s.lower(), s).lstrip('#')
    if len(s) == 3:
        s = ''.join(c * 2 for c in s)
    return tuple(bytes.fromhex(s))


def show(blob):
    leds, frames = geometry(blob)
    print(f'version {blob[1]:02x}{blob[0]:02x}  mode {blob[OFF_MODE]}  brightness {blob[OFF_BRIGHT]}  '
          f'loop {blob[OFF_LOOP_START]}..{blob[OFF_LOOP_END]}  cycle {blob[OFF_CYCLE]}  '
          f'{frames} frames x {leds} leds')
    for f in range(frames):
        o = OFF_FRAMES + f * leds * 3
        print(f'  frame {f}: ' + ' '.join(blob[o + 3 * l:o + 3 * l + 3].hex() for l in range(leds)))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('action')
    ap.add_argument('args', nargs='*')
    ap.add_argument('--brightness', type=int)
    ap.add_argument('--speed', type=int, help='cycle time, bigger = slower (1..255)')
    ap.add_argument('--save', action='store_true', help='commit to flash')
    a = ap.parse_args()

    pad = Pad()
    old = pad.read_led()
    blob = bytearray(old)
    act = a.action

    if act == 'show':
        return show(old)
    if act == 'backup':
        open(a.args[0], 'wb').write(old)
        return print(f'saved {len(old)} bytes to {a.args[0]}')
    if act == 'restore':
        blob = bytearray(open(a.args[0], 'rb').read())
        if len(blob) != len(old):
            sys.exit('backup size mismatch')
    elif act == 'color':
        c = colour(a.args[0])
        fill(blob, lambda f, l: c, 4)
    elif act == 'off':
        fill(blob, lambda f, l: (0, 0, 0), 0)
    elif act == 'rainbow':
        hues = [(255, 0, 0), (255, 255, 0), (0, 255, 0), (0, 255, 255), (0, 0, 255), (255, 0, 255)]
        fill(blob, lambda f, l: sample(hues, l + f), 6)
    elif act == 'flow':
        cols = [colour(x) for x in a.args]
        fill(blob, lambda f, l: sample(cols, l + f), 1)
    elif act == 'breath':
        cols = [colour(x) for x in a.args]
        fill(blob, lambda f, l: scale(cols[int(f * len(cols)) % len(cols)], tri(f)), 3)
    elif act == 'wave':
        cols = [colour(x) for x in a.args]
        fill(blob, lambda f, l: scale(sample(cols, l), tri(l + f)), 7)
    elif act != 'brightness':
        sys.exit(f'unknown action {act}')

    if act == 'brightness':
        a.brightness = int(a.args[0])
    if a.brightness is not None:
        blob[OFF_BRIGHT] = max(0, min(100, a.brightness))
    if a.speed is not None:
        blob[OFF_CYCLE] = max(1, min(255, a.speed))

    n = pad.write_led(blob, old)
    print(f'wrote {n} packet(s)')
    if a.save:
        pad.save()


if __name__ == '__main__':
    main()
