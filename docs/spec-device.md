# Flydigi Apex 5 — device, settings, input, screen spec

This spec is extracted for a Rust reimplementation. It was produced by read-only analysis of the
`openflydigi` Python project. No hardware was touched.

- **Citations** look like `file:line` and are relative to the openflydigi checkout root (for
  example `flydigi/device.py:244`).
- **Provenance labels** are carried over from the source:
  - **HW**: measured on an Apex 5 by the openflydigi author.
  - **SDK**: transcribed from Flydigi Space Station 4.2.0.9 decompiles, with no hardware check.
  - **UNVERIFIED**: unknown, or the source contradicts itself.
- **Scope.** The lighting commands (167–169) and the mapping/profile commands (161–166, 171, 172–175)
  belong to another spec. They appear here only where they touch this scope.

---

## 0. Conventions used throughout this document

### 0.1 Offset convention: **raw** (the report-id byte is index 0)

Every offset in this document is a **raw** offset. Index 0 is the HID report id byte (`0x03` on
the way out, `0x04` on the way back), exactly what `read()`/`write()` on `/dev/hidrawN` produce and
consume.

Flydigi's SDK (and much of openflydigi's prose) indexes **after** stripping the report id. So
`SDK data[n]` equals `raw[n+1]` (PROTOCOL.md:471-473, flydigi/motion.py:16-17,
docs/findings-steam.md:126-127).

### 0.2 Output packet (host to pad), always 32 bytes written

| raw | value |
|---|---|
| 0 | `0x03` report id (flydigi/device.py:35, PROTOCOL.md:463-465) |
| 1 | `0x5A` |
| 2 | `0xA5` |
| 3 | command id |
| 4 | length L. Normally `payload_len + 2` (it counts bytes [3] and [4]) |
| 5 .. 5+payload_len-1 | payload |
| 3+L | checksum = `sum(raw[3 .. 3+L)) & 0xFF` (end exclusive) (PROTOCOL.md:101-112, flydigi/blobs.py:27-39) |
| rest | zero |

- The checksummed builder is `blobs.build` (flydigi/blobs.py:27-39).
- The raw builder is `device.build` (flydigi/device.py:244-252). It sets `[4]=len(payload)` and
  writes no checksum, and callers override `[4]` by hand.
- Which commands take a checksum is given per command below.
- HID report map of the vendor interface (PROTOCOL.md:461-462):

  | report | direction | size |
  |---|---|---|
  | `0x03` | output | 31 B + id |
  | `0x04` | input | 31 B + id |
  | `0x08` | output | 12 B, unused by the project |

### 0.3 Reply / input report (pad to host), report id `0x04`

Command replies and the live input stream share report id `0x04`. **They are distinguished only by
raw[3]**:

- `0xEF` means a live input report.
- Anything else is the echoed command id. No command id collides with `0xEF`
  (flydigi/blobs.py:60-62, flydigi/motion.py:72-85).

Single-frame command reply (HW; flydigi/mock/pad.py:463-477, PROTOCOL.md:467-473):

| raw | meaning |
|---|---|
| 0 | `0x04` |
| 1,2 | `5A A5` |
| 3 | command id. Not always the one you sent; see the screen 208/209 aliases in §4.3 |
| 4 | total frames in this reply (1 for single-frame) |
| 5 | frame index (0) |
| 6.. | payload. For most write commands raw[6] = success flag `1`. For command 19, raw[6] is the echoed **value** |

- **Matching rule.** Match on raw[0]==0x04 and raw[3]==expected id. That is Flydigi's `IsAck`
  (flydigi/identity.py:342-348). `Controller.ack_ok` additionally requires raw[6]==1
  (flydigi/device.py:453-459).
- **Reply checksums.** Flydigi never validates a reply checksum (PROTOCOL.md:994-998, dock SDK;
  same transport layer). Do not validate them.
- **Replies are broadcast to every reader of the hidraw node.** Steam/SDL and Space Station may be
  sending their own commands, so stale or foreign replies arrive.
  - Drain pending reads before each write (flydigi/device.py:393-410).
  - Match by command id with a deadline, and expect to see foreign replies (PROTOCOL.md:500-506,
    docs/findings-steam.md:29-43).
  - openflydigi uses an advisory `flock(LOCK_EX)` on the hidraw fd to serialise its own processes
    (flydigi/device.py:340-391). It must not and cannot exclude Steam.
- **Bad checksum means silence.** A checksummed command with a bad checksum gets **no reply**, not
  a NAK. Time out instead (PROTOCOL.md:888, flydigi/blobs.py:14-15).
- **Timeouts.** The SDK default is 500 ms × 3 retries (PROTOCOL.md:96-99). The project uses
  0.3–0.6 s per exchange.

---

## 1. Device discovery and identity

### 1.1 Finding the vendor node

The algorithm is `device._real_nodes` (flydigi/device.py:188-210) and `find_device`
(flydigi/device.py:213-241):

1. Enumerate `/dev/hidraw*`. For each node, read `/sys/class/hidraw/<node>/device/uevent` and parse
   `HID_ID=<bus>:<vendor>:<product>` (all hex, zero-padded to 8 digits) (flydigi/device.py:119-134).
2. Require vendor == `0x37D7`.
3. Require **`product >> 12 == 2`**, the controller family. The same Flydigi classification uses
   `6` for the CD2 dock (`37d7:6001`) and `1` for the cooler (flydigi/device.py:40-71).
   - **Do not match on VID + descriptor alone.** The CD2 charging dock is also `37d7` and its
     descriptor also starts `06 a0 ff`. A sleeping pad leaves the bus, so the old code opened the
     dock (flydigi/device.py:40-51).
4. Read `/sys/class/hidraw/<node>/device/report_descriptor` and require its first 3 bytes ==
   `06 A0 FF` (usage page 0xFFA0) (flydigi/device.py:38, :197-208).
   - The pad also exposes a keyboard/mouse hidraw (`05 01 09 06`). Only interface 2 (`x-y:1.2`)
     carries the vendor page (docs/device-settings.md:78-92).
5. **Exclude `37d7:2502` (Apex 6 / "gpa6").** It uses a different framing and refuses legacy
   packets (flydigi/device.py:200-206, :426-431; udev/72-flydigi-apex5.rules:29-30). Treat it as
   unsupported.
6. Open the node `O_RDWR | O_NONBLOCK` (flydigi/device.py:321).

Then gate on identity with command 1 (§1.3). `find_device` cannot tell pad models apart. A Vader 5
(`f5`) or Apex 6 opens the same way, and only the device type distinguishes them
(flydigi/identity.py:7-36, :264-282).

USB topology of a wired Apex 5 (docs/device-settings.md:78-84):

| interface | driver | endpoints | content |
|---|---|---|---|
| `1.0` | xpad | ep 0x81 IN 1 ms, ep 0x02 OUT 4 ms | Xbox-compatible gamepad (evdev) |
| `1.1` | usbhid | ep 0x82 IN 1 ms | keyboard + mouse, `05 01 09 06` descriptor |
| `1.2` | usbhid | ep 0x83 IN 1 ms, ep 0x03 OUT 4 ms | **vendor interface**, `06 a0 ff` |

- The pad is full-speed USB.
- **A sleeping pad leaves the USB bus entirely**, so "no node" equals asleep or unplugged
  (flydigi/device.py:219-222, flydigi/settings.py:230-233).
- **Switch mode.** When the pad is in Nintendo Switch mode it enumerates as `057e:2009` (Switch Pro
  Controller) with **no** 37d7 and no vendor page, so it is unreachable (docs/device-settings.md:472-477).
- **Dongle.** The vendor page and report id `0x03` are identical over the 2.4 GHz dongle
  (flydigi/device.py:37, :216-217; PROTOCOL.md:429-430). The dongle's own USB product id is **not
  stated anywhere in the source** (UNVERIFIED). The udev rule only names `2501`, and the
  `pid>>12==2` rule would accept any `0x2xxx`. One test fixture shows the HID product string
  `"Flydigi APEX5 Wireless"` (flydigi/device.py:139, tests/test_models.py:2024).
- **Bluetooth.** Untested (PROTOCOL.md:429-430).

### 1.2 Connection type: wired vs dongle

- Read it from **command 1 reply raw[7]**: `1` = wired, `2` = dongle (flydigi/motion.py:158-164,
  flydigi/gpa6.py:168, PROTOCOL.md:1084).
- openflydigi maps "not 1" to dongle.
- Bluetooth value: unknown.
- Do not infer the connection type from the node number. It changes between wired and dongle
  (flydigi/device.py:216-217).

### 1.3 Command 1 — GET_INFO / heartbeat (`HeartBeatControllerCommandNewXInput`)

**Request** (flydigi/motion.py:326-328, flydigi/registry.py:198-202):
```
03 5A A5 01 02 03 00…        [4]=2, [5]=checksum(sum of [3..5)) = 0x03
```
Notes on accepted request forms:
- HW: the pad also answered `03 5a a5 01 00 …` (length 0, no checksum) (PROTOCOL.md:469-470).
- UNVERIFIED: PROTOCOL.md:1079 shows a captured `03 5a a5 01 02 47`, which does not match the
  computed sum of 0x03. Use the computed sum.

**Reply** (HW capture over the dongle, firmware 7.0.4.5; PROTOCOL.md:1079-1085):
```
raw: 04 5a a5 01 01 00 80 02 00 00 00 00 05 45 01 00 70 45 21 31 45 25 00 00 01 28 00 00 11 31 1f 00
idx:  0  1  2  3  4  5  6  7  8  9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31
```

| raw | field | decode | source |
|---|---|---|---|
| 3 | `0x01` | | |
| 4 / 5 | frames total / index | `IsAckFinished` = `data[4] > data[3] \|\| data[4] == data[3]-1`; the reply is "fragmented" in SDK terms, but every observed reply is a single frame | docs/device-settings.md:294-297 |
| 6 | **DeviceType** | 128 K5, 129 K5Eva, 133 K5Mm, 134 K5Srs, 135 K5GS, 136 K5LZ, all code `k5` = Apex 5 | flydigi/identity.py:84-89, PROTOCOL.md:549-550 |
| 7 | **ConnectType** | 1 wired, 2 dongle | flydigi/motion.py:158-164 |
| 8..11 | "DeviceMac", 4 bytes, stored LS-last | display as `reversed(bytes)` joined by `:`, upper-case. **HW: all zeroes on this pad, so treat 0 as "none"** | flydigi/motion.py:119-148 |
| 12 | **battery** | `hi = b>>4, lo = b&0x0F`; `hi == 1` means charging (Flydigi reports level 6); otherwise level = `lo` in **0..5** (5 = full). HW: wired pad read `level 5, charging False` | flydigi/motion.py:159-161, docs/device-settings.md:272-292 |
| 13 | chip type | (0x45 in capture) | docs/device-settings.md:294-296 |
| 14 | motion chip type | (0x01 in capture) | same |
| 15 | unlabelled | 0x00 in capture, UNVERIFIED | — |
| 16..29 | **7 firmware versions**, 2 bytes each, BCD nibbles `hi>>4 . hi&F . lo>>4 . lo&F`; all-zero = not present | order: **main, dongle, switch/SI, trigger, screen, ADC, NearLink** | flydigi/motion.py:171-192 |
| 30 | `0x1f` in capture | not a reproducible sum. Do not validate | — |

The capture decodes to:

| component | value |
|---|---|
| main | 7.0.4.5 |
| dongle | 2.1.3.1 |
| switch | 4.5.2.5 |
| trigger | none |
| screen | 0.1.2.8 |
| ADC | none (a Vader part) |
| NearLink | 1.1.3.1 |

A wired pad reports dongle = none (flydigi/motion.py:171-175).

Further notes on the reply fields:
- **Battery granularity.** There is no finer battery figure anywhere in the command set
  (docs/device-settings.md:272-283).
- **Version compare.** Compare versions numerically. Flydigi's ordinal string compare is buggy
  (flydigi/motion.py:210-238).
- **Device types.** Other device types belong to other models. Refuse to write to them
  (flydigi/identity.py:84-100, :135, :264-282). openflydigi supports `k5` and an unvalidated `f5`.
  An Apex 5 clone app should accept **only 128, 129, 133–136**.

### 1.4 Protocol version (3.1 / 3.2)

- **Not in command 1.** "ProtoVersion" is the little-endian u16 at **offset 0 of the 840-byte
  mapping profile blob**:
  - An Apex 5 reports **769 = 0x0301 = v3.1**.
  - A Vader 5 reports 770 = v3.2 (docs/findings-profile-blob.md:647-648, PROTOCOL.md:912-916).
- It decides macro layout and limits (PROTOCOL.md:952-958).
- Reading it needs a profile read (command 163). **Command 163 switches the active profile as a side
  effect** (PROTOCOL.md:865-869). That belongs to the mapping spec.

### 1.5 Command 4 — UID (the per-unit identifier)

```
TX 03 5A A5 04 02 06 …          ([4]=2, checksum = 0x06; flydigi/identity.py:351-356)
RX 04 5a a5 04 01 00 | 14 20 6e 7a 1c 00 00 00 00 dc ba 3e 00 | 00 …
```
- The UID is **13 bytes at raw[6..19)** (HW; PROTOCOL.md:1089-1099, flydigi/identity.py:317,
  :365-378).
- Lower-case hex of it is the stable key openflydigi uses (flydigi/registry.py:280-292).
- The pad reports **no USB serial number** (docs/findings-steam.md:410-416).
- UNVERIFIED: PROTOCOL.md:1093 shows a captured checksum of `4a`, which differs from the computed
  value. Use the computed `06`.

### 1.6 Nickname — command 2 (read), command 24 (write)

**Read** (HW; flydigi/identity.py:381-422, PROTOCOL.md:1101-1115):
```
TX 03 5A A5 02 02 04 …
RX 04 5a a5 02 01 00 44 65 73 6b 00 …     -> name bytes from raw[6]
```
- Take raw[6..], cut at the first `0x00` or `0xFF`, and decode as UTF-8.
- Reject empty, first byte 0x00/0xFF, invalid UTF-8, or non-printable text.
- **A factory-fresh pad returns junk** (`01 01 09 09 09 64 04 5e`). It passes Flydigi's own
  emptiness test, so also apply the printable check.

**Write, command 24** (`0x18`). HW-verified layout (flydigi/identity.py:425-486, PROTOCOL.md:1117-1155):
```
03 5A A5 18 (2+n) <n name bytes UTF-8> 00 …      NO checksum
```
- n = 1..**26** UTF-8 bytes (bytes, not characters) (flydigi/identity.py:327).
- The pad stores `raw[4]-1` bytes starting at raw[5], which is **one byte more than the name**.
  The byte right after the name therefore **must be 0**. If you put a checksum there, it becomes
  part of the name.
- The command is **not checksum-validated** and ACKs anything, so verify by reading back with
  command 2.
- Flydigi's own builder is buggy: it writes the checksum at fixed raw[6] and corrupts the second
  character. **Do not copy it.**

### 1.7 Command 28 (`0x1C`) — "AcquireController" / third-party heartbeat

Layout (SDK; docs/device-settings.md:262-265, docs/findings-steam.md:178-185):
```
03 5A A5 1C 17 <acquire 1|0> <20-byte ASCII tag, NUL padded, raw[6..26)> <checksum at raw[26] = sum[3..26)>
```
- The reply carries the grant at raw[6] (SDK `data[5]`).
- The tag is read back by command 16 as `control_by` (§1.8).

What it does:
- It is an **advisory cooperative lock**. SDL sends it with tag `"SDL"` and **re-sends it every
  30 s** (`FLYDIGI_ACQUIRE_CONTROLLER_HEARTBEAT_TIME`) (docs/findings-steam.md:20-22, :515-521).
- SDL only sends it after it sees command-16 `third_party == 1` (docs/findings-steam.md:178-185).

Is it required? **No.**
- The SDK never sends it before trigger commands.
- 81/82, settings and config commands are HW-verified without it (docs/device-settings.md:262-265).
- openflydigi **does not implement it**.
- Recommendation for the Rust app: **do not send it.** Sending it would compete with Steam/SDL.

**Heartbeat / keep-alive: none is required** for the vendor interface.
- Space Station and the openflydigi GUI poll command 1 every **30 s** while connected, and every
  **2 s** while searching (gui/app.py:36-43; PROTOCOL.md:500-502).
- That poll is informational, not a keep-alive.

### 1.8 Commands 16/17 — transport flags and third-party takeover

**Command 16, read** (HW; flydigi/motion.py:241-289, docs/findings-steam.md:125-136):
```
TX 03 5A A5 10 02 12 …
```

| raw | field (1 = on) |
|---|---|
| 6 | `controller_data` (XInput report on interface 1.0) |
| 7 | `raw_data` (the 0xEF vendor input stream) |
| 8 | `keyboard` |
| 9 | `mouse` |
| 10 | `third_party` ("Allow third-party apps to take over mappings") |
| 11..30 | `control_by`, 20-byte ASCII tag, NUL-trimmed (e.g. `"SDL"`) |

**Command 17, write** (HW; flydigi/motion.py:43-59, docs/findings-steam.md:112-123):
```
03 5A A5 11 07 <controller_data> <raw_data> <keyboard> <mouse> <third_party> <checksum at raw[10]=sum[3..10)>
```
- Each flag is `1` on, `0` off, or **`0xFF` leave unchanged**. Always send `0xFF` for flags you do
  not mean to change.
- Match the reply on raw[3] == 0x11.

Behaviour:
- **Third-party firmware gate.** Space Station offers `third_party` only if the main firmware is
  **≥ 7.0.3.0** on k5 (flydigi/motion.py:243-246, docs/findings-steam.md:141-148).
- **Turning `third_party` on** makes SDL/Steam acquire the pad. The pad then sets itself
  `controller_data` = 0 and `raw_data` = 1, which **kills the xpad evdev gamepad input**.
- **Turning it off** restores those flags by itself. Nothing re-enumerates
  (docs/findings-steam.md:166-191, :353-357).
- Vendor commands (81/82, configs, the motion stream) keep working with third-party on
  (docs/findings-steam.md:325-335).
- **Never write `controller_data=0`** unless you mean to blind the standard gamepad. The keyboard
  and mouse flags are never written by Flydigi (docs/findings-steam.md:200-201). Treat writing them
  as UNVERIFIED and risky.
- Do not confuse this with the dock's command 17, which means something else on `37d7:6001`
  (PROTOCOL.md:1014).

---

## 2. Device settings

**One read covers everything** (command 3). Writes go through:

| command | role |
|---|---|
| 19 | ten boolean sub-features |
| 20–23 | four numeric settings |
| 29 | restart |

These are pad (not profile) state. **There is no commit command**; they take effect when they land
(gui/models/settings.py:5-10).

- **Persistence across power cycle/sleep: not documented in the source (UNVERIFIED).** By contrast,
  profile/lighting state needs command 166 to survive sleep (PROTOCOL.md:871-875). No equivalent
  is documented for settings.
- **Always read command 3 back after a write.** The command-19 ACK does not say which setting
  moved, and the pad ACKs unsupported sub-ids (flydigi/settings.py:31-37, :285-301).

### 2.1 Command 3 — `ReadHardwareFunctionStatus`

```
TX 03 5A A5 03 02 05 …                                   (flydigi/settings.py:176-182)
RX 04 5A A5 03 01 00 FB 7B 01 00 0F 00 02 11 …           (HW, wired Apex 5; docs/device-settings.md:55-63, tests/test_settings.py:31)
```

Raw offsets (the source's `body[n]` is raw[n+1]; flydigi/settings.py:154-173):

| raw | field |
|---|---|
| 6 | **supported** bits for sub-ids 1..8, bit (N-1) |
| 7 | **enabled** bits for sub-ids 1..8 |
| 8 | supported bits for sub-ids 9..10, bit (N-9) |
| 9 | enabled bits for sub-ids 9..10 |
| 10 | sleep time, minutes (0 = never) |
| 11 | report rate (wire enum) |
| 12 | stick precision (wire enum) |
| 13 | stick sensitivity (wire enum) |

**Rule: sub-id N is bit N-1** (sub-ids 9 and 10 roll into raw 8/9) (flydigi/settings.py:21-24, :132-136).

### 2.2 Command 19 — sub-features (boolean)

```
03 5A A5 13 04 <subId> <value 0|1> <checksum at raw[7] = sum[3..7)>
RX 04 5A A5 13 01 00 <value echoed> <crc>   — echoes VALUE, never the sub-id (HW)
```
Sources: flydigi/settings.py:188-210, docs/device-settings.md:158-162, :222-230.

| sub | name | supported on k5 (HW reply) | default seen | notes |
|---|---|---|---|---|
| 1 | quick-switch config ("Fast Swap Config": modifier + A/B/X/Y picks a profile) | yes | on | **HW write+readback verified** (docs/device-settings.md:232-237) |
| 2 | Xbox home button | yes | on | **Do not expose.** SDK refuses it on NewXInput, there is no UI, and the effect is unmeasured (docs/device-settings.md:179-204; flydigi/settings.py:41-45) |
| 3 | motion debounce | **no** | — | unsupported on k5; do not expose |
| 4 | mapping switch (Space Station: "Turbo Function", locale key `turbo_mapping_title`) | yes | on | enables per-button turbo; whether it also gates macros is unverified (docs/space-station.md §4.1). **Not** the third-party toggle (docs/device-settings.md:169; docs/findings-steam.md:159-160). openflydigi's GUI does expose it (gui/models/settings.py:50-51) |
| 5 | joystick debounce | yes | on | turning it off **disables auto-calibration** (docs/device-settings.md:107-118) |
| 6 | joystick auto-calibration | yes | on | grey it out while sub 5 is off (gui/models/settings.py:200-209). **This is the only stick "calibration" control in the project. No manual stick or trigger calibration command exists in the source** |
| 7 | joystick rebound filter | yes | on | filters the reverse spike on release |
| 8 | **screen status bar always on** | yes | off | screen setting (§4.1) |
| 9 | **always-on display** (SDK name `OffScreen`, **inverted name**) | yes | off | **HW verified**: 1 = picture stays up; 0 = panel dark, logo button wakes it ~2 s (PROTOCOL.md:640-650) |
| 10 | audio | **no** | — | unsupported on k5 |

The "default seen" column is the HW reply 0xFB/0x7B/0x01/0x00 (tests/test_settings.py:44-58).

### 2.3 Commands 20–23 (numeric) and 29 (restart)

```
03 5A A5 <cmd> 03 <value> <checksum at raw[6] = sum[3..6)>
```
- Reply: match on raw[3] only. The success-flag position is **unmeasured** for 20–23
  (flydigi/settings.py:213-224; docs/device-settings.md:176-177).

| cmd | setting | wire values | HW status |
|---|---|---|---|
| **20** (0x14) | report (polling) rate | `1`=1000 Hz, `2`=500, `4`=250, `8`=125 | **Pad reads `0`**, which is not in the map and has an unknown meaning. **Never written on HW.** openflydigi hides it behind `--i-know`. The pad already polls at the 1 ms USB ceiling. **Do not write** (flydigi/settings.py:115-122, :249-259; docs/device-settings.md:69-73) |
| **21** (0x15) | stick precision | **enum declaration index**, not bits: `1`=8-bit, `2`=10-bit, `3`=12-bit, `4`=9-bit, `5`=11-bit, `6`=14-bit, `7`=16-bit (`0`=None) | HW: 3 written, read back as 12-bit. Default 2 (10-bit) (flydigi/settings.py:94-98; docs/device-settings.md:94-105) |
| **22** (0x16) | stick centre sensitivity | `14` Highest, `15` High, `16` Middle-high, `17` Middle, `18` Low-middle, `19` Low, `20` Lowest | HW: 16 written and read back. Default 17 (flydigi/settings.py:100-113) |
| **23** (0x17) | sleep timer, minutes | `0` = never; Space Station 4.2.0.9 offers 1, 5, 15, 60, 180 and never (docs/space-station.md §4.1); openflydigi clamps to 0..60 | HW: 30, then 15, written and read back. Default 15 (flydigi/settings.py:89-92, :227-236) |
| **29** (0x1D) | restart | `03 5A A5 1D 02 1F` (no argument) | **Never sent on HW.** The hidraw node vanishes and returns, so reopen it (flydigi/settings.py:262-270; docs/device-settings.md:20-24) |

Notes on these numeric settings:
- **Precision only affects the xpad/evdev path.** It quantises evdev output. The 0xEF vendor
  stream is **not** quantised (docs/device-settings.md:133-147).
- **Sleep has a real cost.** Sleep makes the pad leave USB; a sleeping pad is indistinguishable
  from an unplugged one.

### 2.4 Other "settings" that are NOT settable on this pad (from the source)

| request | reality |
|---|---|
| Screen brightness / screen timeout | **No command in the source.** Screen controls are only sub 8, sub 9 and the picture upload. The LED ring brightness lives in the lighting blob (byte 6, 0..100; docs/device-settings.md:354), which is another spec |
| Mode switching (Xbox/Switch/…) | **Command 27** `03 5A A5 1B 03 <mode> <crc>`, `BluetoothMode {Switch=1, Xbox=2, Flashplay=3, DInput=4}`. SDK only, **never sent**: "treat as a one-way trip" (docs/findings-other-devices.md:687-690). Physically, the firmware uses a long-press of the Home/logo button for mode switching (flydigi/relay.py:48-50). See DANGEROUS |
| Joystick calibration | Only the sub-6 auto-calibration toggle. No calibration command found |
| Trigger calibration | None. The trigger travel window lives in the profile blob (offset 123), which is the mapping spec (PROTOCOL.md:346-356) |
| Dock smart stop | Command 80 sub 16, legacy envelope; never sent to a k5 and unverified (docs/findings-other-devices.md:719-725) |
| LED test 245 | `03 5A A5 F5 05 R G B crc@[8]`. ACKs, echoes, **changes nothing** (HW; docs/device-settings.md:327-332). Factory-test command; avoid |

---

## 3. Live input report (0xEF stream)

### 3.1 Enabling / disabling

- **Enable:** command 17 with `controller_data=1, raw_data=1`, others `0xFF`:
  `03 5A A5 11 07 01 01 FF FF FF <crc>` (flydigi/motion.py:62-64).
- **Disable:** command 17 with `raw_data=0`, others `0xFF`:
  `03 5A A5 11 07 FF 00 FF FF FF <crc>` (flydigi/motion.py:67-69).
- Enabling raw data does not disturb the xpad node (HW; flydigi/motion.py:12-14).
- **Default state:** `raw_data` reads False by default. SDL turns it on when it takes over
  (docs/findings-steam.md:186-188, :353-355). Some comments describe the node streaming
  continuously at ~970 Hz (flydigi/blobs.py:47-49, flydigi/identity.py:333-336). Check command 16
  rather than assuming the stream is on or off (UNVERIFIED).
- **Rate:** ~**970 Hz wired** (HW: 3870 reports in 4 s) and ~295–300 Hz over the dongle
  (docs/findings-steam.md:367-369, :515-518). Read with a 32-byte buffer, or 64 bytes as the project
  does (flydigi/device.py:443).
- Remember to send the disable command when the tester closes. The relay does so
  (flydigi/relay.py:579-592).

### 3.2 Layout (raw offsets, report id kept)

Sources: docs/findings-steam.md:371-396, flydigi/motion.py:29-40, :72-85.

| raw | field | type / decode |
|---|---|---|
| 0 | `0x04` | report id |
| 1,2 | (presumably `5A A5`; not checked by the parser, UNVERIFIED) | |
| 3 | **`0xEF`** marker | required; otherwise the report is a command reply |
| 4,5 | left stick X | i16 LE (`00 80` = -32768, `FF 7F` = +32767) |
| 6,7 | left stick Y | i16 LE, **negate** for up-positive |
| 8,9 | right stick X | i16 LE |
| 10,11 | right stick Y | i16 LE, **negate** |
| 12 | buttons 0 | Up 0x01, Right 0x02, Down 0x04, Left 0x08, A 0x10, B 0x20, Select 0x40, X 0x80 |
| 13 | buttons 1 | Y 0x01, Start 0x02, LB 0x04, RB 0x08, LT(digital) 0x10, RT(digital) 0x20, L3 0x40, R3 0x80 |
| 14 | buttons 2 | C 0x01, Z 0x02, M1 0x04, M2 0x08, M3 0x10, M4 0x20, M5 0x40, M6 0x80 |
| 15 | buttons 3 | Menu 0x01, Turbo 0x02, (0x04 unknown), Home 0x08, Back 0x10 |
| 16 | left trigger | u8, 0..255 linear |
| 17 | right trigger | u8, 0..255 linear |
| 18,20,22 | gyro X, Y, Z | i16 LE. HW-verified ≈0 at rest. Scale unknown; DS5-calibration inference ~0.05 °/s per LSB (UNVERIFIED) (flydigi/motion.py:112-116) |
| 24,26,28 | accel X, Y, Z | i16 LE. **~4096 LSB per g** (HW: Z ≈ 4096 flat) (flydigi/motion.py:98-110) |

Notes:
- **Source of the button map.** It is transcribed from `OperatorDataParser.IsButtonPressed`
  (SDK). Sticks, gyro and accel are HW-verified. Only the sticks, gyro and accel offsets have been
  hardware-checked by sweeping; treat the button bits as SDK-level (UNVERIFIED on HW).
- **Stick axis conversion.** Flydigi converts a stick axis with `-65535`, which is off by one. Unpack
  as signed instead (docs/findings-steam.md:385-386).
- **Lock trigger effect.** With the `Lock` trigger effect active, the trigger axis becomes digital,
  0 or 255 only (PROTOCOL.md:492-496).
- **Third-party mode.** The 0xEF stream keeps running with third-party on, while the xpad/evdev
  path goes dead (docs/findings-steam.md:365-369).
- **Alternative source.** Sticks and buttons can also come from xpad evdev on interface 1.0, but
  **C, Z, M1–M6, gyro and accel are only in the vendor stream** (docs/findings-haptics.md:51-53).

---

## 4. Screen (160 × 80)

### 4.1 Screen settings (safe; ordinary commands)

| what | packet | status |
|---|---|---|
| Always-on display | cmd 19 sub 9, value 1 = picture stays up, 0 = panel dark (a real blank; logo wakes it ~2 s) | **HW** (PROTOCOL.md:632, :640-650; flydigi/screen.py:479-502) |
| Status bar always on | cmd 19 sub 8, value 0/1 | supported per command 3; used to restore the flag after a HID-picture accident (PROTOCOL.md:631, :557) |
| Read both | command 3: raw[6] bit7 / raw[7] bit7 = status-bar supported/on; raw[8] bit0 / raw[9] bit0 = always-on supported/on | HW (PROTOCOL.md:633; flydigi/screen.py:552-567) |
| Test flood 242 | see DANGEROUS. **Do not ship** | |

The command-19 ACK does not identify the sub-id, so read command 3 back (PROTOCOL.md:652-653).

### 4.2 Image format — LVGL v8 true-colour (HW + verified against Flydigi's own files)

Sources: PROTOCOL.md:528-550; flydigi/screen.py:7-25, :71-77, :104-141.

- **Frame size:** 25604 bytes = 4-byte header + 160 × 80 × 2.
- **Header:** LE u32 `cf | (w << 10) | (h << 21)`, with `cf = 4` (`LV_IMG_CF_TRUE_COLOR`). For
  160 × 80 it is the constant **`04 80 02 0A`**. Bits 5..9 must be 0.
- **Pixels:** RGB565, **high byte first** (`LV_COLOR_16_SWAP=1`), row-major, stride 320.
  - Pack: `v = ((r&0xF8)<<8) | ((g&0xFC)<<3) | (b>>3)`, then emit `[v>>8, v&0xFF]`.
  - Example: red = `F8 00`, blue = `00 1F`.
- **Multi-frame `.bin`:** frames concatenated with no container. Flydigi's stock files are
  `default_screen_image_<deviceType>.bin`, 550 k5 frames across 6 files.
- **"GIF support"** means multi-frame. The host decodes a GIF to frames; nothing GIF-encoded goes
  over the wire. The OTA session declares `picType` = **GIF (1)** for more than one frame and
  **PNG (2)** for exactly one (flydigi/screen_ota.py:57-61, :286-287).
  - Frame period on the serial path: `frameRate = round(interval_ms/10)` clamped to 1..255
    (hundredths of a second) (flydigi/screen_ota.py:290-296).
  - The GUI clamps the interval to 10–2550 ms (gui/models/screen.py:694).
- **Maximum 255 frames.** The count is one byte. **The serial backend does not enforce this**: it
  sends `len & 0xFF` and writes every frame. Enforce ≤255 yourself (PROTOCOL.md:690-693;
  docs/findings-screen.md:36-45; gui/models/screen.py:53).

### 4.3 Upload path A — HID picture family 208–211: **does NOT work on Apex 5. Do not use.**

HW: two complete uploads (9623 packets) were ACKed and echoed, and **the display never changed**
(PROTOCOL.md:552-565; flydigi/screen.py:27-45, :395-407). Space Station never sends this path to a
k5 (PROTOCOL.md:601-604).

It is still harmful:
- **211 commits picture metadata.** 208 followed by 211 for a frame that was never sent
  **destroyed the stored custom image**, and the screen fell back to the status view after reboot.
- The uploads also left the status-bar flag on (PROTOCOL.md:556-565; flydigi/screen.py:370-377).

For reference only:

| cmd | payload |
|---|---|
| 208 | `picId=1, picType, count, idx(1-based), period, sizeHi, sizeLo` |
| 209 | `offHi, offLo, 24 data bytes` (tail zero-padded) |
| 210 | `picId, idx, sizeHi, sizeLo, 0` |
| 211 | `picId, count, sizeHi, sizeLo, 0` |

- Envelope: `5A A5`, checksummed. Size = 25604.
- Reply ids: **208 is acked as `0x18`, 209 as `0x19`**; 210/211 are acked under their own ids
  (flydigi/screen.py:337-348; PROTOCOL.md:582-621).

### 4.4 Upload path B — serial OTA to the screen chip: **the path that works** (HW: test card + 14-frame animation)

Sources: flydigi/screen_ota.py (whole file); PROTOCOL.md:674-806; docs/findings-screen.md:88-125;
gui/worker.py:764-809.

**Preconditions (MUST):**
1. **Wired only.** Re-read command 1 immediately before; require raw[7]==1.
   - Over the dongle, command 31 switches the screen chip, but the CDC tty never reaches the host,
     and the pad is **stranded until power-cycled at its physical switch**
     (docs/findings-screen.md:10-28; gui/worker.py:776-790).
2. **The udev rule for `ffaa:5555` must be installed** (§5). Otherwise the tty is `root:dialout`
   and the upload fails *after* the pad has already switched (PROTOCOL.md:706-710).
3. If a `ffaa:5555` tty already exists (the pad is already switched from a failed attempt), skip
   steps 1–3 below and go straight to the serial session (flydigi/screen_ota.py:363-375;
   gui/worker.py:775-776).

**Sequence:**
1. **HID command 31**, `03 5A A5 1F 03 04 26`. `[5]` = `ChipModule` 4 = `CHIP_SCREEN`.
   - Hard-code it. **Never parameterise the chip** (flydigi/screen_ota.py:139-156).
   - The ACK may not arrive. Absence is not failure (docs/findings-screen.md:101-104).
2. Sleep **5 s** (Space Station's hard-coded settle) (flydigi/screen_ota.py:73).
3. Poll up to 30 s, every 0.5 s, for a tty whose USB device is **VID `0xFFAA` / PID `0x5555`**.
   - On Linux: walk `/sys/class/tty/ttyACM*` and `ttyUSB*`, and read
     `<node>/device/../idVendor` and `idProduct` (flydigi/screen_ota.py:159-191).
   - HW: the pad's 37d7 hidraw nodes **stay enumerated** alongside it (PROTOCOL.md:695-704).
   - Drop the HID handle anyway, because the pad reboots at the end.
4. Open the tty at **921600 8N1**: raw mode, no flow control, CLOCAL|CREAD, and **assert DTR and
   RTS** (TIOCMBIS) (flydigi/screen_ota.py:197-227).
5. Run the serial protocol below with one request and one reply at a time. The reply timeout is
   **18 s** (60 × 300 ms) (flydigi/screen_ota.py:70-72).
6. After the final reply the screen **syncs ~15 s and reboots itself**. **Do not cut power during
   this window** (PROTOCOL.md:798-799, :813-822).

**Serial framing** (flydigi/screen_ota.py:75-86, :270-280):
- **Out:** `[opcode u8][lengthField u16 LE][payload]`.
  - **The length field is a per-opcode constant. Copy it; do not compute it.**
- **In:** `[result u8][opcode u8][len u16 LE][payload…]`.
  - A reply shorter than 5 bytes carries only `[result][opcode]` (this is the end-of-session reply).
  - The meaning of `result` codes is **unknown**, and openflydigi ignores it (UNVERIFIED).

| step | opcode | lengthField | payload | reply payload | count |
|---|---|---|---|---|---|
| 1 | **11** PicGetBaseAddr | `04 00` | `picType(1=GIF,2=PNG), picNum (u8), frameRate (u8), isRestoreDefault (0/1)` | `baseAddr` u32 LE at reply[4..8] (HW: `0x002ff000`) | 1 |
| 2 | **10** PicGetVersion | `06 00` | none | version (ignored) | 1 |
| 3 | **3** EraseSector | `06 00` | `addr` u32 LE = base + i·4096 | echo | ceil(size/4096) |
| 4 | **5** WriteData | `40 00` (64 = whole packet) | `addr` u32 LE = base + i·55, `37 00` (u16 55), then **55** data bytes (the last packet is short, but the inner 55 is kept) | echo | ceil(size/55) |
| 5 | **12** PicResetDevice | `08 00` | `totalLength` u32 LE, `crc32` u32 LE | short reply with opcode 12 = done | 1 |

- **Step order:** base address (11) is fetched **before** version (10) despite its higher opcode.
- **size:** `size = 25604 × frames`; data = frames concatenated.
- **Timing (HW):** ~52 ms per exchange (~19/s). One frame = 7 erases + 466 writes ≈ **25 s**.
  14 frames ≈ 5 min 46 s. 255 frames ≈ 1.8 h (PROTOCOL.md:681-689).

**CRC** (`AppOtasCrcCal`; flydigi/screen_ota.py:93-133, PROTOCOL.md:786-796). Its quirks:
- standard reflected table (poly 0xEDB88320)
- seed 0, no final xor
- the arithmetic is **i32**
- `q = crc / 256` is a signed **truncating** division (Rust `i32 /` matches)

Rust:
```rust
let mut crc: i32 = 0;
for &b in data { let q = crc / 256; crc = crc.wrapping_shl(8) ^ (TABLE[((q ^ b as i32) & 0xFF) as usize] as i32); }
let out = crc as u32;
```

**Why the openflydigi author judged this tolerable** (flydigi/screen_ota.py:22-35; PROTOCOL.md:801-806):
- The base address is read from the chip, and all writes are base + offset, so the
  **program region is unreachable** without `ScreenUpgradeType.PROGRAM`. A picture upload never
  sends PROGRAM; the console's `--upgrade_type 2` is CUSTOM_PIC (PROTOCOL.md:746-749).
- A failed picture upload leaves a working gamepad with a stale picture. It is retryable on the
  already-present tty without a second command 31 (PROTOCOL.md:819-822).
- The factory animation can be restored by uploading Flydigi's
  `default_screen_image_<deviceType>.bin` with `isRestoreDefault=1` (PROTOCOL.md:805-806).
  openflydigi does not ship that file (docs/findings-screen.md:117-121).
- Space Station's own failure recovery is "slide the power switch on the back to restart"
  (PROTOCOL.md:815).

**Risk rating: MEDIUM.** It writes the flash of the screen chip through a vendor bootloader.
- **Unknown:** whether the upgrade-mode flag is volatile (PROTOCOL.md:433-435). The bootloader's
  behaviour on malformed input is also unknown.
- The source has a stale contradiction: flydigi/screen.py:41-45 says "nothing should" send command
  31, while screen_ota.py does.

Recommendation:
- Ship this only behind explicit confirmation, a wired check, a frame cap of 255, no
  auto-retry (gui/worker.py:767-772), and no polling of the HID node during the upload
  (gui/app.py:463-464).
- Or **omit uploads entirely** and ship only sub 8 / sub 9.

---

## 5. udev rules for non-root access

These are verbatim from `udev/72-flydigi-apex5.rules` (lines 27, 30, 38, 45, 48, 55). Install to
`/etc/udev/rules.d/72-flydigi-apex5.rules`, then run `udevadm control --reload && udevadm trigger`
(udev/72-flydigi-apex5.rules:7-8; flydigi/setup.py:65-67, :485-497).

```
# Apex 5 vendor command interface
KERNEL=="hidraw*", ATTRS{idVendor}=="37d7", ATTRS{idProduct}=="2501", TAG+="uaccess"
# Apex 6 family (unvalidated) — omit if you don't support it
KERNEL=="hidraw*", ATTRS{idVendor}=="37d7", ATTRS{idProduct}=="2502", TAG+="uaccess"
# CD2 charging dock (only if you drive the dock)
KERNEL=="hidraw*", ATTRS{idVendor}=="37d7", ATTRS{idProduct}=="6001", TAG+="uaccess"
# Screen-chip bootloader CDC tty after command 31 (only if you implement screen upload)
KERNEL=="ttyACM*", ATTRS{idVendor}=="ffaa", ATTRS{idProduct}=="5555", TAG+="uaccess"
# (openflydigi also has /dev/uhid and virtual-DualSense input rules — not needed for this app)
KERNEL=="uhid", TAG+="uaccess"
SUBSYSTEM=="input", ATTRS{name}=="Wireless Controller*", TAG+="uaccess"
```

- **The file number must be < 73.** `TAG+="uaccess"` is acted on by systemd's
  `73-seat-late.rules`. A `99-` file silently does nothing (udev/72-flydigi-apex5.rules:10-20;
  flydigi/setup.py:62-71).
- Some distros (Fedora/Aurora) already make hidraw world-accessible (udev/72-flydigi-apex5.rules:22-24).
- If the dongle enumerates under a different PID, the hidraw rule will not match. The dongle's PID
  is unknown (§1.1).
- The rule-file comment at udev/72-flydigi-apex5.rules:40-41 says the pad "leaves the HID bus"
  after command 31. HW measurement contradicts this: the HID nodes remain (PROTOCOL.md:695-704).

---

## 6. VIBRATION-RELATED (the app must never enable vibration)

Every item below can start, configure, test or persist rumble or trigger vibration. The only
vibration-related packet that is safe to emit is the explicit **stop** in row 1.

| # | command / field | what it does | modes / values that vibrate | evidence |
|---|---|---|---|---|
| 1 | **`0x12` (18) grip rumble** `03 5A A5 12 06 <L> <R> [<LT> <RT>]`, no checksum | drives the grip motors directly; SDL sends the identical packet. On Vader pads (`IsSupportTriggerVibration`), raw[7]/[8] drive **trigger motors** | any non-zero level. **The only allowed use is stop: `03 5A A5 12 06 00 00 00 00`**. Rumble appears to latch until stopped (the bench tool sends an explicit off; UNVERIFIED) | PROTOCOL.md:487, :228-232; flydigi/effects.py:377-388, :404-408; tools/flydigi_cmd.py:289-301 |
| 2 | **81 (0x51) SetForceTrigger**, mode **2** (`Sniper`; Space Station's UI label "Recoil"/机枪) | trigger motor **vibrates/rattles** past a travel point | mode 2: params `stroke, pressure, strength, frequency, match` | PROTOCOL.md:152-160, :197-214, :482; flydigi/effects.py:94-107, :289-294 |
| 3 | **81 SetForceTrigger**, mode **5** (`Vibration`) | dead code in Flydigi's stack. HW: buzzed with the pad's own grip bind in place | mode 5 | PROTOCOL.md:220-238; flydigi/effects.py:318-336 |
| 4 | **82 (0x52) SetForceTrigger SyncWithGrip** `03 5A A5 52 0B side bindType filter scale stroke pressure strength freq`, no checksum | **routes the game's grip rumble into the trigger motors** ("Tier-1 vibration bind"). The bind is **live state that persists** with no host running and **survives a config apply** | `bindType=2` binds; `0` appears to mean no bind. Suppression seen: bindType 2, filter 255, scale 0, zero params | PROTOCOL.md:161-163, :240-249, :486, :509-513; flydigi/effects.py:339-352 |
| 5 | **Space Station's/openflydigi's "engage stored" replay** | after a profile read or apply it re-sends the profile's stored trigger block. **Stored type 5 is sent as command 82**, stored type 2 as an 81 mode-2 command | **Never replay stored effects** of type 2 or 5 | flydigi/effects.py:391-469; PROTOCOL.md:328-333; docs/device-settings.md:318-323 |
| 6 | profile blob trigger block, offset **185** (20 bytes per side; `[0]` mode, `[1]` bindType (2 = Vibration), `[2]` filter, `[3]` scale, `[4..8]` bind params) | stores Vibration/Sniper effects in flash (mapping spec) | mode 2 or 5, bindType 2 | PROTOCOL.md:315-364 |
| 7 | gamelist "Tier-1 vibration" presets (`vibType`/`vibFilter`/`pwmScal`/`vibParams`) applied via 82; openflydigi's daemon fans this **out to every attached pad** | game-preset vibration bind | all | flydigi/effects.py:472-497; flydigi/identity.py:137-144; flydigi/registry.py:244-269 |
| 8 | **command 18 trigger-motor bytes** (`VibrationCommandFactory` `VibrationType.Trigger/Both`, raw[7]/[8]) | Vader-family trigger motors; not k5 hardware | any | PROTOCOL.md:228-232; docs/findings-other-devices.md:113-116 |
| 9 | **legacy XInput command 48 sub 14** "test vibration" (envelope `A5 30 0E …`, 15 bytes, no checksum) | factory vibration test; legacy XInput dialect only, not NewXInput | n/a | docs/device-settings.md:206-220 |
| 10 | XInput 48 sub 6 / sub 8 | force trigger / grip bind in the legacy dialect | | docs/device-settings.md:212-220 |
| 11 | **K6 family 83–87** (mode, local mode, waveform, strength mapping, realtime with `(Trigger, LeftGrip, RightGrip)` samples) | Apex 6 trigger and grip haptics; gated to k6 and **not for k5** | all | PROTOCOL.md:259-313 |
| 12 | Xbox-interface rumble via the kernel xpad driver (evdev `EV_FF` on interface 1.0) | grip rumble outside the vendor protocol | any FF upload | docs/device-settings.md:78-80 (xpad bound); the app must not open evdev for FF |
| 13 | DualSense relay or haptic-audio bridges (openflydigi `relay.py` / `haptics.py`) | convert DS5 rumble and haptic audio into command 0x12 | all | docs/findings-haptics.md:34-42; flydigi/haptics.py:5 |
| 14 | lighting blob byte 9 `grip_sync` ("Vibration light effect") | LED dims while the grip motor runs. Does not cause vibration (lighting spec) | n/a | docs/device-settings.md:386-402 |

**Non-vibrating trigger modes** (resistance only): 81 mode 0 Normal (clears), 1 Race, 3 Recoil
(UI "Sniper"), 4 Lock (PROTOCOL.md:477-484). These belong to another spec.

**Recommended policy:**
- Whitelist outgoing command ids.
- Emit 0x12 only from an explicit user-run rumble test: grip motors only (raw[7]/[8] = 0), under a
  second, always followed by the stop packet. Apex 5 Control does this (`device::rumble`).
- Never emit 82, 81 mode 2/5, or 83–87.
- Never replay stored trigger effects.
- 0x12 in SDL's form (length 6, no checksum) is the only allowed vibration-family packet.

---

## 7. DANGEROUS (firmware, bootloader, factory reset, brick or strand risk)

| cmd | what | risk | evidence |
|---|---|---|---|
| **31 (0x1F) SwitchToFirmwareUpgradeMode** `03 5A A5 1F 03 <chipModule> <crc>` | puts **one named chip** into upgrade mode. `ChipModule`: Main 0, Rf 1, **Si 2**, Screen 4, Trigger 5, Dongle 6, Adc 7, Led 8 | **HIGH / BRICK.** Any chip other than 4 hands a program chip to a vendor bootloader (Megahunt, NearLink, Jieli) with no implemented protocol and no recovery. Even chip 4 **over the dongle strands the pad** until it is power-cycled. Only `chip=4`, wired, as part of §4.4 | docs/findings-other-devices.md:733-767; docs/findings-screen.md:10-28; flydigi/screen_ota.py:139-156 |
| serial OTA opcodes 3/5/12 on `ffaa:5555` | erase and write the screen chip's flash | **MEDIUM** (picture region only, address from device). **Never** send `ScreenUpgradeType.PROGRAM`/`SYS_PIC`. Do not power off during the ~15 s post-write sync. Never more than 255 frames | §4.4; PROTOCOL.md:746-749, :801-806 |
| **253** factory reset | named as "factory reset" in the effect-envelope blacklist; **payload layout not in the source** | **HIGH**, unknown | docs/findings-steam.md:549-552 |
| **175** reset all mapping configs | `cfgId` is ignored; **all four profiles** return to factory (names, macros, remaps). 10 s flash op | **HIGH (data loss)**, mapping spec | docs/device-settings.md:241-246 |
| **166 / 171** flash commits | write the running profile to flash or a Switch-bank slot | MEDIUM (data), mapping spec | PROTOCOL.md:842-843, :871-875; docs/findings-steam.md:549-551 |
| **27** mode switch (Switch/Xbox/Flashplay/DInput) | changes the descriptor and USB identity (Switch mode = `057e:2009`, no vendor page) | **HIGH (strand)**: "one-way trip until proven otherwise", never sent | docs/findings-other-devices.md:687-690; docs/device-settings.md:472-477 |
| **29** restart | reboots the pad | LOW–MEDIUM; **never sent on HW**; the node disappears | docs/device-settings.md:20-24 |
| **242** TestScreen `03 5A A5 F2 06 on R G B crc@[9]` | floods screen **and RGB LEDs**; `on=0` does **not** clear it. Factory test | **MEDIUM (stuck state)**: only exit is the pad's physical power switch | PROTOCOL.md:655-668; docs/findings-screen.md:47-51 |
| **245** TestLed (NewXInput) | ACKs, no effect. **On the DInput dialect id 245 is upgrade mode**: never send 245 outside the `5A A5` envelope | LOW / HIGH by dialect | docs/device-settings.md:327-332; docs/findings-other-devices.md:739-742 |
| **208–211** HID picture family | inert for display. **211 commits metadata and destroyed the stored custom image**; 208 alone leaves an announced-unsent frame; it also sets the status-bar flag | MEDIUM (data) | PROTOCOL.md:552-565; flydigi/screen.py:365-388 |
| **20** report rate | pad reports an undocumented `0`; a write could lose a working rate | MEDIUM, never written on HW | flydigi/settings.py:249-259 |
| **17** with `controller_data=0`, or keyboard/mouse flags | blinds the xpad gamepad; keyboard/mouse flags are never written by Flydigi | MEDIUM (reversible) | docs/findings-steam.md:186-201 |
| **19 sub 2** Xbox home | unmeasured; SDK refuses it on NewXInput | LOW, do not expose | docs/device-settings.md:179-204 |
| **48 sub 11/12** (legacy XInput) | upgrade mode for screen/main and trigger chips | HIGH, legacy dialect only | docs/device-settings.md:212-220 |
| dock **224 / 254** (on `37d7:6001`) | dock firmware upgrade mode / rewrite device type | HIGH (no documented recovery) | PROTOCOL.md:1023-1024; docs/findings-other-devices.md:456-459 |
| **163** read profile | side effect: **switches the active profile** on the first reply packet | LOW (state), mapping spec | PROTOCOL.md:865-869 |

**Documented recoveries** (Space Station UI text, PROTOCOL.md:808-822):
- Failed screen upload: slide the power switch on the back.
- SI-chip flash gone wrong: **hold START (lower right of LOGO) 8 s**, which recovers that one chip
  only (docs/findings-other-devices.md:771-775).
- Real firmware updates: use real Windows hardware, not a VM, because a USB-passthrough
  re-enumeration mid-flash is the classic brick (docs/findings-other-devices.md:767-769).

**Recommended outgoing whitelist** for a safe settings/tester app:

| command | allowed form |
|---|---|
| 1, 2, 3, 4, 16 | reads |
| 17 | only the raw_data flag, others `0xFF`; optionally third_party |
| 19 | subs 1, 4, 5, 6, 7, 8, 9 |
| 21, 22, 23 | numeric settings |
| 24 | nickname |
| 0x12 | grip motors only (length 6, trigger bytes 0); the rumble test stops it within a second |

- Everything else is behind an explicit "danger" gate, or absent.
- Command 31 is allowed only as the hard-coded `chip=4` step of a wired screen upload.
