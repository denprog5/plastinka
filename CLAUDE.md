# CLAUDE.md

**Пластинка** (Plastinka) — «сейчас играет» для стрима на Windows (Rust, один exe, трей). Берёт текущий трек Spotify из системных медиа-контролов Windows (GSMTC) и отдаёт страницу-карточку на `127.0.0.1:7720` для источника «Браузер» в OBS. Родственные проекты с теми же приёмами: `../shit-counter` (Солонка), `../stream-source` (Мурлыка). Пользовательская документация — `README.md`.

Главное требование — **не влиять на FPS** игры и OBS: никакого опроса и таймеров в программе, страница статична между событиями. Минимализм важнее фич.

## Команды

```bash
cargo build --release
cargo test
cargo clippy --all-targets -- -D warnings
python tools/prep.py      # source/*.png → assets/web/*.webp, assets/*.ico, wizard-small*.png; потом пересобрать
powershell -ExecutionPolicy Bypass -File scripts/build-installer.ps1   # release + Inno Setup → dist/Plastinka-Setup-<в>.exe
```

Установщик — `installer/Plastinka.iss` (Inno Setup 6, в `%LOCALAPPDATA%\Programs\Plastinka` без админа). Один файл и для установки, и для автообновления. Тихое обновление не трогает автозапуск (`Check: not WizardSilent`), с `/RELAUNCH=1` перезапускает программу; перед копированием процесс убивается `taskkill`. `AppId` не менять. `.iss` и `.ps1` — UTF-8 **с BOM** (иначе Inno Setup / PowerShell 5 портят кириллицу). Проверка: `setup.exe /VERYSILENT /SUPPRESSMSGBOXES /DIR=<папка> /TASKS=`, повторно с `/RELAUNCH=1`, затем `unins000.exe /VERYSILENT`.

`.cargo/config.toml` включает `+crt-static` — exe не зависит от VC++ Redistributable.

## Архитектура

- **Главный поток** (`main.rs`) — голый Win32-цикл `GetMessageW` для `tray-icon`. `App::update()` идемпотентно обновляет меню, иконку (играет / серая) и подсказку. Будится через `win::notify_main(EV_REFRESH)` (флаг в атомике + `PostThreadMessageW`, флаг не теряется, пока открыто меню).
- **Поток плеера** (`media.rs`) — WinRT `GlobalSystemMediaTransportControlsSessionManager` (крейт `windows`, async-операции через блокирующий `.join()`). Спит в `park()`; обработчики событий Windows (`SessionsChanged`, `CurrentSessionChanged`, `MediaPropertiesChanged`, `PlaybackInfoChanged`, `TimelinePropertiesChanged`) только будят его. После пробуждения ждёт 200 мс, чтобы собрать пачку событий. Свойства и обложка перечитываются только по `MediaPropertiesChanged`; обложка — байты из `Thumbnail`, версия растёт при смене хеша. Позиция — `Position` на момент `LastUpdatedTime`, плюс прошедшее время, если играет. Новое состояние шлётся, только если поменялось что-то кроме позиции или позиция разошлась с ожидаемой больше чем на 1,5 с (перемотка) — браузеры шлют `TimelinePropertiesChanged` часто.
- Выбор плеера: по умолчанию сессия, чей AUMID содержит `spotify` (`Spotify.exe` или `SpotifyAB.SpotifyMusic_…!Spotify` из Store). С `any_player` — текущая сессия Windows, а если она на паузе — та, что играет.
- **Сервер** (`server.rs`) — свой минимальный HTTP на `127.0.0.1`, поток на соединение. `/` и картинки встроены через `include_bytes!`, `/cover?v=N` — текущая обложка, `/events` — SSE (`data: {json}` на каждое изменение; браузер сам переподключается). Отвалившиеся SSE-клиенты убираются при следующей рассылке (`Hub::publish`).
- **Поток обновлений** (`update.rs`, перенесён из Солонки) — WinHTTP без TLS-крейтов: `releases/latest` репозитория `denprog5/plastinka` через 15 с после старта и раз в 6 ч. Найденное при запуске ставится само (если `auto_update` и рядом с exe есть `unins000.exe`), найденное позже — пункт меню. SHA-256 сверяется с `digest` из API GitHub. Ассет релиза должен называться `Plastinka-Setup-*.exe`. Портативный exe не обновляется — открывается страница релиза.
- **Страница** (`assets/web/player.html`) — карточка 560×128 в окне 640×200. Прогресс считает сама от `pos` + `performance.now()`, обновляет раз в секунду только пока видна и играет. Скрытие на паузе — с задержкой 2 с (при переключении трека плеер на миг «не играет»). Смена трека: новая обложка сначала декодируется, потом короткая глитч-анимация. Анимации только transform/opacity/clip-path и только на переходах. Не помещающиеся название/исполнитель (после уменьшения шрифта до 20 px) — бегущая строка через Web Animations (`transform`, пауза 2,5 с → ~40 px/с → пауза 2 с → обратно); запускается при показе и смене трека, отменяется при скрытии. Параметры адреса: `progress=0`, `always=1`, `demo=1`.

## Картинки

Сгенерированы Codex (скилл `codex-image`, референсы канала в `source/ref-*.png`): `source/panel-bg.png` (фон), `source/glitch.png` (красная полоса на чёрном — `prep.py` переводит яркость в альфу), `source/icon-src.png` (пластинка; серую иконку трея и заглушку обложки делает `prep.py`). Шрифт — системный Segoe UI Variable Light, без загрузки.

## Релиз

1. Поднять версию в `Cargo.toml` и `assets/app.rc` (`FILEVERSION`, `PRODUCTVERSION`, строки версии), `cargo test`, `cargo clippy --all-targets -- -D warnings`.
2. `scripts/build-installer.ps1` → `dist/Plastinka-Setup-X.Y.Z.exe`.
3. `git tag vX.Y.Z && git push --tags`, `gh release create vX.Y.Z dist/Plastinka-Setup-X.Y.Z.exe`.

## Проверка без Spotify

Страница с `navigator.mediaSession` и беззвучным `<audio>` в браузере регистрируется в GSMTC так же, как Spotify; с включённым `any_player` программа её видит. Для страницы без программы — `?demo=1`.
