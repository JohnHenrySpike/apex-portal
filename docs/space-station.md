# Flydigi Space Station 4.2.0.9: что удалось извлечь

Здесь собрано всё полезное для Apex 5 Control, что нашлось при разборе официального приложения Flydigi для Windows.
Установщик не запускался. Ни один запрос к серверам Flydigi не отправлялся. При разборе геймпад не открывался.
Всё ниже прочитано из кода, файлов приложения и скриншотов интерфейса.

Метки достоверности:
- **[код]** — видно в декомпилированном коде или в коде интерфейса;
- **[HW]** — проверено на железе (так помечено в `docs/spec-*.md`);
- **[вывод]** — наше заключение, нужна проверка.

Обозначения путей:
- `SS/` = `space-station/` в корне проекта (папка исключена из git, есть копия в архиве `space-station-4.2.0.9.tar.zst`);
- `D/` = `SS/decompiled/service/` — декомпилированная служба;
- `UI/` = `SS/app/resources/app.asar.extracted/.vite/renderer/main_window/assets/` — минифицированный интерфейс. Строк в нём нет, поэтому место указано уникальной подстрокой для `grep -o`.

Здесь сжатая версия. Полные отчёты по каждой теме, со ссылками на строки кода, лежат в `SS/research/`: `servers.md`, `settings.md`, `proto-profile.md`, `firmware.md`, `screen.md`, а также дизассемблер прошивальщика `hid_boot_command.asm`.

## Содержание

1. [Из чего состоит приложение](#1-из-чего-состоит-приложение)
2. [Главное](#2-главное)
3. [Серверы Flydigi](#3-серверы-flydigi)
4. [Настройки: чего нет в Apex 5 Control](#4-настройки-чего-нет-в-apex-5-control)
5. [Proto-файлы и связь интерфейса со службой](#5-proto-файлы-и-связь-интерфейса-со-службой)
6. [Официальные перечисления](#6-официальные-перечисления)
7. [Формат профиля](#7-формат-профиля)
8. [Прошивка](#8-прошивка)
9. [Своя картинка или анимация на экране](#9-своя-картинка-или-анимация-на-экране)
10. [Безопасность: WriteDeviceType и другие опасные команды](#10-безопасность-writedevicetype-и-другие-опасные-команды)
11. [Что проверить на железе](#11-что-проверить-на-железе)

---

## 1. Из чего состоит приложение

| Часть | Что это | Где разобрано |
|---|---|---|
| `Flydigi Space Station.exe` | Electron: окно и интерфейс на React (Vite, antd) | `SS/app/resources/app.asar.extracted/` |
| `SpaceStationService.exe` | служба Windows на .NET 8, single-file bundle. Работает с HID, хранит профили, держит именованный канал для интерфейса | распакована `SS/tools/unbundle.py`, декомпилирована ILSpy в `D/` |
| `firmware/FirmwareConsole.exe` | отдельная .NET-утилита прошивки и загрузки картинок на экран | `SS/decompiled/firmware-console/` |
| `firmware/tool/mhtool/hid_boot_command.exe` | прошивальщик основного MCU (Megahunt) | дизассемблирован, см. §8.4 |
| `firmware/tool/hsh_tool/BurnTool.exe` | HiSilicon BurnTool для радиочипа NearLink и донгла | только строки |
| `firmware/jl_firmware_upgrade_x64.dll` | прошивальщик чипа JieLi (Switch-интерфейс) | только экспорты |
| `driver/` | драйверы виртуальной клавиатуры и мыши (Feiz*), фильтр Xbox 360, PawnIO | не нужны |
| `Configs/Controller/k5/default/` | заводские профили `default_mapping_<deviceType>.dat` и картинки экрана `default_screen_image_<deviceType>.bin` | §7.6, §9 |
| `Logs/service_log_20260428.txt` | лог службы, случайно попавший в установщик | — |

Самые полезные декомпилированные сборки:

- `D/Flydigi.ControllerSdk/` — весь протокол геймпада. Команды по папкам `Flydigi.ControllerSDK.data.command.{config,setting,information,screen,test}`, разбор профиля в `...data.parser/MappingConfigParser.cs`, разбор подсветки в `LedConfigParser.cs`, модели устройств в `...factory/FlydigiControllerFactory.cs`.
- `D/SpaceStationService/` — логика службы. Основное: `Flydigi.ControllerService.service.controller/ControllerBusinessService.cs` (обработчики IPC), `Flydigi.ControllerService.data/ControllerRepository.cs` (чтение и запись профилей), `...data.mapper/ControllerDataMapper.cs` (перевод данных интерфейса в байты), `KeyboardMouseInjectRunner.cs` (эмуляция клавиатуры и мыши).
- `D/Flydigi.Basic/`, `D/Flydigi.Hid/` — формат пакета, контрольная сумма, поиск HID-интерфейсов.
- `SS/decompiled/firmware-console/FirmwareLibrary/` — прошивальщики (`OtaNewUpdater.cs` для экрана, `MhExeUpdater.cs`, `HshExeUpdater.cs`, `JieLiUpdater.cs`).

Как повторить разбор:

```bash
python3 space-station/tools/unbundle.py space-station/app/SpaceStationService.exe /tmp/svc
ilspycmd -p -o /tmp/src/Flydigi.ControllerSdk -r /tmp/svc /tmp/svc/Flydigi.ControllerSdk.dll
```

---

## 2. Главное

1. **WriteDeviceType запускает не страница «Настройки», а ответ сервера** [код]. Это поправка к прошлой заметке.
   - При каждом подключении геймпада интерфейс отправляет данные устройства на `https://edge.flydigi.com/space-api/product-sn/device-id`, вход в аккаунт не нужен.
   - Если сервер ответит `valid:false`, интерфейс просит службу записать в геймпад присланный сервером `deviceId` (HID-команда 254), после чего геймпад перезагружается.
   - В нашем стенде это выглядело как реакция на «Настройки», потому что заглушка `fake-api.json` отвечала без поля `valid`. Подробно в §10.
2. **Коды режимов подсветки в `src/lighting.rs` взяты не из того перечисления** [код]. Это перечисление кулера (`RgbEffectTypeProto`). У геймпада своё (`LedType`): FLOW 1, BREATH 2, GRADIENT 3, FEEDBACK 4, ON 5, Close 6, Default 7. См. §7.5. **Но переходить на `LedType` нельзя** [HW]: прошивка учитывает байт режима при воспроизведении, и с кодами BREATH 2 и GRADIENT 3 наши «Дыхание» и «Вращение» шли рывками. Apex 5 Control оставил свои коды, при которых все эффекты идут плавно; Space Station просто называет их неправильно.
3. **Подсветка хранится отдельно для каждого слота** [код, HW]. Служба читает и пишет блок подсветки с `cfgId` = номер слота. На железе слот 1 отличался от слотов 2–4. Чтение 167, как и 163, делает этот слот активным [HW]. Apex 5 Control раньше читал слот 0 и этим переключал геймпад на первый профиль; теперь подсветка читается и пишется для активного слота.
4. **Клавиатура и мышь — только на ПК** [код]. Назначения «Особый» (клавиша или кнопка мыши), стик → клавиатура или мышь и гироскоп → мышь геймпад не выполняет. Он только гасит кнопку (код 254), а нажатия подставляет служба Flydigi. На Linux для этого нужен свой демон с uinput.
5. **Экран на Apex 5 загружается не по HID, а через последовательный порт чипа экрана** [код]. Это подтверждает путь из `docs/spec-device.md` §4.4. Команды 208–211 Flydigi использует только для старых моделей.
6. **Образов прошивки в установщике нет** [код]. Они скачиваются с сервера. Проверок целостности (md5, подпись) клиент не делает.
7. **Неизвестные байты профиля Flydigi не сохраняет** [код]. Перед записью он собирает 840 байт заново в буфере, залитом 0xFF. Наше правило «переносить неизвестное как есть» безопаснее и совместимо.

---

## 3. Серверы Flydigi

Все запросы делает интерфейс (Electron). В .NET-службе нет ни одного адреса. Служба и FirmwareConsole скачивают только то, что им передал интерфейс: прошивки, моды игр, кадры экрана. Окружений test/dev и разделения CN/global нет: адреса зашиты константами в `UI/index-BA01cjWW.js`. Регион влияет только на интерфейс: членский центр и вкладка «сообщество» включаются на `zh-Hans`.

### 3.1 Хосты

| Хост | Для чего | Нужно Apex 5 Control |
|---|---|---|
| `https://api.flydigi.com/pc` | прошивки, обновление приложения, игры для адаптивных курков, анимации экрана, обмен профилями, каталог устройств, баннеры | да |
| адреса из ответов `api.flydigi.com` | файлы прошивок, zip-моды игр, картинки анимаций, установщик | да (прошивки, анимации) |
| `https://edge.flydigi.com/space-api` | аккаунт, вход по SMS, баллы, магазин, **сверка типа устройства** | нет |
| `https://data.flydigi.com` | аналитика подключений, опрос NPS, короткие ссылки `api/web/link?u=<id>` | нет |
| `assets.giocdn.com` (GrowingIO, проект `85a9ba065460930a`) | аналитика | нет |
| `https://crash-reporter.flydigi.com/upload` | отчёты о падениях; отправка выключена (`uploadToServer:false`) | нет |
| `mall-1300023079.cos.ap-shanghai.myqcloud.com`, `*.feishu.cn` | QR-коды магазина, справка | нет |

### 3.2 Как строятся запросы к `api.flydigi.com/pc`

- axios, `timeout: 10000`, `withCredentials: true`. GET-параметры уходят в query, тело POST — JSON.
- Заголовки: `language: <язык интерфейса>` (`en`, `zh-Hans`, `zh-Hant`, `ko`, `pt`, `es`, `ru`, `ja`, `pol`) и `appVersion: 4.2.0.9`.
- **Токена, подписи, метки времени нет.** Любой может повторить запрос.
- Ответ — конверт `{code, msg, data}`; успех — `code` 200 (или 0). `code == 599` — разлогин. Вызывающему коду возвращается `data`.

Пример:

```bash
curl -s -X POST https://api.flydigi.com/pc/Update/firmware \
  -H 'Content-Type: application/json' -H 'language: en' -H 'appVersion: 4.2.0.9' \
  -d '{"device_code":"k5","device_id":128,"app_version":"4.2.0.9","main_chip":"7.0.4.5"}'
```

(Пример составлен по коду; сами мы этот запрос не отправляли.)

### 3.3 Эндпоинты `api.flydigi.com/pc`

| Метод и путь | Параметры | Ответ (`data`) | Где в коде |
|---|---|---|---|
| `POST /Update/firmware` | `{device_code: "k5", device_id: <deviceType>, app_version, main_chip, rf_chip, si_chip, trigger_chip, screen_chip, dongle_chip}` — поле чипа есть, только если у устройства есть версия этого чипа | `{chip_list: {<чип>_chip: {version, url, info, is_push}}}` | `UI/index-BA01cjWW.js`, `yI=e=>yC.post("/Update/firmware",e)` |
| `POST /Update/software` | `{version}` | `{version, url, info, force}` | `ij=e=>yC.post("/Update/software",e)` |
| `GET /adapter_trigger/list` | `version=0` | массив игр (§3.4) | `UI/index-Dg5xFi6u.js`, `"/adapter_trigger/list"` |
| `GET /screen_pic/list` | `version=0` (для зарядной станции ещё `device_code`) | `[{id, title, imagePath}]` — обычные GIF, PNG, JPG | `UI/animation_community-C13FrjU9.js`, `"/screen_pic/list"` |
| `POST /config_share/upload` | профиль: `{configName, controllerType: "k5", configContent}`; макрос: то же плюс `configType: 2` | `{uniqueId}` — код для обмена | `UI/shareConfig-DxVm4a41.js` |
| `GET /config_share/download` | `uniqueId, controllerType, configType` (1 профиль, 2 макрос) | `{config_type, config_name, config_content}` | там же |
| `GET /product/list` | `version=0` | каталог устройств: `{categories: [{category, name, devices: [{device_code, device_name, cover, tutorial_video, support, connect_guide}]}]}` | `UI/equipementList-BwSBxdFv.js` |
| `GET /activity_banner/list` | `device_code` | `{list: [{language, image_url, link_url, title}]}` | `UI/index-DvPpOxTr.js` |
| `/nss/index?device_code=&lang=&device_type=` | — | HTML-страница отзывов, открывается в браузере | `UI/index-DM6mSbRo.js` |

Подробности:

- **Прошивки.** `device_id` — это deviceType (128..136), а не UID. Поле `info` — текст изменений, `url` передаётся в FirmwareConsole, `is_push` — показывать ли окно обновления при подключении. md5 и флага принудительного обновления у прошивок нет. Обновление предлагается при **любом отличии** версии. В режиме восстановления неподдерживаемого устройства интерфейс шлёт `main_chip: ""` (или `dongle_chip: ""`), чтобы получить последнюю версию. Как прошивка ставится — §8.
- **Обновление приложения.** Установщик скачивается в `%TEMP%\updates\update-<версия>.exe` и запускается. electron-updater не используется.
- **Каталога пресетов кнопок на сервере нет.** `/config_share/*` работает только по коду. Профиль в `configContent` — protobuf `ControllerMappingConfigBean` в виде hex через дефис (`0A-1B-…`, §7.8), макрос — байты `MacroItem` в десятичном виде через дефис. Интерфейс дополнительно оборачивает профиль в `JSON.stringify`, то есть на сервер уходит строка в кавычках. Работает ли из-за этого импорт, без сервера не проверить.
- **Анимации экрана.** Сервер отдаёт исходные картинки, всё преобразование делает клиент (§9).

### 3.4 Игры для адаптивных курков

- `GET /adapter_trigger/list?version=0`. Интерфейс не разбирает ответ, а передаёт массив в службу как JSON-строку (IPC 4160).
- Служба разбирает его в `GameListTriggerModel` (`D/AdapterTriggerService/AdapterTriggerService.model/GameListTriggerModel.cs`) и сохраняет в `Configs\Controller\Trigger\`.
- Поля (имена, скорее всего, в camelCase):

| Поле | Назначение |
|---|---|
| `id`, `gameName`, `enGameName`, `icon`, `imagePath` | игра |
| `platform`, `platforms`, `platform_game_ids` | Steam AppID, Epic, Xbox через запятую — так служба находит установленную игру |
| `processGameName(s)`, `exeName`, `gameStartType` | как распознать и запустить игру (`steam://rungameid/<id>` или exe) |
| `isMod`, `modName`, `modStartType`, `modDownLoadUrl`, `version` | мод Flydigi для игры (zip, ставится в папку игры) |
| `isPS5` | режим эмуляции DualSense |
| `isVibration`, `vibType`, `vibParams`/`vibParamsRight` (5 байт через запятую), `vibFilter`, `pwmScal` | «вибро-курки» без мода; уходят в геймпад командой привязки к вибрации |
| `minFirmwareVersion`, `noticePicUrl`, `update`, `scenes` | ограничения и тексты |

- Моды скачиваются простым GET по `modDownLoadUrl` без проверки хэша и общаются со службой по UDP `127.0.0.1:7878` (JSON-пакеты). Для Linux это малополезно: всё завязано на вибрацию и Windows-моды.

### 3.5 `edge.flydigi.com/space-api` и `data.flydigi.com`

Для Apex 5 Control не нужны, кроме одного запроса (выделен).

| Запрос | Что |
|---|---|
| `POST /space-api/auth/sms`, `/auth/login`, `/auth/refresh`, `/auth/login-ticket/generate` | вход по SMS (только китайские номера), токены |
| `GET /space-api/member/profile`, `/member/privileges`, `POST /member/me` | профиль. `member/me` выдаёт ключи шифрования на сессию |
| `POST /space-api/member/device`, `/member/act` | «привязка» устройства и активность за баллы. Тело зашифровано: HMAC-SHA512 от канонической строки и AES (CryptoJS, режим пароля), ключи из `member/me` |
| `/space-api/mall/*`, `/space-api/feature-flags` | магазин баллов |
| **`POST /space-api/product-sn/device-id`** | **сверка типа устройства. Уходит при каждом подключении, без входа в аккаунт и без шифрования. Тело — весь `DeviceInfo`: uid, MAC, deviceType, версии, VID/PID. По ответу `valid:false` интерфейс перезаписывает тип геймпада (§10)** |
| `POST data.flydigi.com/api/space_station` | аналитика при каждом подключении: версия приложения и ОС, язык, uid, модель, версия прошивки |
| `POST data.flydigi.com/api/nps/report` | опрос удовлетворённости |

У `edge.flydigi.com` заголовок `Authorization: Bearer <токен>` (без входа — пустой), ответ `{success, data}`; на 401 токен обновляется через `/auth/refresh`.

---

## 4. Настройки: чего нет в Apex 5 Control

Сверка сделана по скриншотам официального интерфейса (`SS/screens-2560/`), коду интерфейса, службы и SDK, а также по нашему `src/`.

### 4.1 Страница «Настройки» (настройки устройства, не профиля)

Все значения читаются одной командой 3 (`ReadHardwareFunctionStatus`, `D/Flydigi.ControllerSdk/Flydigi.ControllerSDK.data.command.setting/ReadHardwareFunctionStatusCommandFactory.cs:21-91`). Пакеты в формате `03 5A A5 <cmd> <len> <данные> <сумма>`.

| Настройка | Что делает | Команда | У нас | Что сделать |
|---|---|---|---|---|
| Быстрая смена профиля | Fn + A/B/X/Y переключает слоты | `13 04 01 <0/1>` | есть | — |
| **Время сна** | минуты до засыпания | `17 03 <минуты>` | **частично**: слайдер 0..60 (`src/ui/mod.rs:576`), `src/safety.rs:35` пропускает только ≤60 | Официальный интерфейс предлагает 1, 5, 15, 60, **180** и «никогда» (0). Разрешить 180 в safety (`0..=60 \| 180`) и сделать выбор из этих значений |
| **Turbo Function** (sub 4) | разрешает турбо-режимы кнопок | `13 04 04 <0/1>` (`EnableMappingSwitchCommandFactory.cs`) | **подписано неверно**: у нас «Переключатель раскладки» (`src/ui/mod.rs:506-510`) | Переименовать в «Кнопка Turbo». В интерфейсе Flydigi ключ локали `turbo_mapping_title`: «Turbo Function», по-китайски «Turbo 键功能». Выключает ли он ещё и макросы, не ясно [вывод] |
| Фильтр дребезга стиков (sub 5) | стабильнее центр; без него не работает автокалибровка | `13 04 05 v` | есть | — |
| Автокалибровка (sub 6) | центр обнуляется после 3 с покоя; отключается при заданном смещении стика | `13 04 06 v` | есть | В подсказку добавить «3 с покоя» |
| Rebounce (sub 7) | гасит отскок стика; для шутеров советуют выключать | `13 04 07 v` | есть | — |
| Строка состояния всегда (sub 8) | — | `13 04 08 v` | есть (`src/ui/mod.rs:600-620`) | В официальном интерфейсе её нет, а служба при каждом чтении сама выключает этот флаг (`ControllerBusinessService.cs:1133-1139`). Стоит убрать тумблер или пометить его как экспериментальный |
| Анимация экрана (sub 9) | показывать ли анимацию | `13 04 09 v`, 1 = показывать | есть | — |
| Точность стиков | разрядность вывода | `15 03 <v>` | предлагаем 7 значений (`src/device.rs:150`) | Flydigi предлагает только 8–12 бит (значения 1..5). 14 и 16 бит пометить как экспериментальные или скрыть |
| Чувствительность центра | отзывчивость в центре | `16 03 <v>` | 7 значений (`src/device.rs:159`) | Flydigi предлагает только 14/17/19. Можно оставить: все значения из перечисления, 16 проверено на железе |
| Частота опроса (cmd 20) | — | `14 03 <1/2/4/8>` | заблокирована | Оставить. Apex 5 возвращает 0, и Flydigi тоже прячет эту настройку |
| Xbox Home (sub 2), антидребезг гироскопа (sub 3), аудио (sub 10), умный стоп дока | — | — | нет | У Apex 5 недоступны (флаги Usable = 0 или команда только для старых протоколов) |
| Разрешить сторонним приложениям (Steam, SDL) | разрешает внешним программам перехватывать геймпад | cmd 17 `11 07 FF FF FF FF <0/1>` (`ControllerSdk.cs:645-648`). Доступно только при прошивке main ≥ 7.0.3.0 | нет | Необязательно. Если делать, разрешить в safety только `p[0..4] = FF` и `p[4] ≤ 1` и предупредить, что ввод через xpad пропадёт |
| Восстановить по умолчанию | сбросить все 4 профиля | cmd **175** | нет (175 запрещена) | Не открывать 175. Сделать циклом нашего `factory_reset` (`src/profile.rs:508`) по 4 слотам: результат тот же, но байты вибрации сохраняются |
| Обновление прошивки | — | cmd 31 и загрузчики | нет, умышленно | см. §8 |

### 4.2 Функции профиля

| Функция | У нас | Что сделать |
|---|---|---|
| **Анти-мёртвая зона и потолок стика** (отрицательные «Центр» и «Край»: ползунки двусторонние, влево — «Смещение») | нет: только 0..100 | `stick_nodes` уже поддерживает `center < 0` и `edge < 0`. Отрицательные значения писать только в банк точек, а core-байты оставить 0: прошивка играет банк, а core Flydigi кодирует иначе (§7.4). Ценность высокая для шутеров. Учесть, что Flydigi при смещении выключает автокалибровку |
| **Выключить вибрацию рукояток** | только показываем (`src/profile.rs:334`) | Тумблер только в одну сторону: байт 145 → 0xFF. `check_no_vibration_change` (`src/profile.rs:491`) должна пропускать только переход в «выключено» |
| Подсветка по слотам | всегда слот 0 (`src/lighting.rs:119`, `:128`) | Сначала прочитать 167 с cfgId 1..3 и сравнить. Если подсветка разная, передавать номер слота в `LedConfig::read/write` |
| Код режима подсветки (байт 8) | свои коды из перечисления кулера (`src/lighting.rs:51-62`) | ~~Писать `LedType`~~ — проверено и отменено: байт влияет на воспроизведение, с `LedType` «Дыхание» и «Вращение» идут рывками [HW]. Оставлены свои коды |
| Подсветка «Обратная связь» | нет | байт 2 = 1, кадры = цвета, loopEnd = число цветов (`ControllerDataMapper.cs:184-197`). Похоже, каждое нажатие переключает кадр [вывод] |
| «Световой эффект при вибрации» | нет (`src/lighting.rs:21`) | байт 9 (`gripSync`) = 0/1. Сам вибрацию не вызывает. Безопасно |
| Сброс одного профиля | `factory_reset` сбрасывает 840 байт, но не подсветку | Flydigi после сброса пишет и заводскую подсветку слота из `default_mapping_<deviceType>.dat` |
| Дублировать слот | нет | Только на хосте: прочитать слот A (163 переключает активный слот, см. spec-profile §2.4), записать в B (164/165), сохранить (166) |
| Экспорт и импорт профиля | нет | Только на хосте: 840 байт в файл и обратно. Импорт официального формата см. §7.8 |
| Switch-слоты (cfgId 4..7 + cmd 171) | нет, 171 запрещена | Нужно только тем, кто играет на Switch. Доступно с прошивки main 7.0.2.8. Если делать, разрешить только 171 с cfgId 4..7 и после подтверждения |
| Кнопка → клавиатура или мышь («Особый»), стик → клавиатура или мышь, гироскоп → мышь | показываем «в Linux не работает» (`src/ui/profile.rs:306`) | Свой демон: 254 в таблице кнопок, поток 0xEF (cmd 17 raw_data = 1) и `uinput`. Работает, только пока приложение запущено. Самая трудоёмкая позиция |
| Курки: «Отдача» (режим 2) и «Вибрация» (режим 5) | нет, умышленно | Оставить: это вибрация |
| Адаптивный курок по играм (моды, DS-режим) | нет | Только на ПК, в основном про вибрацию (команда 82). Не делать |

### 4.3 Тест геймпада

| Функция | Как у Flydigi | Что сделать |
|---|---|---|
| Частота опроса в Гц | хост считает отчёты в секунду (IPC `RollingRateReport`) | Сделано: тестер считает отчёты 0xEF за каждую секунду (`src/tester.rs`, `apex5ctl input`). Через донгл около 300 Гц [HW]. По кабелю 545–555 Гц, когда геймпад подключён через два USB 2.0-хаба вместе с игровой мышью и клавиатурой [HW]. 970 Гц из спецификации, видимо, намерены при подключении напрямую в порт |
| Проверка округлости стика | рисует след стика на хосте | Сделано: максимальный радиус в 72 секторах по 5°, след на круге и среднее отклонение от круга в %, когда обведены все сектора (`src/tester.rs`). Два замера: датчик из потока 0xEF и выход xpad из evdev (`src/evdev.rs`). Поток 0xEF сырой, до профиля: по осям 1.00, по диагоналям 1.12–1.20, отклонение около 10 % [HW]. Выход xpad при профиле «круг» даёт 1.00 во всех направлениях, 0.0 % [HW] |
| Режимы «Выход / Назначение / Оригинал» | служба пересчитывает по профилю | У нас только «Оригинал». «Выход» можно брать из evdev. Низкая ценность |
| Тест вибрации | команда 0x12 | Сделано: только моторы рукояток, меньше секунды, затем стоп (`src/safety.rs` пропускает `12 06 L R 00 00`) |

Заводские команды `TestJoystick`, `AdcCalibration`, `TestRf` и подобные интерфейс не вызывает нигде. Ручной калибровки стиков и курков в официальном приложении нет.

### 4.4 Что сделать в первую очередь

| # | Что | Ценность | Трудоёмкость | Риск |
|---|---|---|---|---|
| 1 | Явный запрет 254 и 240–253 в `safety.rs` с тестами | защита | очень низкая | нет |
| 2 | Сон 180 мин и выбор значений как у Flydigi | средняя | очень низкая | низкий |
| 3 | Переименовать sub 4 в «Кнопка Turbo» | средняя | очень низкая | нет |
| 4 | Анти-мёртвая зона и потолок стика | **высокая** | низкая | низкий |
| 5 | Тумблер «Выключить вибрацию рукояток» | **высокая** | низкая | низкий |
| 6 | Код режима подсветки по `LedType` | низкая–средняя | очень низкая | нет |
| 7 | Частота опроса и округлость в тестере | средняя | низкая | нет |
| 8 | Подсветка по слотам (сначала проверить) | средняя | низкая | низкий |
| 9 | «Сбросить все» через 4 × `factory_reset`; сброс профиля вместе с подсветкой | средняя | низкая | средний |
| 10 | Дублировать слот, экспорт и импорт | средняя | средняя | низкий |
| 11 | Подсветка «Обратная связь» и «при вибрации» | низкая | низкая | нет |
| 12 | Пометить точность 14/16 бит | низкая | очень низкая | — |
| 13 | Пересмотреть тумблер строки состояния | низкая | очень низкая | — |
| 14 | Разрешить сторонним приложениям | низкая–средняя | низкая | средний |
| 15 | Switch-слоты | низкая | средняя | средний |
| 16 | Демон uinput для клавиатуры и мыши | средняя | **высокая** | низкий |

---

## 5. Proto-файлы и связь интерфейса со службой

### 5.1 Откуда proto-файлы

Копий `.proto` в установленном приложении нет. Все 14 файлов в `SS/proto/` восстановлены из дескрипторов protobuf, встроенных в `SpaceStationService.exe`. Номера полей и значения перечислений совпадают с C#-классами (`D/Flydigi.SharedResources/Flydigi.SharedResources.Data.Protobuf/`) и с кодом интерфейса.

Везде `package Flydigi.SharedResources.data.protobuf`, синтаксис proto3.

| Файл | Назначение | Что внутри | Где используется |
|---|---|---|---|
| `Common.proto` | общие типы | `LocalConfigs`, `DeviceConfig {id, title, version, active, fileName, hashCode}`, `Point`, `Color` | файлы `index.dat`, IPC |
| `Enum.proto` | общие перечисления | `ChipType`, `ChipModule`, `Category`, `ConnectType`, `LedType`, `ConfigType`, `JoystickSensitivity`, `JoystickPrecision`, `ReceiveRawDataScene` | везде |
| `ControllerEnum.proto` | перечисления геймпада | `ControllerKey`, типы назначений стика, кнопок, гироскопа, курков, макросов | профиль, IPC |
| `ControllerMappingConfigBean.proto` | **модель профиля** | `ControllerMappingConfigBeans` (4 слота), `ControllerMappingConfigBean` и вложенные Joystick/Key/Led/Trigger/Vibration/Motion/Macro | заводские `.dat`, кэш профилей на ПК, обмен профилями |
| `ControllerMappingConfig.proto` | упрощённые структуры для интерфейса | `SimpleKeyConfig`, `LedConfigs`, `MacroConfig`, `TriggerConfigs` (с `oneof` по типу курка) | IPC GetConfig/UpdateConfig |
| `Device.proto` | описание устройств | `DeviceInfo`, `ControllerInfo` (флаги `support_*`, `contain_keys`), `FirmwareInfo`, `KeyState`, `JoystickState` | IPC |
| `IpcCommand.proto` | **протокол интерфейс ⇄ служба** | `IpcCommandEnum` (112 команд), `IpcCommand`, `IpcResult`, около 90 структур параметров | именованный канал |
| `KeyboardEnum.proto` | коды клавиш | `KeyboardKey` (= Windows `Keys`), `KeyID` (USB HID usage) | клавиатурные назначения на ПК |
| `MouseEnum.proto` | кнопки мыши | `MouseKey` | — |
| `KeyboardMappingConfigBean.proto`, `Charger*.proto`, `Cooler*.proto` | клавиатуры, зарядная станция CD2, кулер | — | не Apex 5 |

Профиль Flydigi можно декодировать так:

```bash
protoc --decode=Flydigi.SharedResources.data.protobuf.ControllerMappingConfigBeans \
  -I space-station/proto ControllerMappingConfigBean.proto \
  < space-station/app/Configs/Controller/k5/default/default_mapping_128.dat
```

### 5.2 Канал интерфейс ⇄ служба

- Именованный канал `\\.\pipe\fcs.sock` (`D/SpaceStationService/Flydigi.ControllerService/FlydigiWorker.cs:262-306`). Права: читать и писать может любой локальный пользователь. Одновременно обслуживается один клиент.
- Цепочка: интерфейс `ipcSocket.send("send_command", obj)` → preload → main-процесс Electron собирает protobuf `IpcCommand` → канал.
- Кадр (одинаковый в обе стороны, `CommunicationProtocolHandler.cs`):

  ```
  меньше 1 МиБ:  "FDG_PROTOCOL\n"   | int32 LE длина | protobuf
  1..5 МиБ:      "FDG_CHUNK\n"      | int32 LE всего | int32 LE число кусков | куски (int32 LE длина ≤ 65536 | байты)
  от 5 МиБ:      "FDG_COMPRESSED\n" | int32 LE длина | gzip(protobuf)
  ```

- Интерфейс шлёт `IpcCommand {cmdId, category, oneof params}`, служба отвечает `IpcResult {cmdId, state, code, category, oneof data}`.
- Нет ни идентификатора запроса, ни контрольной суммы, ни рукопожатия. Ответы сопоставляются только по `cmdId` и могут приходить не по порядку.
- Служба сама присылает события: подключение и батарея (4098), неподдерживаемое устройство (65536), состояние кнопок (10, при включённом потоке), частота опроса (13), прогресс загрузки (4148), смена слота на геймпаде (4115), автосохранение (4114).
- Прошивка идёт мимо канала: Electron сам запускает `FirmwareConsole.exe` и читает его вывод.

### 5.3 Все 112 IPC-команд

Обозначения: R — читает с геймпада, W — пишет в RAM геймпада, W! — пишет во флеш или меняет режим, F — только файлы на ПК, P — событие от службы, ⚠ — опасно, «—» — служба эту команду не обрабатывает.

**Служба и подключение**

| Имя | № | Смысл | |
|---|---|---|---|
| Unknown | 0 | заглушка | — |
| Init | 1 | рукопожатие: версия приложения и язык → путь к данным, флаги логов | — |
| UploadLog | 2 | отправка логов | — |
| OperatorChanged | 10 | состояние кнопок, стиков, курков, IMU | P |
| EnableXInput | 11 | переключатель XInput | — |
| EnableRawData | 12 | включить или выключить поток 0xEF | W |
| RollingRateReport | 13 | замер частоты опроса | R/P |
| SwitchUsb | 15 | **перевести чип в режим загрузчика** | ⚠ |
| EnableControlByThirdPartyApp | 16 | разрешить управление сторонним программам | W |
| AcquireController | 17 | захват геймпада сторонней программой | W |
| CheckThirdPartyAcquiredController | 18 | кто сейчас держит геймпад | R |
| GetConnectedDevices | 4097 | список устройств | R |
| DeviceConnectionChanged | 4098 | подключение, отключение, батарея | P |
| GetDeviceDetailInfo | 4099 | возможности устройства и список профилей | R |
| GetCurrentAppliedConfigId | 4100 | активный слот | — |
| EnableLog | 57345 | логирование службы | F |
| UseMockData | 65535 | тестовые данные | — |
| DeviceNotSupported | 65536 | подключено неподдерживаемое устройство | P |

**Профили**

| Имя | № | Смысл | |
|---|---|---|---|
| GetConfig | 4112 | данные профиля по разделам (`ConfigType`). `cfgId` фактически игнорируется: всегда текущий профиль | R |
| UpdateConfig | 4113 | изменить текущий профиль (164/165, подсветка 168/169), через 250 мс автосохранение | W, затем W! |
| SaveConfig | 4114 | сохранить во флеш (166) с новым случайным dataVersion; то же число — событие автосохранения | W! |
| ApplyOnboardConfig | 4115 | переключить слот (162), несохранённое сначала сохраняется; то же число — событие | W |
| SaveOnboardConfigToLocal | 4116 | сохранить текущий профиль в файл на ПК | F |
| ApplyLocalConfig | 4117 | записать профиль с ПК в слот | W! |
| UpdateLocalConfigSort / UpdateLocalConfigName / DeleteLocalConfig / CreateNewLocalConfig | 4118–4121 | библиотека профилей на ПК | F |
| ShareOnboardConfig / ShareLocalConfig | 4122 / 4123 | экспорт в строку (§7.8) | R / F |
| DownloadSharedLocalConfig | 4124 | импорт строки | F |
| ResetMappingConfig | 4127 | `cfgId=0` — **сброс всех слотов (175)**; `1..4` — заводской профиль в слот `cfgId-1` | ⚠ W! |
| ApplySwitchConfig | 4167 | записать профиль в Switch-слот (171) | W! |

**Макросы** (библиотека на ПК)

| Имя | № | Смысл | |
|---|---|---|---|
| GetLocalMacroConfigList, Rename-, Copy-, Delete-, CreateLocalMacroConfig | 4128–4132 | библиотека | F |
| ShareMacroConfig | 4133 | экспорт | — |
| SaveSharedMacroConfig | 4134 | импорт (байты в **десятичной** записи через «-») | F |
| ApplyMacroConfig | 4135 | назначить макрос кнопке | W |
| SaveMacroConfig | 4136 | сохранить макрос в библиотеку | F |
| UpdateLocalMacroConfigSort | 4137 | порядок | — |
| GetLocalMacroConfigDetail | 4138 | макрос из файла | F |

**Экран**

| Имя | № | Смысл | |
|---|---|---|---|
| GetScreenStatus | 4144 | статус экрана | — |
| EnableScreenStatusBarAlwaysOn | 4145 | строка состояния всегда видна | W |
| OffScreen | 4146 | показывать ли анимацию | W |
| UploadPic | 4147 | загрузка картинки по HID (только старые модели) | W! |
| UploadProgressChanged | 4148 | прогресс загрузки | P |

**Адаптивные курки по играм** (на ПК)

| Имя | № | Смысл | |
|---|---|---|---|
| GetAdapterTriggerGames | 4160 | список игр | F |
| EnableAdapterTriggerGame | 4161 | включить игру | F/W |
| UninstallAdapterTriggerGame, StartAdapterTriggerGame, UpdateAdapterTriggerGame | 4162–4164 | мод, запуск, путь к игре | F |
| AdapterTriggerGameStatusChanged | 4165 | событие | P |
| UpdateAdapterTriggerGameMod | 4166 | обновить мод | F |
| SetAdapterTriggerGameFavorite | 4168 | избранное | F |

**Вибрация и настройки геймпада**

| Имя | № | Смысл | |
|---|---|---|---|
| CallVibration / CallGripVibration / CallTriggerVibration | 4176–4178 | тест вибрации | W |
| UpdateReportRate | 4179 | частота опроса | W! |
| UpdateSleepTime | 4180 | время сна | W! |
| UpdateJoystickSensitivity | 4181 | чувствительность центра (14..20) | W! |
| UpdateJoystickPrecision | 4182 | точность | W! |
| EnableJoyStickDebounce / EnableJoystickAutoCalibration / EnableJoystickRebound | 4183–4185 | фильтры стиков | W! |
| EnableQuickSwitchConfig | 4186 | быстрая смена профиля | W! |
| EnableMotionDebounce | 4187 | антидребезг гироскопа | W! |
| ReadHardwareFunction | 4188 | все настройки устройства разом | R |
| UpdateNickname | 4189 | имя геймпада | — |
| EnableMappingSwitch | 4190 | Turbo Function | W! |
| EnableDockSmartStop | 4191 | умный стоп на доке | W! |
| EnableAudio | 24582 | звук | W! |

**Зарядная станция** (24577–24581: подсветка, выключение с ПК, сон при зарядке, анимация, синхронизация подсветки) и **кулер** (28673–28695: вентилятор, RGB, кривые, отчёты; среди них ResetDefaultConfig 28694 — заводской сброс кулера) к Apex 5 не относятся.

**Заводские тесты и идентичность**

| Имя | № | Смысл | |
|---|---|---|---|
| AdcCalibration | 65520 | калибровка АЦП стиков и курков | ⚠ |
| TestIndicator | 65521 | индикатор | W |
| TestScreen | 65522 | заливка экрана цветом | W |
| TestRF / TestDongleLosses | 65523 / 65524 | радио и потери пакетов донгла | W |
| TestRgb / TestJoystick / TestTrigger | 65525–65527 | тесты | W |
| SwitchToXInputMode | 65528 | принудительно в XInput | W! |
| **WriteDeviceType** | **65529** | **записать тип устройства и перезагрузить** | ⚠⚠ |

Служба не обрабатывает 16 команд: Unknown, UploadLog, EnableXInput, GetCurrentAppliedConfigId, ShareMacroConfig, UpdateLocalMacroConfigSort, GetScreenStatus, UpdateNickname, три команды кулера (SetRgbColor, SetRunMode, PowerControl), пять команд конфигов из диапазона кулера (28683–28687) и UseMockData.

---

## 6. Официальные перечисления

Этих названий не было в наших спецификациях. Источник — `SS/proto/*.proto`.

### 6.1 Кнопки (`ControllerKey`)

| № | Имя | № | Имя | № | Имя |
|---|---|---|---|---|---|
| 0 | Up | 10 | Lb | 20 | M3 |
| 1 | Right | 11 | Rb | 21 | M4 |
| 2 | Down | 12 | Lt | 22 | M5 (LM) |
| 3 | Left | 13 | Rt | 23 | M6 (RM) |
| 4 | A | 14 | Thl (нажатие левого стика) | 24 | Menu |
| 5 | B | 15 | Thr | 25 | Turbo |
| 6 | Select | 16 | C (нет у Apex 5) | 27 | Home |
| 7 | X | 17 | Z (нет) | 28 | Back (нет) |
| 8 | Y | 18 | M1 | 32 | **Macro** (метка в таблице кнопок) |
| 9 | Start | 19 | M2 | 254 | **KeyboardMouse** (метка «клавиатура или мышь») |
| | | | | 255 | **None** (в таблице кнопок: «на себя») |

Направления стика: 160 Center, 161 Up, 162 RightUp, 163 Right, 164 RightDown, 165 Down, 166 LeftDown, 167 Left, 168 LeftUp. Стики как целое: 240 JsLeft, 241 JsRight, 242 JsWheel.

Apex 5 поддерживает 27 кнопок: JsLeft, JsRight, Thl, Thr, A, B, X, Y, четыре направления крестовины, Lt, Lb, Rt, Rb, Select, Start, Home, Menu, Turbo, M1..M6 (`FlydigiControllerFactory.cs:525`, `GenerateControllerApex5`). Моторов в курках у Apex 5 нет: `IsSupportTriggerVibration` не выставлен.

### 6.2 Назначения

| Перечисление | Значения |
|---|---|
| `KeyMapType` | Key 0, Continuous 1 (турбо), Macro 2, MultiFunction 3, Keyboard 4 |
| `KeyMapTypeContinuousEnableType` (режим турбо, байт +1 записи кнопки) | Close 0, Press 1 (пока удерживается), Click 2 (переключатель) |
| `JoystickMapType` | Joystick 0, Keyboard 1, Mouse 2, DPad 3 |
| `JoystickMapKeyboardDirection` | Four 0, Eight 1 |
| `JoystickCircularityType` | Rectangle 0, Circular 1 |
| `JoystickSensitivityType` | Default 0, Quick 1, Slow 2, Custom 3 |
| `MotionUseMode` | FPS 0, RACER 1 |
| `MotionMapType` | Off 0, LeftJoystick 1, RightJoystick 2, Mouse 3 (только на ПК) |
| `MotionEnableType` | Click 0, Press 1 |
| `TriggerType` | Normal 0, AdapterTrigger 1, VibrationTrigger 2 |
| `AdapterTriggerType` | Normal 0, Race 1, Sniper 2, Recoil 3, Lock 4, Vibration 5 |
| `MacroEnableType` | None 0, Once 1, Press 2, Click 3 |
| `MacroActionEvent` | Release 0, Press 1, LeftJoystick 2, RightJoystick 3, Hold 5 |
| `VibrationType` | Both 0, Grip 1, Trigger 2, Grip_Trigger 3 |

### 6.3 Подсветка, устройства, настройки

| Перечисление | Значения |
|---|---|
| `LedType` (байт 8 блока подсветки) | Unknown 0, FLOW 1, BREATH 2, GRADIENT 3, FEEDBACK 4, ON 5, Close 6, Default 7 |
| `ConfigType` | All 0, Simple 1, Led 2, Button 3, Macro 4, Joystick 5, Motion 6, Trigger 7, Vibration 8 |
| `JoystickSensitivity` | Highest 14, High 15, MiddleHigh 16, Middle 17, LowMiddle 18, Low 19, Lowest 20 |
| `JoystickPrecision` | 8Bit 1, 10Bit 2, 12Bit 3, 9Bit 4, 11Bit 5, 14Bit 6, 16Bit 7 |
| `ReportRate` | 1000 Гц 1, 500 Гц 2, 250 Гц 4, 125 Гц 8 |
| `ChipType` | Unknown 0, Wch 1, Telink 2, Krly 3, NearLink 4, Megahunt 5, Puya 6, ESP 7, FREQ 8, JIELI 9 |
| `ChipModule` | MAIN 0, RF 1, SI 2, SCREEN 4, TRIGGER 5, DONGLE 6, ADC 7, LED 8 |
| `ConnectType` | Wired 1, Dongle 2, Bluetooth 3 |

Типы устройства Apex 5 (`D/Flydigi.ControllerSdk/Flydigi.ControllerSDK.data.model/DeviceType.cs:53-59`). Различаются только оформлением; всё, кроме 128, в SDK считается лицензионной редакцией (`IsIp`):

| deviceType | Имя | Заводская подсветка |
|---|---|---|
| 128 | K5 | анимированная радуга, 10 кадров |
| 129 | K5Eva | статичная, оранжевый и зелёный |
| 133 | K5Mm | статичная, красная |
| 134 | K5Srs | статичная, пурпурная |
| 135 | K5GS | статичная, два цвета |
| 136 | K5LZ | статичная, оранжевая. В `GetDeviceCodeById` не сопоставлен с "k5" (`FlydigiControllerUtil.cs:181-186`) |

Коды клавиатуры (`KeyboardKey`) — это значения Windows `System.Windows.Forms.Keys`: A..Z = 65..90, F1..F24 = 112..135, модификаторы флагами Shift 0x10000, Control 0x20000, Alt 0x40000. Собственные коды Flydigi: WheelUp 433, WheelDown 434. На геймпад они не попадают.

---

## 7. Формат профиля

Наш формат описан в `docs/spec-profile.md`. Код Flydigi подтвердил раскладку 840 байт. Ниже то, что добавилось или разошлось.

### 7.1 Две формы профиля

1. **Protobuf `ControllerMappingConfigBean`** — так профиль живёт на ПК: в заводских `.dat`, кэше и обмене профилями. В нём есть поля, которых нет на геймпаде: подсветка, клавиатурные коды, параметры мыши.
2. **840 байт на геймпаде** (протокол v3.1, `protoVersion` = 769). Перевод в обе стороны — `D/Flydigi.ControllerSdk/Flydigi.ControllerSDK.data.parser/MappingConfigParser.cs` (дальше MCP).

### 7.2 Раскладка 840 байт по коду Flydigi

Чтение — MCP:30-53 и 646-651, запись — MCP:55-91 и 707-715.

| Смещение | Размер | Поле | Кодировка |
|---|---|---|---|
| 0 | 2 | `protoVersion` | u16 LE, 769 = v3.1 |
| 2 | 1 | `packageCount` | 77; хранится, но нигде не используется. 77/79/84 — число 10-байтных пакетов у v2.0/v3.0/v3.1 |
| 3 | 10 | `oldLedConfig` | сырые байты, переносятся |
| 13 + 3k | 3 × 32 | кнопка k (`ControllerKey` 0..31) | +0 цель, +1 режим турбо, +2 частота турбо (0 = без турбо) |
| 109 + 7s | 7 × 2 | стик s (0 левый, 1 правый), core | +0 тип чувствительности; +1 центр; +2,+3 точка 1; +4,+5 точка 2; +6 край |
| 123 + 7s | 7 × 2 | курок s | +0 тип, +1 начало, +2..+5 точки, +6 конец; 0..255 |
| 137 | 8 | гироскоп | [0] тип назначения; [1] кнопка включения 1; [2] режим включения; [3] мёртвая зона; [4],[5] чувствительность (пишутся одинаково); [6] FPS/RACER; [7] кнопка включения 2 |
| 145 | 9 | вибрация рукояток | [0] общий флаг (**0 = вкл, 255 = выкл**); левая [1..4], правая [5..8]: вкл, min, max, scale |
| 154 | 29 | моторы курков | [0] флаг (**0 = вкл, 1 = выкл**), далее linear и micro по сторонам |
| 183 | 2 | `lunpan` (колесо) | сырые |
| 185 + 20s | 20 × 2 | адаптивный курок s | +0 тип (`AdapterTriggerType`); +1 тип привязки к вибрации (пишется `тип == 5 ? 2 : 0`); +2 фильтр; +3 масштаб; +4..+8 параметры привязки; +9 граница; +10..+19 параметры |
| 225 | 2 | `dataVersion` | u16 LE, случайная метка кэша; 0xFFFF = слот не записывался |
| 230 | 538 | макросы | см. spec-profile §4.2 |
| 770 | 20 | название | UTF-16LE, обрезка по 20 **байтам** (суррогатная пара может разрезаться), хвост 0xFF |
| 790 + 12s | 12 × 2 | стик s, банк | +0 тип (пишется, при чтении игнорируется); +1..+9 девять точек; +10 круг/квадрат; +11 край |
| 820 | 5 | интервалы макросов | мс / 10 |
| 830 | 6 | кривая гироскопа | zero, p1.x, p1.y, p2.x, p2.y, end |

Байты 227..230, 768..770, 814..820, 825..830 и 836..840 Flydigi всегда записывает как 0xFF.

### 7.3 Таблица кнопок (MCP:567-639)

- Запись: турбо → `[цель, режим, частота]` (цель всегда явная); макрос → `[32, 0, 0]`; клавиатура или «Особый» → `[254, 0, 0]`; «на себя» → `[255, 0, 0]`; иначе `[цель, 0, 0]`.
- Чтение: `b0 == 32` → макрос; иначе `b2 > 0` → турбо; иначе кнопка (`b0 > 32` значит «на себя», так 254 и 255 читаются как «на себя», а клавиатурный код подтягивается из кэша на ПК).

### 7.4 Стики

- Если стик назначен не на «Джойстик», служба пишет в core-байт центра 127. Остальное делает служба на ПК.
- Отрицательные центр и край Flydigi пишет в дополнительном коде, а читает как `127 - b`. Поэтому в core их не писать: прошивка всё равно играет банк точек [HW].
- Служба при записи пересчитывает x точек core через `CalculatePoint`, обратного пересчёта нет. На поведение не влияет: core инертен.
- Интерфейс считает банк из 9 точек с `Math.round`, а мы отбрасываем дробную часть. Заводской банк `50 62 75 87 100 112 125 137 150` совпадает с нашим способом. После правки в Space Station там будет `…63…88…113…138` (разница не больше 1).
- Ещё одно отличие интерфейса: y внутренней точки масштабируется в диапазон `start.y..end.y`. Это важно только при отрицательных центре и крае (анти-мёртвая зона).

### 7.5 Подсветка (отдельный блок, не в 840 байтах)

Читается командой 167, пишется 168/169 с `cfgId` = номер слота. Разбор: `D/Flydigi.ControllerSdk/Flydigi.ControllerSDK.data.parser/LedConfigParser.cs:95-176`.

| Смещение | Поле | Примечание |
|---|---|---|
| 0..1 | версия | `00 03` (768). При записи Flydigi пишет `version & 0xF` вместо `& 0xFF` — для 768 это всё равно 0 |
| 2 | clickFeedback | 1 только в режиме FEEDBACK |
| 3 | loopStart | |
| 4 | loopEnd | |
| 5 | loopTime | скорость |
| 6 | яркость | 0..100 |
| 7 | число светодиодов | 12 |
| 8 | режим | `LedType` 1..7 |
| 9 | gripSync | свет при вибрации |
| 10..19 | — | 0xFF |
| 20.. | кадры | 12 × RGB на кадр; у заводских 10 кадров, всего 380 байт |

Как интерфейс Flydigi превращает режим в кадры (`ControllerDataMapper.cs:158-395`):

| Режим | Кадры | loopEnd |
|---|---|---|
| FLOW | готовые кадры `LedConstants.FlowK5Std` | 12 |
| BREATH | цвет i в кадр 2i, нечётные кадры чёрные | 2n − 1 |
| GRADIENT | цвет i в кадр i | n − 1 |
| ON, FEEDBACK | цвет только в кадр 0 | 0 / n |
| Close | все кадры чёрные | 0 |
| Default | копия из `default_mapping_<deviceType>.dat` | как в файле |

### 7.6 Заводские профили Apex 5

В каждом `default_mapping_<deviceType>.dat` 4 одинаковых слота «配置1…4». Общие значения: все кнопки «на себя», стик p1 (63,63), p2 (127,127), банк `50 62 75 87 100 112 125 137 150`, курки 0..255, гироскоп: чувствительность 25, мёртвая зона 4, вибрация рукояток L min 60 / R min 80.

В 840 байтах модели различаются только **байтом 154**: у 128/129/133/134 моторы курков выключены (1), у 135/136 включены (0). Это совпадает с spec-profile §3.13. Остальные различия — заводская подсветка (§6.3) и чувствительность мыши (только на ПК).

### 7.7 Как Flydigi читает и пишет профиль

| cmd | Данные | Ответ |
|---|---|---|
| 161 | — | активный слот, метки dataVersion всех 4 слотов. В SDK называется `ReadMappingConfigVersionAll`; в нашей spec — `ReadCurrentMappingConfigId`, стоит переименовать |
| 162 | cfgId | подтверждение |
| 163 | cfgId, 20 | несколько пакетов: всего, номер, cfgId, 20 байт данных |
| 164 | cfgId, начальный пакет, число пакетов, 20 | подтверждение |
| 165 | номер пакета, 20 байт | подтверждение |
| 166 | dataVersion (2 байта) | подтверждение, тайм-аут 10 с |
| 167 / 168 / 169 | то же для подсветки | |
| 171 | dataVersion, cfgId | Switch-слот; длина 4, cfgId в сумму не входит |
| 175 | cfgId (всегда 0) | сброс всех профилей |

Отличия от нашей реализации:

- Полную запись Flydigi делает 84 пакетами по 20 байт: 1680 байт, где 840..1679 — 0xFF. У нас 42 пакета, и это корректно.
- Пакет 165, на который не пришёл ответ, Flydigi **пропускает** и продолжает передачу. Мы прерываем передачу, так правильнее.
- Подтверждение = совпадение номера команды; флаг успеха и сумма ответа не проверяются. 500 мс, 3 попытки.
- Каждое изменение через 250 мс автоматически сохраняется (166) с новой случайной меткой `Random.Next(65535)`.
- На ПК кэш сверяется с геймпадом по меткам dataVersion: перечитываются только слоты с изменившейся меткой. Так как 163 переключает слот, после чтения служба возвращает активный слот.
- Запись в Switch-слот (171) Flydigi не откатывает: исходный профиль в RAM не восстанавливается. Перед записью он сбрасывает клавиатурные назначения и типы стиков.

### 7.8 Файлы на ПК и обмен профилями

Корень: `<папка службы>\Configs\Controller\k5\`.

| Путь | Содержимое |
|---|---|
| `default\default_mapping_<deviceType>.dat` | `ControllerMappingConfigBeans`, 4 заводских слота |
| `default\default_screen_image_<deviceType>.bin` | заводская анимация экрана |
| `onboard\<uid>.dat` | кэш всех 4 слотов геймпада |
| `cache\<uid>.dat` | текущий профиль |
| `local\index.dat` | список профилей на ПК (`LocalConfigs`) |
| `local\<unix-ms>.dat` | профиль на ПК (`ControllerMappingConfigBean`) |
| `local\led_<uid>.dat` | пресеты подсветки интерфейса |
| `..\macro\index.dat`, `..\macro\<ms>.dat` | библиотека макросов, общая для всех моделей |

- **Обмен профилем**: строка `BitConverter.ToString(bean.ToByteArray())`, то есть protobuf-профиль в hex через дефис: `0A-1B-…`. Это **не** 840 байт: в строке есть и подсветка, и клавиатурные назначения. Импорт: разбить по «-», hex → байты, `ParseFrom`.
- **Обмен макросом**: байты `MacroItem` через дефис, но в **десятичной** записи.
- `hashCode` в `DeviceConfig` используется только для макросов: `md5(MacroItem.ToByteArray())`. Так служба отбрасывает дубликаты.

### 7.9 Что поправить в `docs/spec-profile.md`

| Место | Что сказать |
|---|---|
| §3.0 | Flydigi не переносит неизвестные байты, а пишет в них 0xFF (список в §7.2). `packageCount` — число 10-байтных пакетов у старых версий |
| §3.3 | Байт края (+6) Flydigi записывает: какой прочитал, такой и пишет. x точек core пересчитывается через `CalculatePoint`. Интерфейс округляет банк, а не отбрасывает дробь |
| §3.5 | Правило «байт 144 только при Press» — это правило интерфейса Flydigi, служба пишет всегда |
| §3.6 | Flydigi при записи не меняет min и max местами. Интерфейс пишет одно значение в обе стороны, поэтому заводская асимметрия L60/R80 теряется |
| §3.7 | Конкретика по моделям: 128/129/133/134 — выкл (1), 135/136 — вкл (0) |
| §3.8 | «Normal пишет только байт режима» неверно: Flydigi пишет ещё `param[0] = start`, `param[1] = end`, `param[2..4] = 0`. Вреда нет: для Normal параметры не используются |
| §3.9 | Хвост названия Flydigi заполняет 0xFF, а не 0x00 |
| §4.2 | Число шагов SDK читает только из младшего байта. Время шага при записи переполняется по модулю 65536, а не ограничивается |
| §2.0, §2.5 | Полная запись у Flydigi 84 пакета. Наш код (`src/hid.rs`) пишет один отрезок от первого до последнего изменённого пакета, а spec описывает несколько отрезков |
| §2.6 | При правке подсветки Flydigi меняет метку dataVersion (автосохранение) |
| §2.8 | Сброс одного слота у Flydigi переписывает и подсветку слота |

---

## 8. Прошивка

### 8.1 Чипы Apex 5

| Чип (`ChipModule`) | Что это | Тип | Чем шьётся | Загрузчик на USB |
|---|---|---|---|---|
| MAIN 0 | основной MCU | Megahunt | `hid_boot_command.exe` | HID `0483:5720`, feature-отчёты 1–5 |
| RF 1 | радиочип NearLink (SparkLink, «星闪») | NearLink (HiSilicon BS21/BS2X) | `BurnTool.exe -dfu` | HID того же VID/PID, usage page 0xFFEF |
| SI 2 | Switch-интерфейс | JieLi | `jl_firmware_upgrade_x64.dll` | USB-накопитель, SCSI-команды JieLi |
| SCREEN 4 | чип экрана | FreqChip | `OtaNewUpdater` в FirmwareConsole | CDC-порт `FFAA:5555`, 921600 бод |
| DONGLE 6 | приёмник | NearLink | `BurnTool.exe -dfu` | HID, usage page 0xFFEE |

Курков с моторами и отдельного АЦП у Apex 5 нет. Источник: `FlydigiControllerFactory.cs:525-627`.

### 8.2 Версии

Все версии приходят в одном ответе на команду 1 (`03 5A A5 01 02 03`), разбор в `HeartBeatCommandFactory.cs:104-215`. Раскладка совпадает с `docs/spec-device.md` §1.3:

| raw | Поле |
|---|---|
| 6 | deviceType |
| 7 | подключение: 1 провод, 2 донгл |
| 13 | младший полубайт — тип основного MCU (5 = Megahunt) |
| 16–17 | main |
| 18–19 | dongle (нули по проводу) |
| 20–21 | SI |
| 22–23 | trigger (у Apex 5 нет) |
| 24–25 | screen |
| 26–27 | ADC (у Apex 5 нет) |
| 28–29 | RF (NearLink) |

Версия — два байта, четыре полубайта: `70 45` → 7.0.4.5.

Пороги версии main, от которых зависят функции:
- **7.0.2.8** — запись в Switch-слоты;
- **7.0.2.9** — макросы в окне назначения кнопки;
- **7.0.3.0** — новый протокол гироскопа, разрешение сторонним приложениям, гироскоп → мышь.

Служба сравнивает версии как строки (`DeviceUtil.cs:35-38`), поэтому на `7.0.10.0` сравнение сломается. Сравнивать нужно числами.

### 8.3 Как Space Station обновляет прошивку

1. Проверка обновлений: `POST https://api.flydigi.com/pc/Update/firmware` (§3). Обновление предлагается, если версия на сервере **отличается** от текущей, в том числе если она старше.
2. Прошивать можно только по проводу; донгл — через донгл. Иначе интерфейс показывает «请先有线连接手柄».
3. Интерфейс шлёт IPC `SwitchUsb` (15), служба отправляет HID-команду **31**: `03 5A A5 1F 03 <chipModule> <сумма>`, ответа не ждёт.

   | Чип | Пакет |
   |---|---|
   | main | `03 5A A5 1F 03 00 22` |
   | RF | `03 5A A5 1F 03 01 23` |
   | SI | `03 5A A5 1F 03 02 24` |
   | screen | `03 5A A5 1F 03 04 26` |
   | dongle | `03 5A A5 1F 03 06 28` |

4. Через 5 с Electron запускает:

   ```
   FirmwareConsole.exe --device_id k5 --chip_module <N> --chip_type <N> --url <url> --vendor_id 37d7 --product_id <pid>
   ```

5. FirmwareConsole скачивает файл в `%TEMP%` (сначала curl, потом HttpClient). ZIP распаковывается, берётся первый `*.bin`, `*.ufw` или `*.hex`. **Ни md5, ни подписи не проверяются.** Повторов нет, общего тайм-аута нет.
6. Electron читает вывод: `Progress: N%` — прогресс, `success` — готово, `failed` или любой вывод в stderr — ошибка, процесс убивается.
7. После успеха интерфейс 10 с показывает «геймпад перезагружается» и заново читает версии.

**Опасная особенность**: команда 31 уходит **до** скачивания файла. Если скачивание не удалось, геймпад остаётся в загрузчике.

### 8.4 Загрузчик основного MCU (Megahunt)

Протокол восстановлен дизассемблированием `hid_boot_command.exe` (MinGW + hidapi, 31 КБ). VID/PID в утилиту не передаются, она ищет `0483:5720` сама.

| Шаг | Вызов | Байты (первый — report id) | Смысл |
|---|---|---|---|
| 1 | `send_feature_report` 9 Б | `01 00 00 00 00 00 00 00 00` | указатель в 0 [вывод] |
| 2 | `send_feature_report` 5 Б | `04 <размер u32 LE>` | **стирание** |
| 3 | `send_feature_report` 63 Б, в цикле | `02 <длина ≤ 60> 00 <данные>` | запись |
| 4 | `send_feature_report` 13 Б | `03 A5 A5 A5 A5 <crc32 LE> <размер LE>` | флаг «прошивка цела» |
| 5 | `send_feature_report` | `01 …` | указатель снова в 0 |
| 6 | `get_feature_report` id 2, в цикле | данные с `[3]` | **чтение флеша** и сравнение с файлом |
| 7 | `get_feature_report` id 3 | ожидается `A5A5A5A5`, crc, размер | проверка флага |
| 8 | `send_feature_report` 5 Б | `05 A5 A5 A5 A5` | запуск приложения |

- Каждый отчёт повторяется до 3 раз.
- CRC — CRC-32/MPEG-2: полином 0x04C11DB7, старший бит первым, начальное значение 0xFFFFFFFF, без финального XOR, по всему файлу.
- Файл — сырой `.bin` без заголовка, пишется с адреса 0.
- Загрузчик умеет **читать флеш обратно** (шаг 6). Значит, в режиме загрузчика можно снять дамп текущей прошивки [вывод].

### 8.5 Остальные чипы

- **RF и донгл (NearLink)**: `BurnTool.exe -dfu -pid:"0x<PID>" -vid:"0x<VID>" -usage:"0x1" -usagepage:"0x<FFEF|FFEE>" -bin:"<файл>"`. Формат пакетов HiSilicon HID-DFU не восстановлен. В библиотеке есть AES и «signature frame», так что образы `.fwpkg` могут быть подписаны [вывод]. Ошибка FirmwareConsole: после выхода BurnTool он всегда пишет «Success!», даже если прошивка не удалась.
- **Экран (FreqChip)**: тот же последовательный протокол, что и для картинок (§9.4), но с опкодами 1/2/3/5/9 и записью в область программы. После прошивки экрану нужно около 15 с, потом он перезагружается.
- **SI (JieLi)**: файлы `.ufw` через `jl_firmware_upgrade_x64.dll`; устройство в режиме обновления видно как USB-накопитель. **Если файл не `.ufw`, FirmwareConsole просто запускает его как программу** (`ExeUpdater`). Восстановление по инструкции Flydigi: удерживать START 8 с.

### 8.6 Что можно и чего нельзя делать на Linux

Можно и безопасно:
- Читать версии командой 1. Это уже есть в Apex 5 Control.
- Проверять наличие обновлений запросом к `api.flydigi.com`, но только с согласия пользователя: запрос отправляет версии и тип устройства в Flydigi. Для этого не нужен геймпад.

Нельзя:
1. Отправлять команду 31 с любым чипом, кроме экрана (4) по проводу. Main уходит в загрузчик `0483:5720`, RF и донгл — в неизвестный протокол HiSilicon, SI — в режим накопителя JieLi. Без рабочего прошивальщика выхода нет.
2. Отправлять отчёт 4 (стирание) в `0483:5720` «для пробы».
3. Повторять схему Flydigi «сначала загрузчик, потом скачивание». Образ должен быть скачан и проверен **до** команды 31.
4. Прошивать через донгл что-либо, кроме донгла.
5. Запускать скачанные `.exe`.

Особенности Linux [вывод]:
- ModemManager опрашивает новые `ttyACM` AT-командами. Для `FFAA:5555` нужно правило udev `ENV{ID_MM_DEVICE_IGNORE}="1"`.
- Накопитель JieLi может подхватить udisks; нужно `ENV{UDISKS_IGNORE}="1"`.
- `0483:5720` — VID ST, его используют и другие устройства; без контекста не открывать.
- Проброс USB в виртуальную машину при переподключении устройства — частая причина «кирпича».

Если когда-нибудь делать прошивку main на Linux: протокол из §8.4 простой, но неизвестно, проверяет ли загрузчик что-то кроме флага и как ведёт себя при неполной записи. Безопаснее шить main из Windows официальным приложением.

---

## 9. Своя картинка или анимация на экране

Путь подтверждает `docs/spec-device.md` §4.4 (там же проверено на железе). Здесь — что делает Space Station.

### 9.1 Экран

| Параметр | Значение |
|---|---|
| Разрешение | 160 × 80, альбомная ориентация |
| Пиксель | RGB565, **старший байт первым** |
| Кадр | формат LVGL v8: заголовок `04 80 02 0A` (cf = 4, w = 160, h = 80), затем пиксели; 25604 байта |
| Анимация | кадры подряд, без сжатия и без контейнера |
| Скорость | один интервал на все кадры; на геймпад уходит байт `round(мс / 10)` |
| Кадров | интерфейс ограничивает 150 (у Apex 4 — 35); в протоколе байт, то есть до 255 |
| Слотов | один; каждая загрузка заменяет предыдущую |
| Яркость, удаление | команд нет |
| Ограничение по прошивке | нет, проверяется только `supportScreen` |

Цвет квантуется округлением: красный и синий — шагами по 8 (максимум 248), зелёный — по 4 (максимум 252). Дизеринга нет. Прозрачные пиксели становятся чёрными.

Заводские анимации: `default_screen_image_<deviceType>.bin`, у 128 — 137 кадров, 129 — 132, 133 — 85, 134 — 10, 135 — 144, 136 — 42.

### 9.2 Что делает интерфейс

1. Принимает только jpg, png и gif. Видео нет (`ffmpeg.dll` — обычная библиотека Chromium).
2. GIF раскладывается на кадры через WebCodecs `ImageDecoder`. Собственные задержки кадров теряются: берётся средний интервал, который можно поменять в пределах 1…2500 мс.
3. Пользователь выбирает окно 2:1 и масштаб 1…20. Каждый кадр рисуется в canvas 160 × 80 и переводится в LVGL-кадр портом `lv_img_conv`.
4. Без проводного подключения загрузка не начинается.
5. Интерфейс отправляет в main-процесс сообщение `upload_pic2screen` с кадрами и интервалом.
6. Для `deviceCode == "k5"` main-процесс отправляет службе `SwitchUsb` (команда 31 для чипа 4), склеивает кадры в `%TEMP%\<время>.bin` и через 5 с запускает:

   ```
   FirmwareConsole.exe --device_id k5 --chip_module 4 --chip_type 8 --url <файл> --vendor_id 37d7 --product_id <pid>
     --pic_type <1 анимация | 2 картинка> --pic_num <N> --frame_rate <round(мс/10)> --upgrade_type 2 --is_restore_default <0|1>
   ```

### 9.3 Официальная подборка и «Восстановить по умолчанию»

- Подборка: `GET https://api.flydigi.com/pc/screen_pic/list?version=0`, ответ — список `{id, title, imagePath}`. `imagePath` — обычный GIF или PNG, который проходит тот же путь, что и свой файл. Своего формата на сервере нет.
- «Сообщество» показывается только на китайском и просто открывает ссылку `http://data.flydigi.com/api/web/link?u=1700644138`.
- «Восстановить по умолчанию»: заводской `default_screen_image_<deviceType>.bin` загружается с `is_restore_default = 1` и `frame_rate = 0`. Вероятно, этот флаг включает заводскую логику анимации (у EVA она меняется во время игры) [вывод]. Для своих анимаций флаг должен быть 0.

### 9.4 Протокол последовательного порта (`OtaNewUpdater.cs`)

1. Команда 31 для экрана: `03 5A A5 1F 03 04 26`, один раз, без ожидания ответа. Только по проводу: через донгл геймпад зависает до выключения питания [HW].
2. Ждать порт `FFAA:5555` (Flydigi: до 10 попыток через 3 с). Параметры: 921600 бод, 8N1, без управления потоком, DTR = 1, RTS = 1.
3. Запрос: `[опкод u8][поле длины u16 LE][данные]`. Поле длины — константа для каждого опкода. Ответ: `[результат][опкод][длина u16 LE][данные]`; результат Flydigi не проверяет.

| Шаг | Опкод | Поле длины | Данные | Ответ |
|---|---|---|---|---|
| 1 | 11 PicGetBaseAddr | `04 00` | pic_type, pic_num, frame_rate, is_restore_default | базовый адрес u32 LE в `[4..8]` (на железе `0x002FF000`) |
| 2 | 10 PicGetVersion | `06 00` | — | игнорируется |
| 3 | 3 EraseSector | `06 00` | адрес = база + i·4096 | по одному на каждые 4096 байт |
| 4 | 5 WriteData | `40 00` | адрес = база + j·55, `37 00`, до 55 байт | последний пакет короче, поля длины те же |
| 5 | 12 PicResetDevice | `08 00` | размер u32 LE, CRC u32 LE | конец сессии |

- Каждый следующий запрос — после ответа на предыдущий. Тайм-аут ответа 18 с. Повторов нет: тайм-аут прерывает всю загрузку.
- CRC — своя версия CRC32 Flydigi (таблица 0xEDB88320, начальное 0, без финального XOR, вычисления в знаковом int32). Готовый код на Rust есть в `docs/spec-device.md` §4.4.
- Один кадр — около 7 стираний и 466 записей, примерно 25 с; 150 кадров — около часа [HW].
- После загрузки экрану нужно около 15 с, затем геймпад перезагружается сам. Питание не трогать.

### 9.5 Рецепт для Apex 5 Control

```text
кадры   = декодировать(файл)                       // png/jpg — 1 кадр, gif — все кадры
кадры   = первые min(150, N)
интервал = выбор пользователя или средний из gif; 10..2550 мс
для каждого кадра:
    обрезать 2:1, уменьшить до 160×80, наложить на чёрный
    out = 04 80 02 0A
    r,b = min(round(v/8)*8, 248); g = min(round(v/4)*4, 252)
    px = (r<<8) | (g<<3) | (b>>3); out += [px>>8, px & 0xFF]
blob = все кадры подряд (25604 × N)

проверить: подключение по проводу (cmd 1, raw[7] == 1), есть правило udev для ffaa:5555
если порт ffaa:5555 уже есть — сразу к загрузке (повтор после сбоя без второй команды 31)
отправить 03 5A A5 1F 03 04 26 (один раз), закрыть hidraw, ждать порт до 30 с

открыть порт 921600 8N1, DTR=1, RTS=1
база = xfer(0B 04 00 <тип> <N> <round(мс/10)> 00)[4..8]   // проверить: не 0, кратна 4096
xfer(0A 06 00)
для i: xfer(03 06 00 <база + i*4096>)
для j: xfer(05 40 00 <база + j*55> 37 00 <до 55 байт>)
xfer(0C 08 00 <размер> <crc>)
// ждать перезагрузки; затем при необходимости cmd 19 sub 9 = 1 (показывать анимацию)
```

Риски:
1. Команда 31 — только с чипом 4 и только по проводу.
2. Никогда не отправлять опкоды 1/2/9 и режимы `PROGRAM`/`SYS_PIC`: они пишут программу чипа экрана.
3. Писать и стирать только относительно базы, которую вернул чип. Неправдоподобная база — сразу прекратить.
4. Не загружать в цикле и не повторять автоматически: каждая загрузка стирает флеш.
5. `frame_rate = 0` и больше 255 кадров не отправлять.
6. Во время загрузки не держать hidraw геймпада открытым.
7. HID-команды 208–211 и TestScreen (242) не использовать: на Apex 5 они не работают, а 211 портит сохранённую картинку [HW].

---

## 10. Безопасность: WriteDeviceType и другие опасные команды

### 10.1 Что такое WriteDeviceType

- IPC 65529 → HID-команда **254**: `03 5A A5 FE 03 <deviceType> <сумма>` (`D/Flydigi.ControllerSdk/Flydigi.ControllerSDK.data.command/WriteDeviceTypeCommandFactory.cs:17-34`). Для старых протоколов это cmd 80 sub 15 (XInput) и cmd 245 sub 5 (DInput).
- Сразу после подтверждения служба отправляет перезагрузку (cmd 29) (`ControllerSdk.cs:483-486`, `ControllerRepository.cs:2262-2273`).
- Значение никак не проверяется: число из IPC приводится к `DeviceType` без проверки диапазона и без сверки с текущей моделью.

### 10.2 Когда Space Station её отправляет

Код в `UI/index-BA01cjWW.js` (функция `Yk`):

```js
{run:s} = vl(L8, {manual:!0, onSuccess:(f,m) => {
  f.data.valid || Kn({cmdId: IpcCommandEnum_WriteDeviceType,
                      category: m[0].category, deviceType: f.data.deviceId, uid: m[0].uid})
}})
L8 = async e => wa.post("/space-api/product-sn/device-id", e)   // https://edge.flydigi.com
```

- Запрос уходит **при каждом подключении** устройства и при обновлении списка устройств, независимо от входа в аккаунт. В нём весь объект устройства: uid, deviceType, MAC, версии прошивок, VID/PID.
- Если сервер ответил `valid:false`, геймпаду записывается тип из ответа сервера, и геймпад перезагружается.
- Без сети запрос падает и команда не уходит. В нашем стенде заглушка отвечала `{"data":[]}`, поэтому `valid` было `undefined`, и команда отправлялась с `deviceType: undefined`. Со страницей «Настройки» это не связано.
- Назначение, по-видимому, — исправлять редакцию (128/129/133–136) по базе серийных номеров [вывод].
- Опасный случай: если сервер ответит `{valid:false}` без `deviceId`, служба запишет 0 (`Unknown`) и перезагрузит геймпад [вывод].

### 10.3 Последствия

- Тип почти наверняка хранится в энергонезависимой памяти: иначе не нужна перезагрузка [вывод].
- USB VID/PID, судя по коду, не меняются. Но по этому байту модель определяют и Space Station, и Apex 5 Control.
- При чужом типе Apex 5 Control откажется работать с геймпадом (`Info::supported()`, `src/device.rs:40`). Space Station сочтёт его другой моделью и может предложить чужую прошивку — это риск «кирпича» [вывод].
- Вернуть можно той же командой с прежним значением, пока вендорский интерфейс ещё доступен.

### 10.4 Рекомендации для `src/safety.rs`

254 уже запрещена: её нет в белом списке. Стоит закрепить это явно:

```rust
254 => bail!("blocked: device-type rewrite (254) changes the pad's model id"),
240..=253 => bail!("blocked: factory/test command {cmd}"),
```

- Добавить 28, 240, 241, 244, 246, 247, 253 и 254 в тест `dangerous_commands_are_refused`.
- Никогда не реализовывать «исправление редакции». Тип только читать. Если он вне 128/129/133–136, показывать понятную ошибку, например «тип изменён программой Flydigi», и ничего не писать.
- В README стоит упомянуть, что Space Station при каждом подключении отправляет данные геймпада на `edge.flydigi.com` и по ответу сервера может переписать тип модели.

### 10.5 Все служебные команды (NewXInput)

| cmd | Что делает | Пакет |
|---|---|---|
| 28 | захват геймпада сторонней программой | `1C 17 <1/0> <20 байт ASCII-метки> …` |
| 29 | перезагрузка | `1D 02 1F` |
| 31 | загрузчик чипа | `1F 03 <chip> …` |
| 175 | сброс всех профилей | `AF 03 00 …` |
| 240 | калибровка АЦП (у Apex 5 АЦП-прошивки нет) | `F0 03 <1 старт / 2 стоп>` |
| 241 | тест индикатора | `F1 07 led freq times R G B` |
| 242 | заливка экрана и подсветки, «залипает» [HW] | `F2 05 on R G B` |
| 244 | тест радио и потерь донгла | `F4 03 <1/2>` |
| 245 | тест подсветки, ничего не меняет [HW]; в протоколе DInput это вход в загрузчик | `F5 05 R G B` |
| 246 | заводской режим стиков | `F6 06 <1/2>` |
| 247 | прогон моторов курков (может вибрировать) | `F7 03 <1/2>` |
| 253 | **заводской сброс** (у Flydigi сумма записана не в тот байт) | `FD 02 …` |
| 254 | **перезапись типа устройства** | `FE 03 <type>` |

Интерфейс Flydigi из этого списка вызывает только 29 (после 254), 31 (прошивка и экран) и 175 («Восстановить по умолчанию»). В Apex 5 Control все они запрещены веткой по умолчанию в `src/safety.rs:50`. Новых опасных команд, кроме уже известных, декомпиляция не показала.

---

## 11. Что проверить на железе

- ~~Хранит ли Apex 5 подсветку отдельно по слотам~~ — да, и чтение 167 переключает активный слот [HW], см. §2 п. 3.
- Что именно выключает «Turbo Function» (sub 4): только турбо или ещё переназначения и макросы.
- ~~Влияет ли байт 8 блока подсветки (режим) на работу прошивки~~ — да, см. §2 п. 2 [HW]. Как именно прошивка трактует каждый код, не изучено.
- Сохраняются ли настройки команд 19–23 после сна и выключения.
- Как прошивка ведёт себя при `frame_rate = 0` без флага `is_restore_default`.
- Сколько кадров реально помещается в память экрана (выше 150).
