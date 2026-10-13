# Translations

The app's texts are written in English in the code. Each file here gives them in
another language: `ru.json` is Russian. Every `*.json` in this folder is built into
the app and shows up in Settings → App → Language.

```json
{
  "language": "Русский",
  "strings": {
    "Save": "Сохранить",
    "Profile {slot} active": "Активен профиль {slot}",
    "Off#turbo": "Выкл"
  }
}
```

- `language` is the language's own name, as the language menu shows it.
- The keys are the English texts. Don't change them; translate the values.
- `{slot}`, `{count}` and the like are filled in by the app. Keep them, in any order
  the sentence needs. Write `{{` and `}}` for a brace.
- A key may end in `#context`, like `Off#turbo` and `Off#gyro`: the same English
  words in places that your language may say differently. English shows the part
  before `#`.
- An empty or missing value shows the English text.

## Adding a language

1. Make a template with every text, empty (the code is ISO 639-1, like `de`, or
   with a region, like `pt_BR`):

   ```sh
   tools/i18n.py update de --language Deutsch
   ```

2. Fill in `locales/de.json`. To see it without rebuilding the app, copy it to
   `~/.config/apex5-control/locales/` and restart the app. Files there are read on
   top of the built-in ones, so they can also fix a few texts of an existing
   language. `APEX5_LANG=de` picks a language for one run.

3. Check it and open a pull request:

   ```sh
   tools/i18n.py check
   cargo test
   ```

## When the code changes

`tools/i18n.py update <code>` adds new texts (empty) and removes ones the code no
longer has, keeping what is translated. `tools/i18n.py check` reports how much of
each language is translated, and fails on texts the code no longer has or on `{}`
places that differ from the English.

In the code, texts go through `t("…")`, `tr!("… {name} …", name)` or, in tables
shown later, `key("…")`; see `src/lang.rs`.
