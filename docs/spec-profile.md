# Flydigi Apex 5 mapping profile and macro protocol specification

Status: extracted by **read-only source analysis** of the open-source `openflydigi` project (Python,
Apex 5 `37d7:2501`). No command in this document was sent to hardware during extraction.
"Hardware-verified" below means *the openflydigi source or docs claim it was measured on a real
Apex 5*; it is not a claim made by this extraction.

Scope: mapping-profile commands **161, 162, 163, 164, 165, 166, 171** (and the related destructive
**175**), the **840-byte profile blob**, and **macros**: the protocol 3.1 page inside the profile,
which the Apex 5 uses, and the protocol 3.2 store behind commands 172, 173 and 174, which the Apex 5
does not use. Lighting (167/168/169) is out of scope and is already implemented.

## 0. Citation keys

All paths are relative to the openflydigi checkout root.

| Key | File |
|---|---|
| `M` | `flydigi/mapping.py` |
| `B` | `flydigi/blobs.py` |
| `D` | `flydigi/device.py` |
| `E` | `flydigi/effects.py` |
| `MAC` | `flydigi/macros.py` |
| `FC` | `flydigi/factory_config.py` |
| `ID` | `flydigi/identity.py` |
| `P` | `PROTOCOL.md` |
| `F` | `docs/findings-profile-blob.md` |
| `DS` | `docs/device-settings.md` |
| `W` | `gui/worker.py` |
| `GP` | `gui/models/profile.py` |
| `MP` | `flydigi/mock/pad.py`, the fake pad the tests run against (`tests/fake_pad.py` re-exports it) |
| `TM` | `tests/test_mapping.py` |
| `CLI` | `tools/flydigi-mapping` |

Offsets in this document are **decimal** unless written `0x..`. Byte ranges `a..b` are
**half-open**, so `a..b` means bytes a through b-1 (F:6-7).

---

## 1. Transport recap, with the indexing used in this document

The outgoing envelope is already known: 32 bytes, with `[0]=0x03`, `[1]=0x5A`, `[2]=0xA5`,
`[3]=cmd`, `[4]=payload_len+2`, `[5..]=payload`, `[3+len]=sum8([3..3+len))`, and the remaining
bytes zero (B:27-39, D:114-116, D:244-252). Every command in this document is checksummed with
this rule except **171**, which is a quirk (see §2.7). **A bad checksum is answered with silence,
not an error** (P:888, B:14-15, TM:344-351).

**Reply indexing.** Replies arrive as input report `0x04`. This document gives two indexes:

* **raw[i]**: index into the report as read from hidraw, *including* the report-id byte. raw[0]=0x04.
* **body[i]** = raw[i+1]: report-id stripped. This is what openflydigi's code and Flydigi's SDK
  (`data[]`) index. `B.replies` strips the byte (B:51), and `P` §9 uses `data[]` in this sense.

Common reply header: raw[1]=0x5A, raw[2]=0xA5, **raw[3]=echoed command id** (body[2]).

* **Matching replies.** openflydigi matches a reply on `len(reply) > 7 && raw[3] == cmd`
  (B:54-67). Command replies share report id 0x04 with the streaming input reports, which carry
  **0xEF at raw[3]**, and no command id collides with that value (B:60-62, `motion.py`:26).
* **Success flag.** The generic ACK convention (`ParseAckData`) is raw[6] (body[5]) == 1
  (P:472-473, D:453-459). **openflydigi does not check the success flag for 162, 164, 165, 166, 171
  or 175**. It accepts any reply that echoes the command id (M:665-669, M:711-715, M:851-855,
  B:91-100). The fake pad answers these with raw[4]=1 and raw[6]=1 (MP:439-453). What a real pad
  sends on failure is **unknown**.
* **Replies are broadcast to every process reading the hidraw node.** Drain stale input before
  writing, and hold an exclusive lock for whole sequences (P:500-506, D:341-410). openflydigi uses
  `flock(2)` on the node with a 5 s acquire timeout (D:102, D:380-391).
* **A sleeping pad leaves the USB bus entirely**, and the node disappears (D:219-222).

---

## 2. Commands

### 2.0 Summary

| Cmd | Hex | Name (SDK) | Payload | Len byte `[4]` | Checksum at | Reply |
|---|---|---|---|---|---|---|
| 161 | 0xA1 | status / `ReadCurrentMappingConfigId` | none | 2 | raw[5] | one packet: active slot and 4 version tags |
| 162 | 0xA2 | apply / `ApplyMappingConfigByCfgId` | `[cfgId]` | 3 | raw[6] | ACK |
| 163 | 0xA3 | read profile | `[cfgId, pkgSize=20]` | 4 | raw[7] | 42 packets |
| 164 | 0xA4 | write start | `[cfgId, startIdx, nPkts, pkgSize=20]` | 6 | raw[9] | ACK |
| 165 | 0xA5 | write pack | `[pktNum, 20 data bytes]` | 23 | raw[26] | ACK per packet |
| 166 | 0xA6 | save / `SaveCurrentMappingConfig` (`PermanentSaveMappingConfig`) | `[verLo, verHi]` | 4 | raw[7] | ACK |
| 171 | 0xAB | save running profile to a Switch slot (`SaveCurrentSwitchMappingConfig`) | `[verLo, verHi, cfgId]` | **4 (quirk)** | **raw[8], sum over [3..7)** | ACK |
| 175 | 0xAF | reset **all** profiles (`ResetMappingConfigByCfgId`) | `[cfgId]`, which is ignored | 3 | raw[6] | ACK |

Sources: M:16-19, M:63-70, P:835-846, B:27-39, M:810-830.

Example frames, computed with the stated rules. Bytes after the checksum are 0x00 up to 32 bytes.

```
161            03 5A A5 A1 02 A3
162 cfg=1      03 5A A5 A2 03 01 A6
163 cfg=2      03 5A A5 A3 04 02 14 BD
164 0,0,42,20  03 5A A5 A4 06 00 00 2A 14 E8      (full write, slot 0)
164 1,5,1,20   03 5A A5 A4 06 01 05 01 14 C5      (one packet, #5, slot 1)
166 v=0x1234   03 5A A5 A6 04 34 12 F0
171 cfg=5 v=0x5321  03 5A A5 AB 04 21 53 05 23   (asserted byte-exact by TM:1502-1517)
175            03 5A A5 AF 03 00 B2
```

### 2.1 Timeouts

| Cmd | Space Station reference | openflydigi uses |
|---|---|---|
| default, every command | 500 ms, 3 retries (P:96-99) | — |
| 161 | 500 ms | 1.0 s (M:579) |
| 162 | 500 ms | 0.5 s (M:663) |
| 163 | 500 ms | 1.5 s per attempt, 3 attempts; each attempt resends 163 (B:103-136, M:600) |
| 164 / 165 | 500 ms | 0.5 s per packet, with no retry; a missing ACK aborts the transfer (B:143-183) |
| **166** | **10 000 ms** (flash write) (P:97, P:842) | 2.0 s (M:697). The docstring says "the pad takes seconds", so **use 10 s** |
| 171 | 10 000 ms | 10 s (M:833) |
| 175 | 10 000 ms | 10 s (M:783) |

Every wait is a ceiling. Stop collecting as soon as the expected reply arrives (B:42-51, D:412-448).

### 2.2 Command 161: status (no side effects)

Request: `03 5A A5 A1 02 A3`.

Reply, a single packet (M:587-597, P:848-855, mock reply MP:609-618):

| raw | body | Meaning |
|---|---|---|
| 3 | 2 | 0xA1 |
| 4, 5 | 3, 4 | not read by openflydigi; the mock writes raw[4]=1 and raw[5]=0 |
| **6** | **5** | **raw active slot**. 0..3 means XInput bank slot N. 4..7 means the Switch bank is active, and the logical slot is N-4. openflydigi normalises: `active = raw-4 if 3 < raw <= 7 else (raw if raw <= 7 else 0)` (M:591-594) |
| 7+2i, 8+2i | 6+2i, 7+2i | slot i's **data_version**, uint16 **little-endian**, for i=0..3 (raw 7..14). Code: `(body[7+2i] << 8) \| body[6+2i]` (M:595) |

* `0xFFFF` = the slot has never been written. A factory reset (175) also returns every tag to
  0xFFFF (P:853-854, DS:242-245).
* The tags are observed to be random change-detection values (23224, 65078, 65535), not counters
  (P:862-863).
* Whether the reply carries more than 4 tags is unknown. openflydigi reads 4 (M:579).
* Has **no side effect** (P:848, M:582-583).

### 2.3 Command 162: apply, which switches the running profile

Payload `[cfgId]`, where cfgId is 0..3. The SDK's `ApplySwitchConfigAsync` also accepts 4..7
(M:72-77, DS:461-464). Reply: ACK (M:663-669).

Side effects:
* Makes `cfgId` the running ("active") profile.
* **The firmware re-parses the macro page only on load.** A macro written with 164/165 is stored
  and **not played until 162** (or a 163 read) pages the profile in (P:901-908, F:590-593,
  M:1437-1444). Remaps in the key table take effect as soon as the packet lands (F:115).
* **Audibly re-seats the trigger motors** (the force-trigger actuators). A read or apply also
  appears to drop the live trigger effect: "the read could return Lock while the physical trigger
  had already gone loose" (CLI:78-83). Stored trigger effects are **not** engaged by 162. They need
  live command 81/82 (§9, P:328-333, E:414-421).
* The live 82 vibration bind **survives** a config apply (P:247-248).

### 2.4 Command 163: read profile. SIDE EFFECT: it switches the active slot

Payload `[cfgId, 20]`. The pad streams `total` packets back to back without further requests
(B:103-136).

Per-packet reply layout (B:120-125, MP:620-633):

| raw | body | Meaning |
|---|---|---|
| 3 | 2 | 0xA3 |
| 4 | 3 | `total`, the number of packets (42 for a profile) |
| 5 | 4 | `index`, 0-based |
| 6 | 5 | cfgId |
| 7..27 | 6..26 | 20 data bytes, which belong at blob offset `index*20` |

* Collect until `len(distinct indices) >= total`. Packets may be reassembled out of order (P:885).
  If any are missing after the timeout, retry the whole read by resending 163 (B:115-135).
* **Side effect: 163 makes cfgId the running profile.** It switches on the **first reply packet**,
  even when the read later fails (P:865-869, M:607-612, M:644-650). After reading slot 2, 161
  reports 2 as active (M:608-609).
* **Aliasing**: reading cfgId 4..7 returns slot 0..3 byte-for-byte, with the same title and tag.
  The Switch bank is **write-only** from the host (DS:468-471, DS:472-476).
* Profile size: **840 bytes = 42 packets × 20**. **Do not size it from blob byte 2**, which reads
  77 on an Apex 5 (F:41-46, M:1169-1174).

#### Safe read that preserves the running slot (M:631-660)

```
prev = status(161).active        # decide BEFORE reading; may be None if no reply
if prev is None or prev == target:
    return read(163, target), restored=None
try:
    cfg = read(163, target)      # may raise; pad has switched anyway
finally:
    apply(162, prev)             # ALWAYS, even if the read failed
return cfg, restored=prev
```

Reasons: the switch happens before the reader knows whether the read succeeded. A retry
"launders" it: after a failed attempt, 161 truthfully reports `target` as active, so deciding the
restore after the read would skip it (M:644-650).

Caveats:
* The restoring 162 re-seats the trigger motors, and live trigger effects of the restored profile
  are not re-engaged by this function. The CLI re-engages after every read (CLI:73-85).
  **Vibration impact: see §9.**
* openflydigi's desktop app deliberately does **not** browse this way. It opens a profile by
  switching to it, because 166 can only save the running profile (M:638-642, W:358-387).

### 2.5 Commands 164 and 165: write (partial, diffed)

Payload 164: `[cfgId, startIdx, nPkts, 20]`. Payload 165: `[pktNum, d0..d19]`.

Algorithm (B:143-183, M:858-880):
1. Split the new and old blobs into 20-byte packets. Compute **contiguous runs of changed
   packets**. If there is no old blob, or the lengths differ, send all packets as one run starting
   at 0.
2. For each run: send 164 `[cfgId, run_start, run_len, 20]` and wait for the ACK. Then send, for
   i = 0..run_len-1, 165 `[i, packet[run_start+i]]` and wait for each ACK.
   **`pktNum` is relative to the run (0-based), not absolute.** The fake pad writes to
   `start + pktNum` (MP:704-714), and hardware writes made this way read back identical (F:286-287).
3. A missing ACK aborts the transfer with an error (B:177-181).
4. **No other command may be interleaved inside a run.** The pad is tracking the announced packet
   count, so hold the exclusive lock across the whole transfer (B:171-174, DS:341-344).

Side effects:
* Key-table changes take effect as soon as the packet lands (F:115).
* Macro changes need 162 afterwards (§2.3).
* Changes are in **working memory only**. They are lost at the pad's next **sleep** (not only
  power-off) unless committed with 166 (P:873-875).
* Writing to a cfgId that is **not** the running profile is **unmeasured** in the source.
  openflydigi always writes the profile it has just read, which makes it the running one (W:358-387,
  W:550-594). Do the same.

### 2.6 Command 166: save the running profile to flash, and data_version rules

Payload `[verLo, verHi]`, a uint16 LE. Reply: ACK. Timeout: **10 s** (§2.1).

* **166 commits whichever profile the pad is running**, not a slot that you name (P:871-872,
  GP:2389-2402). Before sending 166, confirm with 161 that the active slot equals the slot you
  edited. openflydigi refuses to save otherwise (`canSaveToFlash`: open cfgId == active,
  GP:2389-2402, GP:2569-2584).
* The **version** you pass becomes that slot's cache tag, reported by 161 and stored in the blob at
  **offset 225..227 LE** (M:95, M:700-701, P:856-857).
* **data_version tag rules** (M:672-694, P:858-863, W:569-578, CLI:174-186):
  * After any **mapping change** (remap, macro, stick, title, and so on), send a **fresh random
    tag**: `random in [0, 0xFFFF)`, i.e. 0..65534, that is **≠ the current tag**. Space Station loops
    `Random.Next(65535)` until the value differs.
  * **Never send 0xFFFF.** It means "never written".
  * Reusing the old tag writes the change but tells other apps their cached copy is still current.
    This was measured: a rename did not show up elsewhere.
  * Passing 0 is "a lie of a different shape". What the pad does with 0 is unconfirmed (M:705-708,
    P:861).
  * **Lighting-only save**: send the **current** tag of the active slot, read via 161, because the
    mapping profile did not change (W:632-652, DS:437-443).
  * Update the local copy's blob[225..227] **only after** the 166 ACK (P:860, W:577-578).
* 166 also commits macros (on v3.1 they are in the blob) and, per the reference ordering, any
  macro store written before it (MP:655-663, P:960-962).

### 2.7 Command 171: save the running profile into a Switch-bank slot

This is command 166 with a destination (DS:465-467). Payload `[verLo, verHi, cfgId]`, where cfgId
is **4..7 only** (M:833-855). openflydigi refuses anything else, because aiming 171 at an XInput
slot is **unmeasured** and could silently overwrite a profile (M:839-843, P:872-873).

**Frame quirk, reproduced literally** (M:810-830, TM:1502-1517):
```
buf[3]=171  buf[4]=4 (sic: same as 166, though 3 payload bytes)
buf[5]=verLo  buf[6]=verHi  buf[7]=cfgId (where 166 would put its checksum)
buf[8]=sum8(buf[3..7))      (excludes cfgId)
```
The pad ACKs this exact frame. A "corrected" frame is a different, untested packet.

* Source = the running profile. Read (163) the XInput profile first so that it is running
  (W:451-496).
* The Switch bank is distinct storage. A measured run modified profile 3 in RAM and committed it
  to slot 6: the flash copy of profile 3 was unchanged, and Switch mode used the new mapping
  (DS:478-481).
* In Switch mode the pad enumerates as `057e:2009` with no vendor interface, so the Switch bank
  cannot be read back (DS:472-476).
* Before 171, openflydigi normalises the running profile (M:1258-1305): every key whose target is
  254 (keyboard) is reset to identity, and every stick with center > 100 is reset to center=0. It
  writes this to RAM with 164/165, sends 171, and then writes the original back (no 166)
  (W:466-489).
* Use a fresh tag from `next_data_version` (W:477-478).

### 2.8 Command 175: reset ALL profiles (destructive; do not expose casually)

The cfgId argument is **ignored**. All four profiles return to factory, including the factory
Chinese titles `配置1..4` and tags of 0xFFFF. This was measured (M:783-807, DS:241-246). openflydigi
sends 0. Timeout 10 s.

**Single-slot restore** = write the model's factory blob into the running slot with 164/165, then
166 with a fresh tag (M:725-780, DS:248-253). The factory blob is in §3.13.

---

## 3. The 840-byte profile blob (protocol v3.1, Apex 5)

### 3.0 Region map

| Range | Size | Field | Read/played by firmware? | Source |
|---|---|---|---|---|
| 0..2 | 2 | `ProtoVersion` uint16 LE. Apex 5 = **769 (0x0301) = v3.1** | header | M:37, F:647-649 |
| 2 | 1 | "package count": **77 on Apex 5**. Do not use it | — | F:41-46 |
| 3..13 | 10 | `OldLedConfig`, a legacy LED mirror. Preserve it | unknown | DS:491-495 |
| 13..109 | 96 | key table, 32 × 3 | **yes, live on write** | §3.1 |
| 109..123 | 14 | joystick core curve, 2 × 7 | **no (inert)**; source form for the bank | §3.3 |
| 123..137 | 14 | trigger travel curve, 2 × 7 | **yes** (travel window) | §3.4 |
| 137..145 | 8 | gyro-to-stick mapping | **yes** | §3.5 |
| 145..154 | 9 | grip vibration | yes (PROGRESS: hardware-validated) | §3.6 **VIBRATION** |
| 154..183 | 29 | trigger vibration motors | **no motors on Apex 5** | §3.7 **VIBRATION** |
| 183..185 | 2 | wheel (`Lunpan`), factory `0 0` | unknown | F:720 |
| 185..225 | 40 | stored force-trigger effect, 2 × 20 | stored only; engaged by live 81/82 | §3.8 **VIBRATION-relevant** |
| 225..227 | 2 | `DataVersion` uint16 LE (the cache tag) | tag | §2.6 |
| 227..230 | 3 | unaccounted, factory FF FF FF | ? | F:48-50 |
| 230..768 | 538 | macro page (v3.1) | yes, on load (162) | §4 |
| 768..770 | 2 | unaccounted, factory FF FF | ? | F:48-50 |
| 770..790 | 20 | title, UTF-16LE, max 10 code units | — | §3.9 |
| 790..814 | 24 | joystick extra, 2 × 12 (9-point bank, isRound, edge) | **bank and isRound yes**; edge byte no | §3.3 |
| 814..820 | 6 | unaccounted, factory FF×6 | ? | F:48-50 |
| 820..825 | 5 | macro repeat interval per slot (v3.1 only) | yes | §4 |
| 825..830 | 5 | unaccounted, factory FF×5 | ? | F:48-50 |
| 830..836 | 6 | gyro response curve | **no (inert)** | §3.5 |
| 836..840 | 4 | padding, factory FF×4 | — | F:80 |

Regions 790, 820 and 830 are written by Flydigi's `MappingConfigParserV31` and exist only for
`(proto & 0xF) >= 1`. 820 is written only when `ProtoVersion < 770` (F:651-667).

**Rule: carry every byte you do not interpret through unchanged** (M:53-54). That covers 3..13,
183..185, the mixed-border bytes 194 and 214, the unaccounted ranges, and the second trigger-motor
gear.

Packet map (packet k = bytes 20k..20k+20): key table = packets 0-5; curves and motion = 5-6;
grip vibration and trigger motor = 7-9; force trigger = 9-11; data version = 11; macro page =
11-38; title = 38-39; joystick extra = 39-40; cycle and motion curve = 41.

Enable-flag convention in this blob: **0 = ON, 0xFF = OFF** (inverted) (M:309-310). The one
exception is described in §3.7.

### 3.1 Key table: 13..109

32 entries × 3 bytes, indexed by ControllerKey id: `offset = 13 + 3*keyId` (M:315-321, M:1317-1321).

| Entry byte | Meaning |
|---|---|
| +0 | **target** (see the target codes below) |
| +1 | turbo mode: 0 = off, 1 = while held, 2 = toggle (M:323-326) |
| +2 | turbo frequency: **0 = turbo off**. The unit is unverified. The GUI calls it Hz and caps it at 40 (GP:48, GP:1096-1098); the backend clamps it to ≤255 (M:1389-1390) |

#### ControllerKey ids (M:330-337), copied verbatim

| id | name | Apex 5 key? (M:386-394) | XInput remap target? (M:421-422) | GUI label (GP:76-84) |
|---|---|---|---|---|
| 0 | up | yes | yes | Up |
| 1 | right | yes | yes | Right |
| 2 | down | yes | yes | Down |
| 3 | left | yes | yes | Left |
| 4 | a | yes | yes | A |
| 5 | b | yes | yes | B |
| 6 | select | yes | yes | Select |
| 7 | x | yes | yes | X |
| 8 | y | yes | yes | Y |
| 9 | start | yes | yes | Start |
| 10 | lb | yes | yes | Left bumper |
| 11 | rb | yes | yes | Right bumper |
| 12 | lt | yes | yes | Left trigger |
| 13 | rt | yes | yes | Right trigger |
| 14 | thl | yes | yes | Left stick click |
| 15 | thr | yes | yes | Right stick click |
| 16 | c | no (Vader only) | no | — |
| 17 | z | no (Vader only) | no | — |
| 18 | m1 | yes (back paddle) | **no** (source only) | M1 |
| 19 | m2 | yes (back paddle) | no | M2 |
| 20 | m3 | yes (back paddle) | no | M3 |
| 21 | m4 | yes (back paddle) | no | M4 |
| 22 | m5 | yes (shoulder "LM") | no | LM |
| 23 | m6 | yes (shoulder "RM") | no | RM |
| 24 | menu (silkscreened "Fn"; switches profiles) | not offered | no | — |
| 25 | turbo (arms turbo modes) | not offered | no | — |
| 26 | *(unnamed in source)* | — | — | — |
| 27 | home (Guide) | yes. Measured as both a working source and a working target (M:367-376) | yes | Home |
| 28 | back | not offered; meaning not documented | no | — |
| 29-31 | *(unnamed in source)* | — | — | — |

Apex 5 remappable list, in UI order (M:386-394): `a b x y up down left right lb rb lt rt thl thr
select start home m1 m2 m3 m4 m5 m6`.
XINPUT_TARGETS (M:422) = the same list minus `m1..m6`, `c` and `z`: `a b x y up down left right
lb rb lt rt thl thr select start home`.
Why M1-M6 are never offered as targets: they have no XInput id, so A→M1 would silence A
(M:415-420). `set_mapping` itself accepts any id. The restriction is enforced in the UI and CLI
(GP:37).
Stick bodies JsLeft=240 and JsRight=241 are UI ids only. They are not in the key table (M:378-385).

#### Target byte codes (M:318-321, M:1323-1346)

| Value | Meaning |
|---|---|
| 0..28 | send that ControllerKey (table above). target == own id is normalised to 255 on write (M:1383-1384) |
| **32** | `TARGET_MACRO`: the key runs its macro (§4) |
| **254** | `TARGET_KEYBOARD`: keyboard/mouse/multi-function. **No keyboard code is stored anywhere in the blob**. Both companion bytes are zeroed. On hardware, 254 makes the pad **suppress** that button's gamepad output; on Windows a Flydigi kernel filter driver injects the keystroke. **On Linux a key set to 254 does nothing** (M:1269-1283). So there is **no keyboard keycode table** in this protocol |
| **255** | `TARGET_IDENTITY`: the key does its own job (factory value for all 32 entries) |
| any other value > 32 | firmware treats it as identity (measured with 200) (M:1338-1345) |
| 26, 29-31 | unknown |

Read rules (M:1323-1346): if target == 32, the key is a macro (turbo ignored). Else if frequency > 0,
it is a turbo on `target`. Else if target == 254, it is keyboard. Else if target > 32, it is
identity.

Write rules (M:1348-1392):
* `None` or own id → 255.
* If frequency > 0: turbo needs a real target, so 255 or 254 is replaced by the key's own id. The
  entry is `[target, mode, min(255,freq)]`.
* If frequency == 0: the entry is `[target, 0, 0]`. The mode byte is dropped, so a turbo mode with
  frequency 0 is a no-op (F:110-113).
* **Remapping a key away from TARGET_MACRO must delete its macro body** (see §4.4, orphaned bodies).

### 3.2 Back paddles and M buttons

There is no separate block for them. M1-M4 (ids 18-21, rear paddles) and M5/M6 (ids 22-23,
shoulder LM/RM) are ordinary key-table **sources** (§3.1). An unmapped M-key emits nothing
(DS:483-485). Factory: all 255. A macro can be triggered by any key, paddles included (§4).

### 3.3 Sticks: 109..123 (core) and 790..814 (extra / 9-point bank)

**Core block**, 7 bytes per stick, on a 0..127 scale (M:255, F:119-127). L = 109, R = 116.

| +i | Abs L / R | Field | Range | Notes |
|---|---|---|---|---|
| 0 | 109 / 116 | type `JoystickSensitivityType` | 0 Default, 1 Quick (UI "Instant"), 2 Slow (UI "Delay"), 3 Custom | also written into extra +0 (M:2063-2075) |
| 1 | 110 / 117 | center (inner dead zone / "Offset") | written 0..100. **127 = sentinel "stick is not acting as a stick"** (mapped to kbd/mouse/dpad on the host) | M:280-306 |
| 2,3 | 111-112 / 118-119 | p1.x, p1.y | 0..127 | interior breakpoint |
| 4,5 | 113-114 / 120-121 | p2.x, p2.y | 0..127 | |
| 6 | 115 / 122 | end | factory 127. **Read-only**: no Flydigi app writes it, and their reader corrupts values > 127 (M:2054-2061) | |

Factory: `0 0 63 63 127 127 127` per stick (F:65).

**Extra block** `m_fdg_macro_joy_extra_v2_struct_t`, 12 bytes per stick. L = 790, R = 802
(M:256-257, F:128-134).

| +i | Abs L / R | Field | Range |
|---|---|---|---|
| 0 | 790 / 802 | type (copy of the core type) | 0..3 |
| 1..10 | 791-799 / 803-811 | **bank[9]**: output at x = 0, 12.5, …, 100 % travel, stored as `output% + 50` | **0..150** (50 = none, 150 = full; <50 occurs for dead zones) |
| 10 | 800 / 812 | isRound `JoystickCircularityType` | 0 Rectangle (factory), 1 Circular |
| 11 | 801 / 813 | edge (outer dead zone; UI "Edge") | written 0..100 |

Factory: `0 | 50 62 75 87 100 112 125 137 150 | 0 | 0` (F:74-75).

**What the firmware plays (hardware-verified, F:136-160):** only **bank[9]** and **isRound**.
Flattening the bank to all 50 silences the stick. The core polyline, the type byte, center, and
the edge byte are **inert**: they are the source form the host compiles into the bank. A UI must
recompile the bank after every edit.

isRound measured: Rectangle lets the diagonal reach magnitude 1.19, while Circular clamps it to
1.00, which is ≈0.71 per axis (F:213-228).

**Bank compiler, exact (M:496-576). Truncate, do not round**, which reproduces the factory bank
exactly (F:206-211):
```
fn stick_nodes(center, edge, p1, p2) -> Vec<(f64,f64)>:   // center, edge in %, p in 0..127
  start = if center > 0 { (center, 0) } else { (0, -center) }
  end   = if edge   > 0 { (100-edge, 100) } else { (100, 100+edge) }
  scale = 100/127; span = end.x - start.x
  if span <= 0 { return [start, end] }
  interior(p) = (start.x + span*(p.x*scale)/100, p.y*scale)
  [start, interior(p1), interior(p2), end]

fn along(nodes, x):
  for i in 0..len-1:
    (x0,y0)=nodes[i]; (x1,y1)=nodes[i+1]
    if x <= x1 || i == len-2:
      if x0 == x1 { return if x < x0 { y0 } else { y1 } }
      return y0 + (y1-y0)*(x-x0)/(x1-x0)       // extrapolates past both ends
  nodes.last.y

bank[i] = trunc_toward_zero(clamp(along(nodes, 100*i/8), -50, 100)) + 50,  i = 0..8
```
Check values: the identity line (center 0, edge 0, p1 63,63, p2 127,127) gives
`50 62 75 87 100 112 125 137 150`. A 60 % dead zone gives `[0,0,0,0,26,56,87,118,150]` (F:133).

**Stick presets** (M:273-278). Selecting a preset sets p1/p2 and zeroes center and edge
(M:2141-2150).

| type | p1 | p2 |
|---|---|---|
| 0 Default | (63,63) (pad value; the Flydigi app uses 64,64) | (127,127) |
| 1 Quick ("Instant") | (64,96) | (127,127) |
| 2 Slow ("Delay") | (64,32) | (127,127) |
| 3 Custom | keep the current values | |

Invariants enforced (M:2126-2188):
* center and edge accept **0..100 only**. Negative values ("Offset"/anti-dead-zone for center,
  output ceiling for edge) are **refused**, because Flydigi's reader folds bytes > 127 to
  `127-byte` while its writers use two's complement (M:287-306, F:256-262). Since the core is
  inert, an **anti-dead-zone can still be expressed in the bank**: `stick_nodes` supports
  `center < 0` → start = (0, -center). Only the core-byte encoding is ambiguous.
* **center + edge ≤ 100**, cross-clamped. The field being set gives way (M:2152-2163).
* Points are clamped 0..127. The bank must be **exactly 9** values, each clamped 0..150. Flydigi's
  writer overruns into isRound, edge and the other stick if given more (M:2094-2104).
* Editing any node (center, edge, p1, p2) without picking a preset sets type = 3 Custom in both
  blocks (M:2169-2171).
* Write the type into **both** blocks (M:2063-2075).
* If center > 100 (sentinel 127), compile the bank as if center were 0. Likewise edge > 100 → 0
  (M:2173-2177).

### 3.4 Trigger travel ("Stroke Setting"): 123..137 (live, hardware-verified)

7 bytes per trigger, on a **0..255 scale**. L = 123, R = 130 (M:1949-2006, F:291-335).

| +i | Abs L / R | Field | Factory |
|---|---|---|---|
| 0 | 123 / 130 | type. Never decoded. **Keep it** (factory 0) | 0 |
| 1 | 124 / 131 | zero = travel start | 0 |
| 2,3 | 125-126 / 132-133 | p1 = (zero, zero) | 0 0 |
| 4,5 | 127-128 / 134-135 | p2 = (end, end) | 255 255 |
| 6 | 129 / 136 | end = travel end (full) | 255 |

* The window moves the **physical** travel. The game still sees 0..255. Bringing `end` in makes a
  hair trigger. Measured: a 0..16 window gave 17 distinct values against 240 on the control side
  (F:306-315).
* Writer rules: clamp 0..255, sort so zero ≤ end (equal is allowed), and **mirror the points**:
  p1 = (zero, zero), p2 = (end, end). This is the only form Flydigi writes (M:1972-2006).
* The window has no effect while the trigger is under the Lock effect, which makes the axis
  digital (P:492-496).
* The look-alike `Normal` params at 195/196 and 215/216 are **inert**. Do not use them (F:317-319,
  P:346-356).

### 3.5 Gyro-to-stick mapping: 137..145 (live) and response curve 830..836 (inert)

`m_fdg_macro_motion_mapping_struct_t` (M:1661-1771, F:341-422):

| Abs | Field | Values | Factory |
|---|---|---|---|
| 137 | type `MotionMapType` | 0 Off, 1 LeftJoystick (UI "racing"), 2 RightJoystick (UI "aiming/shooting"), 3 Mouse (host-side only; do not offer it) | 0 |
| 138 | enable key `EnableKey[0]` | ControllerKey id; **255 = none** | **12 (LT)** |
| 139 | method `MotionEnableType` | 0 Click (toggle), 1 Press (hold) | 0 |
| 140 | dead zone | 0..100 | 4 |
| 141 | sensitivity x | 0..100 | 25 |
| 142 | sensitivity y | 0..100 (the writer sets x = y from one number; the reader takes max) | 20 |
| 143 | use mode `MotionUseMode` | 0 FPS, 1 Racer. Derived: left stick → Racer, right stick → FPS; unchanged when Off | 0 |
| 144 | second enable key `EnableKey[1]` | ControllerKey id; 255 = none. **Honoured on its own** (measured) | **0 (D-pad Up!)** |

Rules (M:1703-1771):
* Byte 144 is written **only when method == Press (1)**. Otherwise it is re-emitted unchanged
  (Flydigi's rule). The factory 0 means **D-pad Up also enables the gyro** once the mapping is
  turned on.
* Mouse branch: 138 = 255, 140 = 0, 144 = 255.
* There is no "always on" mode. The gyro is gated by its key.
* Measured on hardware: the pad drives the stick only while enabled, and Click toggles. Releasing
  the key **parks** the stick where it was rather than re-centring it (F:379-403). The Flydigi UI
  warns that enabling the mapping lowers the polling rate (F:365-367).

Response curve **830..836**: `zero, p1.x, p1.y, p2.x, p2.y, end` on a 0..127 scale, factory
`0 63 63 127 127 127`. **Measured inert.** Preserve it and never expose it (M:1773-1828, F:424-467).

### 3.6 Grip vibration: 145..154 (VIBRATION, see §8)

(M:1856-1892, F:469-474)

| Abs | Field | Range | Factory |
|---|---|---|---|
| 145 | master enable | 0 = ON, 0xFF = OFF | **0 (ON)** |
| 146 | left enable | 0 = ON, 0xFF = OFF | 0 (ON) |
| 147 | left min | 0..255 | 60 |
| 148 | left max | 0..255 | 255 |
| 149 | left scale | 0..255 | 50 |
| 150 | right enable | 0 = ON, 0xFF = OFF | 0 (ON) |
| 151 | right min | 0..255 | 80 |
| 152 | right max | 0..255 | 255 |
| 153 | right scale | 0..255 | 50 |

min and max are the window into which the pad clamps the game's rumble. The writer keeps
min ≤ max by swapping (M:1880-1884).

### 3.7 Trigger vibration motors: 154..183 (VIBRATION; absent on the Apex 5)

`IsSupportTriggerVibration` is false for the Apex 5 (`ID`:176-178, `trigger_motors: False`). Flydigi
never reads or writes this block on an Apex 5. It is **inert hardware-wise** (M:2190-2235,
F:476-504). It is present in the blob, with values.

| Abs | Field |
|---|---|
| 154 | shared enable for both sides: **0 = ON**. Flydigi's parser writes **1** for OFF; openflydigi writes 0xFF |
| 155..162 | left gear 1: type(155), min(156), max(157), filter/"block"(158), min_start(159), scale(160, 1..100 %), min_time(161) |
| 162..169 | left gear 2 ("micro"; never written by Flydigi; type byte 255) |
| 169..176 | right gear 1: type(169), min(170), max(171), filter(172), min_start(173), scale(174), min_time(175) |
| 176..183 | right gear 2 |

Factory: `0 | 1 30 80 5 1 50 0 · 255 40 120 5 0 50 0 | (same, right)` (F:70). The
factory enable is 0 (ON) on the measured pad. Some Flydigi default files say disabled (F:735).
**Leave the whole block untouched.**

### 3.8 Stored force-trigger effect: 185..225 (VIBRATION-relevant, see §8)

20 bytes per side. L = 185..205, R = 205..225 (M:1894-1947, P:315-368, F:506-532).

| +i | Abs L / R | Field |
|---|---|---|
| 0 | 185 / 205 | effect mode `AdapterTriggerType` (table below) |
| 1 | 186 / 206 | bind type: **2 iff mode == 5 (Vibration), else 0** (the writer derives it) |
| 2 | 187 / 207 | bind filter (Vibration "threshold/block") |
| 3 | 188 / 208 | bind scale (Vibration "intensity coefficient") |
| 4..9 | 189-193 / 209-213 | bind params[5] |
| 9 | 194 / 214 | "mixed border". Meaning unknown; preserve it |
| 10..20 | 195-204 / 215-224 | effect params[10] |

Factory, both sides: `0 0 10 10 100 1 255 70 0 0 | 0 0 0 0 0 0 0 0 0 0`. That is mode 0, bind
type 0, filter 10, scale 10, bind params `100 1 255 70 0`, mixed border 0, all params 0
(factory hex, §3.13).

**Effect modes** (E:33-39, E:72-150, P:152-159, P:197-214):

| mode | SDK enum | Space Station UI label | Kind | Felt on hardware |
|---|---|---|---|---|
| 0 | Normal | General | none (loose) | clears the effect |
| 1 | Race | Racing | **resistance** (constant past a point) | yes |
| 2 | Sniper | **"Recoil"** (labels swapped!) | **VIBRATION** (rattles/vibrates on its own past a travel point) | yes, "vibrates on its own" |
| 3 | Recoil | **"Sniper"** | **resistance** (breakthrough band) | yes, "resists, then gives way" |
| 4 | Lock | Trigger lock | **resistance** (hard stop; the axis becomes digital 0/255) | yes |
| 5 | Vibration | Vibration | **VIBRATION** (game rumble routed into the trigger, via live command 82) | the stored → 82 path is confirmed |

Param slots per mode (indices into params[0..10], i.e. abs L 195+i, R 215+i) (E:185-198,
P:337-344, TM:482-532). UI bounds are slider bounds, not byte ranges:

| mode | p[0] | p[1] | p[2] | p[3] | p[4] | bind half |
|---|---|---|---|---|---|---|
| 0 Normal | start (inert) | end (inert) | 0 | 0 | 0 | untouched |
| 1 Race | start 0..192 | resistance 1..255 | 0 | 0 | 0 | untouched |
| 2 Sniper | start 0..192 | press 1..255 | strength 1..255 | frequency 1..255 | match_input 0/1 | untouched |
| 3 Recoil | start 0..192 | travel 1..255 | resistance 1..255 | 0 | match_input 0/1 | untouched |
| 4 Lock | start 20..200 | **255 fixed** | **1 fixed** | 0 | 0 | untouched |
| 5 Vibration | stroke 1..200 | frequency 1..255 | **1 fixed** | **90 fixed** | 0 | filter = block 1..255, scale 0..200, params `[stroke,1,1,frequency,0]`, bind type 2 |

Writer behaviour (E:222-252, M:1924-1947):
* Choosing Normal writes only the mode byte. The params are kept.
* Any other mode rewrites all 10 params: zeros, then the fixed constants, then the knobs, each
  clamped to its UI bounds.
* Leaving Vibration resets the bind type to 0 but **keeps the bind bytes** (TM:528-532), matching
  Flydigi's writer.

**Storing an effect does nothing by itself**, and neither does applying the profile. The effect
starts only when the host sends live command 81 (modes 0-4) or 82 (stored mode 5) built from these
bytes, about 500 ms after the read or apply. Write the blob first, then the live commands; the
reverse order silently loses the write (P:328-333, E:391-469). §8 gives the live mapping.

### 3.9 Title: 770..790

20 bytes **UTF-16LE**, at most **10 UTF-16 code units** (M:1307-1315, GP:50-53, DS:252-253).
* Read: decode UTF-16LE and strip trailing `U+FFFF` and `U+0000`.
* Write (openflydigi): encode, truncate to 20 bytes, pad with 0x00 to 20.
* The factory form pads with 0x00 up to 786 and leaves 786..790 as 0xFF. Both readers strip both
  fillers (F:736).
* Factory titles: `配置1`..`配置4` (bytes `4d 91 6e 7f 3X 00`), differing only at offset 774
  (FC:38-40).
* Pitfall: truncating raw bytes can split a surrogate pair. Limit by UTF-16 units, not by chars.

### 3.10 data_version: 225..227

uint16 LE. Factory 0xFFFF. See §2.6.

### 3.11 Wheel: 183..185

`Lunpan` block, factory `0 0`. Uninterpreted. Preserve it (F:720).

### 3.12 Turbo

Turbo is per-key in the key table (§3.1, bytes +1 and +2). The physical Turbo key (id 25) arms it.
There is no separate turbo block.

### 3.13 Factory Apex 5 profile (read off hardware; slot N: byte 774 = 0x31+N)

Source: FC:76-102 (the file header is GPL-3.0-or-later; NOTICE states the bytes are device data,
not original work). Validated: identical across the 6 Apex 5 SKUs except byte 154 (F:740-748).

```
pkt  0 @  0: 01 03 4d 20 04 28 a0 00 00 ff 00 00 00 ff 00 00 ff 00 00 ff
pkt  1 @ 20: 00 00 ff 00 00 ff 00 00 ff 00 00 ff 00 00 ff 00 00 ff 00 00
pkt  2 @ 40: ff 00 00 ff 00 00 ff 00 00 ff 00 00 ff 00 00 ff 00 00 ff 00
pkt  3 @ 60: 00 ff 00 00 ff 00 00 ff 00 00 ff 00 00 ff 00 00 ff 00 00 ff
pkt  4 @ 80: 00 00 ff 00 00 ff 00 00 ff 00 00 ff 00 00 ff 00 00 ff 00 00
pkt  5 @100: ff 00 00 ff 00 00 ff 00 00 00 00 3f 3f 7f 7f 7f 00 00 3f 3f
pkt  6 @120: 7f 7f 7f 00 00 00 00 ff ff ff 00 00 00 00 ff ff ff 00 0c 00
pkt  7 @140: 04 19 14 00 00 00 00 3c ff 32 00 50 ff 32 00 01 1e 50 05 01
pkt  8 @160: 32 00 ff 28 78 05 00 32 00 01 1e 50 05 01 32 00 ff 28 78 05
pkt  9 @180: 00 32 00 00 00 00 00 0a 0a 64 01 ff 46 00 00 00 00 00 00 00
pkt 10 @200: 00 00 00 00 00 00 00 0a 0a 64 01 ff 46 00 00 00 00 00 00 00
pkt 11 @220: 00 00 00 00 00 ff ff ff ff ff 00 00 00 00 00 00 ff ff ff ff
pkt 12..37 : ff × 20 each
pkt 38 @760: ff ff ff ff ff ff ff ff ff ff 4d 91 6e 7f 31 00 00 00 00 00
pkt 39 @780: 00 00 00 00 00 00 ff ff ff ff 00 32 3e 4b 57 64 70 7d 89 96
pkt 40 @800: 00 00 00 32 3e 4b 57 64 70 7d 89 96 00 00 ff ff ff ff ff ff
pkt 41 @820: 03 03 03 03 03 ff ff ff ff ff 00 3f 3f 7f 7f 7f ff ff ff ff
```
Note the factory macro page header: offset 230 = `00` (count 0), 231..236 = `00`×5, then FF fill.
The cycle bytes at 820..825 are `03` (30 ms).

---

## 4. Macros

### 4.1 Which format the Apex 5 uses, and how to detect it

* The Apex 5 reports **ProtoVersion 769 = v3.1** at blob[0..2]. Its macros live **inside the
  profile**, in the page at **230..768** plus repeat intervals at **820..825**. There is **no macro
  command** in v3.1 (P:901-904, P:912-916, F:575-579).
* Layout test (where the macros live): `(ProtoVersion & 0xF) < 2` → in the blob
  (`MACRO_MAX_VERSION = 2`) (M:142, M:1458-1467).
* Capability test (limits): `ProtoVersion >= 770` → v3.2 limits. The two tests agree on every
  existing version (M:150-165, M:186-193).
* v3.2 (e.g. Vader 5, 770) keeps macros in a separate 1620-byte store behind 172/173/174. This is
  **not hardware-validated** (§4.6).

| Limit | v3.1 (Apex 5) | v3.2 |
|---|---|---|
| macros | **5** | 10 |
| total steps across all macros | **128** | 256 |
| time unit | **10 ms ticks** | 1 ms |
| repeat interval max | **2540 ms** (byte × 10) | 65535 ms |

(M:180-183, P:952-958)

### 4.2 The v3.1 page: blob 230..768 (538 bytes, `m_fdg_macro_page_struct_t`)

(M:103-148, F:534-558)

| Abs | Page off | Field |
|---|---|---|
| 230 | 0 | count of macros. **1..5 is valid; any other value (0, 0xFF, …) means no macros** |
| 231..236 | 1..6 | offset of macro slot s (s = 0..4), in **4-byte words** relative to page offset 6 (abs 236). Unused slots are written 0 |
| 236..768 | 6..538 | bodies, 133 words |

Body at `abs = 236 + 4*offset[s]`:

| +i | Field |
|---|---|
| 0 | trigger key id (ControllerKey; any key, paddles included) |
| 1, 2 | step count, uint16 LE |
| 3 | type `MacroEnableType` |
| 4 + 4k + 0..2 | step k: **cumulative** time, uint16 LE, in **10 ms ticks** |
| 4 + 4k + 2 | step k: key id (must be an XInput target, §4.3) |
| 4 + 4k + 3 | step k: event `MacroActionEvent` |

Each body occupies `1 + steps` words. Bodies are packed back to back in list order. Offsets: slot
0 = 0, slot s = sum of (1 + steps) over the previous slots. Everything after the last body up to
768 is **0xFF** (M:1564-1591, TM:1650-1679).

Enums (M:230-248, F:570-573), copied verbatim:

| `MacroEnableType` | value | behaviour |
|---|---|---|
| None | 0 | — |
| Once | 1 | one pass per press. **Plays to the end even if the key is released** (measured, F:605-607) |
| Press ("while held") | 2 | repeats while the key is held (measured) |
| Click ("toggle") | 3 | a press starts it, a second press stops it |

| `MacroActionEvent` | value |
|---|---|
| Release | 0 |
| Press | 1 |
| LeftJoystick | 2 (semantics/params unknown; never emitted by either editor) |
| RightJoystick | 3 (unknown, as above) |
| *(none)* | **4 is skipped** |
| Hold | 5 (never emitted by Space Station's editor) |

The openflydigi GUI writes only Press and Release (GP:1151-1152).

**Repeat interval, 820..825** (one byte per slot s = 0..4) (M:144-148, M:1469-1475, M:1593-1622):
* stored value = ms / 10. **0xFF = unset** (read it as None, not as 2550 ms). Factory value is 3
  (30 ms).
* It is the gap between repeats of a held or toggled macro. Measured: 300 ms written gave 300 ms
  between passes (F:594-595).
* The interval is **per slot, not per macro**. When the list is compacted (a macro removed and the
  later ones shifted down), **set the slots past the new count to 0xFF**, or the next macro to land
  in a slot inherits a stale interval (M:1599-1622, F:560-568). A `None` interval for a slot that
  still holds a macro means "keep the existing byte".

**Time encoding** (M:1574-1584, TM:1681-1692): each step's gap is quantised **individually**
(`gap_ms // 10`) and accumulated: `ticks = min(0xFFFF, ticks + max(0,gap)//10)`. Reading back:
`delay = (cum[k] - cum[k-1]) * 10`. The first step usually has delay 0. 16-bit saturation means the
maximum macro length is 655.35 s.

**Reading** (M:1477-1529) is permissive. Accept unknown key ids raw. For each slot, the body end is
the next offset, or the page end for the last slot. `steps = min(stored_count, (len(body)-4)/4)`.
Skip the slot if the body is out of range or shorter than 4 bytes.

### 4.3 Validation before writing a macro (v3.1)

(M:201-228, M:1536-1659, GP:1154-1159)
1. Number of macros ≤ 5. Sum of steps ≤ 128. 128 = (538-6)/4 - 5, so the region size and
   Flydigi's limit agree (M:136-140, M:170-172).
2. **Step key ∈ XINPUT_TARGETS** = ids {0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,27}. M1-M6, C/Z,
   menu, turbo and back are refused: the host would never see them (M:215-228).
3. Trigger key: any named ControllerKey (M:201-212). The GUI offers the pad's key list.
4. Delay ≥ 0 (negative becomes 0). The GUI caps one step at 8000 ms, matching Space Station's
   editor. The backend does not cap it (GP:1154-1159).
5. Interval clamped to 0..2540 ms, then //10.
6. **One macro per trigger key.** `set_macro` replaces any existing macro on that key.
7. **Bind the key and the body together**: write the key-table entry target = 32 (`TARGET_MACRO`)
   when adding a macro, and target = 255 when clearing one (M:1624-1659).
8. Unbalanced press/release: Space Station refuses it; openflydigi only warns (F:635-638).
9. Names: v3.1 has no name field, and openflydigi silently drops one (M:1635-1638).

### 4.4 Hardware-verified macro behaviour and pitfalls

(P:901-908, F:585-607)
* Macros **play on the pad**, with no host involved. Timings come back to the millisecond.
* **Written ≠ playing: you must send 162 (apply) after writing a changed macro page.** 166 alone is
  not enough. 166 only decides whether the macro survives sleep. A macro applied with 162 and never
  saved still plays (P:906-908).
* Only apply when the macro bytes changed: compare 230..768 together with 820..825. Applying
  re-seats the trigger motors (M:1433-1456, W:563-568).
* **Orphaned bodies still run.** If a key's target is changed away from 32 while its body remains
  in the page, the key sends its new binding **and** the old macro plays alongside it. Measured
  with M1→A over an X-tap macro: `press a, x x x, release a`. **Always delete the body when
  remapping away from a macro** (M:1355-1371, F:596-604).
* Recording off evdev requires the pad's `controller_data` transport flag on (third-party control
  off). With it off, the evdev node is silent (MAC:18-21, F:581-583).

### 4.5 Recording helper semantics (MAC, optional)

These are host-side and not protocol. Buttons come from evdev (BTN_SOUTH→a, BTN_EAST→b, BTN_X→x,
BTN_Y→y, TL→lb, TR→rb, SELECT, START, MODE→home, THUMBL→thl, THUMBR→thr). The hat gives
left/right and up/down. LT/RT are pressed at ≥ 0.5 travel. The first step has delay 0. Keys still
held at the end are released at stop time (MAC:33-59, MAC:91-136).

### 4.6 v3.2 macro store (NOT used by the Apex 5; NOT hardware-validated)

Included for completeness (M:883-1149, P:910-966).

| Cmd | Payload | Reply |
|---|---|---|
| 172 read | `[cfgId, 20]` | 81 packets, same layout as 163 |
| 173 write start | `[cfgId, startIdx, nPkts, 20]` | ACK |
| 174 write pack | `[pktNum, 20 bytes]` | ACK |

Store: 1620 bytes = 81 × 20.
* `[0..2]` version (the store's own version, echoed back on write).
* `[2..4]` count LE (1..10 valid).
* `[4..24]` 10 × uint16 LE word offsets from byte 24, with 0xFFFF for an empty slot.
* Bodies: `[0]` key, `[1..3]` steps LE, `[3]` type, `[4..6]` interval in ms LE, `[6..12]` FF,
  `[12..32]` UTF-8 name with FF fill, then 4-byte steps with a cumulative time in **ms**. Each body
  uses `8 + steps` words.

Write order: profile (164/165), then the store (173/174), then 166. The reference does not
document whether 172 switches the slot (MP:716-723).

---

## 5. Recommended sequences

**Edit and save profile N** (W:550-594, CLI:164-201). Hold the exclusive lock for all steps.
1. 161 → note the active slot A and the tags.
2. If A ≠ N: 163 read N. This switches the pad to N (re-seating trigger motors). Otherwise
   (A == N) 163 read N to get the blob. It is already active, so it does not switch.
3. Edit the blob in memory and validate (§3, §4.3).
4. 164/165 diffed runs to cfgId N.
5. If 230..768 or 820..825 changed: 162 [N].
6. To persist: 161 → assert active == N, then 166 [fresh tag] with a 10 s timeout. On the ACK,
   set blob[225..227] = tag.
7. (Space Station and openflydigi then wait 500 ms and replay the stored trigger effects with live
   81/82. **See §8 before doing this.**)

**Browse without switching**: §2.4 procedure.

**Copy to Switch bank**: 163 read N (now running), optionally normalise and write to RAM, then
171 [tag, N+4], then restore the original to RAM if it was modified. Do not send 166.

---

## 6. Invariants and validation checklist (enforced by openflydigi before writing)

| Area | Rule | Source |
|---|---|---|
| packet | checksum required. Bad checksum → silence | P:888 |
| blob size | exactly 840 for v3.1. Restore files must be 840 (or 2460 = 840 + 1620 for v3.2) | GP:2671-2678 |
| save | only when edited slot == active slot (161) | GP:2389-2402 |
| 166 tag | fresh random ≠ current and ≠ 0xFFFF after mapping changes; current tag for a lighting-only save | M:672-694 |
| 171 | cfgId 4..7 only; quirk frame | M:847-850 |
| key target | UI offers only XINPUT_TARGETS; id == own → 255; turbo requires a real id; freq ≤ 255 (GUI ≤ 40) | M:1348-1392, GP:1092-1098 |
| macro | ≤ 5 macros, ≤ 128 steps, XInput step keys, page bound, intervals ≤ 2540 ms, tail slots → 0xFF, key table = 32 | §4.3 |
| remap from macro | delete the body | M:1355-1371 |
| sticks | center/edge 0..100; center + edge ≤ 100; points 0..127; bank exactly 9 values in 0..150; type in both blocks; recompile the bank | §3.3 |
| trigger travel | 0..255, sorted, points mirrored | §3.4 |
| gyro | enums validated; sensitivity/dead zone 0..100; byte 144 only under Press; Mouse branch | §3.5 |
| grip vibration | 0..255, min ≤ max | M:1869-1884 |
| trigger motor | scale ≤ 100, min ≤ max | M:2237-2258 |
| force trigger | bind type 2 iff mode 5; params clamped to slider bounds | E:222-252 |
| title | ≤ 10 UTF-16 units | GP:50-53 |
| factory restore | refuse a model with no factory blob (an Apex 5 blob on a Vader breaks C/Z) | M:753-765 |

## 7. Known pitfalls and gotchas (hardware-verified unless noted)

1. **163 switches the active profile**, on the first packet, even when the read fails (§2.4).
2. **166 saves the running profile**, not a named slot. The user can switch profiles on the pad
   (Fn) between your status read and the save, so recheck with 161.
3. Unsaved changes are lost on **sleep**, not only on power-off.
4. Macros need **162** to play after a write.
5. Orphaned macro bodies keep playing.
6. `TARGET_KEYBOARD` (254) silences a button on Linux.
7. Blob byte 2 is 77, not a size.
8. The stick bank at 790 is what plays. The core 109 block, type, center and edge byte are inert,
   so every stick edit must recompile the bank, by truncation.
9. The gyro curve at 830 is inert. The Normal-effect params at 195/196 and 215/216 are inert. The
   trigger travel window lives at 123.
10. The factory gyro enable keys are LT (138 = 12) **and D-pad Up** (144 = 0).
11. Releasing the gyro enable key parks the stick off-centre.
12. Force-trigger side value 3 ("Both") ACKs and does nothing. Send one command per side
    (P:140-150).
13. An ACK only means the frame parsed (P:144-145). openflydigi does not check the success byte
    for mapping commands.
14. Replies are broadcast to all readers: drain stale input and lock the node.
15. 175 resets **all four** profiles (and their names), whatever cfgId it is given.
16. Reads of slots 4..7 alias 0..3. The Switch bank cannot be read.
17. Unverified (source says unmeasured): writing (164/165) to a non-running slot; what 166 does
    with tag 0; 171 aimed at slots 0..3; the turbo frequency unit; macro stick events 2/3 and Hold
    5; mixed-border bytes; wheel block; unaccounted ranges; the semantics of `use mode` (FPS vs
    Racer).

---

## 8. VIBRATION-RELATED (the app must never enable vibration)

Everything in the profile and command scope that enables, configures or triggers rumble or
vibration.

### 8.1 Profile fields: what to leave alone and what to force off

| Field | Abs offsets | What it does | Factory | Policy for a no-vibration app |
|---|---|---|---|---|
| **Grip vibration master** | **145** | Gates the game's rumble to the grip motors. 0 = ON, 0xFF = OFF | **0 = ON** | Never write 0. To force off, write **0xFF**. The source describes it as the master switch; disabling it is not separately bench-measured in the docs |
| Grip left enable / min / max / scale | 146 / 147 / 148 / 149 | Per-motor enable (0 = ON, 0xFF = OFF) and intensity window | 0 / 60 / 255 / 50 | Never write 0 to 146. To force off, write 146 = 0xFF. Never raise min, max or scale |
| Grip right enable / min / max / scale | 150 / 151 / 152 / 153 | as above | 0 / 80 / 255 / 50 | as above, with 150 = 0xFF |
| Trigger-motor enable | 154 | Trigger vibration motors. **The Apex 5 has none** (inert) | 0 | Leave it untouched. If you must change it, Flydigi's OFF value is **1**; openflydigi writes 0xFF |
| Trigger-motor gears | 155..183 | min, max, filter, scale of trigger rumble | populated | Leave untouched (inert on the Apex 5) |
| **Force-trigger mode = 2** (Sniper / UI "Recoil") | 185 (L), 205 (R) | **Vibration-type**: the trigger vibrates on its own past a travel point (felt on hardware) | 0 | **Never write 2**. Treat an existing 2 as vibration. Its strength and frequency are in params p[2] and p[3] (197/198, 217/218) |
| **Force-trigger mode = 5** (Vibration) | 185, 205 | **Vibration-type**: routes game rumble into the trigger (replayed as live **command 82**) | 0 | **Never write 5** |
| Force-trigger bind type | 186, 206 | 2 = bound to grip rumble (written iff mode 5) | 0 | Keep it 0. Never write 2 |
| Force-trigger bind filter / scale / params | 187-193, 207-213 | Threshold, intensity coefficient, and `[stroke,1,1,freq,0]` of the rumble→trigger bind | `10, 10, 100 1 255 70 0` (populated at rest) | Leave untouched. Used only by mode 5 |
| Force-trigger mixed border | 194, 214 | unknown | 0 | Leave untouched |
| Modes 0, 1, 3, 4 | 185, 205 | **Resistance-type** (Normal, Race, Recoil/UI "Sniper", Lock). Not vibration | — | Allowed |

Macro, key-table, stick, trigger-travel and gyro fields carry **no** vibration fields. Macro events
are buttons only.

### 8.2 Commands in or adjacent to this scope

| Command | Vibration impact |
|---|---|
| 161, 163 (read), 164/165 (write), 166, 171, 175 | No direct rumble. 175 restores factory grip vibration = **ON** (factory 145/146/150 = 0) for all four slots. A **single-slot factory restore** also writes 145 = 0 (ON). **If you implement a factory restore, patch 145/146/150 to 0xFF first** if your policy is to force vibration off |
| 162 (apply), 163 (read) | Re-seat the force-trigger actuators audibly (motor movement, not rumble) and appear to drop the live 81 effect. **Do not engage stored effects afterwards** unless they are resistance-type |
| **Live replay of stored effects** (openflydigi `effects.engage_stored`, Space Station `UpdateAdapterTriggerConfig`). Out of this command scope, but every openflydigi read and write path calls it (W:379, W:404, W:587, CLI:84, CLI:200) | Stored mode 2 → live **81** `[1, side, 2, start, press≥1, strength≥1, freq≥1, match]` = **trigger vibration**. Stored mode 5 → live **82** `[side, 2, filter, scale, stroke, 1, 1, freq]` (len byte 11, no checksum) = **rumble→trigger bind**. Modes 1, 3 and 4 → 81 resistance. Mode 0 / unknown → 81 `[1, side, 0]` (clear). **A no-vibration app must never send 81 with mode 2 or 5, and never send 82.** Replace stored 2 or 5 with 81 Normal, or skip them (E:391-469) |
| 82 bind state | **The live bind survives a config apply** (P:247-248) and persists until the pad leaves the bus (DS:321-323). If something else armed it, it stays armed. openflydigi observed that bindType 0 "appears to mean no bind", and used `82 [side, 2, 255 filter, 0 scale, 0,0,0,0]` as a suppression (P:234-245). Sending 82 at all is a vibration-configuration command, so use it only for explicit off-forcing, if ever |
| Command 18 (0x12) grip rumble `[4]=6, [5]=left, [6]=right` | Out of scope. Directly drives the grip motors (P:487, E:377-388). Never send |
| Command 48 sub 14 "test vibration" | XInput-dialect only (not this vendor NewXInput path) (DS:213-221). Never send |
| No "haptic test" command | There is **none** among 161-175 |

### 8.3 Effect-mode classification (definitive table)

| mode | name | type |
|---|---|---|
| 0 | Normal / General | none |
| 1 | Race / Racing | resistance |
| 2 | Sniper (UI "Recoil", 机枪) | **VIBRATION** |
| 3 | Recoil (UI "Sniper", 狙击) | resistance (breakthrough) |
| 4 | Lock / Trigger lock | resistance (hard stop) |
| 5 | Vibration | **VIBRATION** (rumble bind via 82; the live mode-5 81 buzzed with a bind present and is unresolved) |

Remember that the **UI labels for modes 2 and 3 are swapped** relative to the SDK enum (P:197-214,
E:83-93). Classify by the numeric mode byte, never by the label.
