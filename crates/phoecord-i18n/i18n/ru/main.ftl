# Общие строки PhoeCord (русский).
# Глоссарий (используем везде одинаково):
#   plugin — плагин; proxy — прокси; config — конфиг (в справке и CLI), «файл конфигурации» в длинных фразах;
#   CA — сертификат CA (локальный); certificate authority — центр сертификации;
#   trust store — хранилище доверенных сертификатов; gateway — шлюз; selftest — самопроверка;
#   enable / disable — включить / выключить; launch — запустить; autostart — автозапуск.
#   install.log, config.toml, имена команд, флагов и id плагинов не переводятся.

plugin-enabled = Плагин { $name } включён.
plugin-disabled = Плагин { $name } отключён.
plugin-unknown = Плагин «{ $name }» не найден.
plugin-failed = Плагин { $name } завершился с ошибкой и был отключён: { $reason }

proxy-listening = Прокси слушает на { $addr }.
proxy-stopped = Прокси остановлен.
proxy-port-busy = Порт { $port } уже занят.

ca-created = Центр сертификации создан: { $path }.
ca-exists = Центр сертификации уже существует: { $path }.
ca-trusted = Центр сертификации добавлен в хранилище доверенных сертификатов.
ca-trust-failed = Не удалось добавить центр сертификации в хранилище доверенных сертификатов: { $reason }

error-generic = Что-то пошло не так: { $reason }
error-config-invalid = Файл конфигурации { $path } недействителен: { $reason }
error-permission-denied = Доступ запрещён. Попробуйте запустить от имени администратора.

lang-changed = Язык изменён на { $language }.

# CLI: setup, uninstall, paths
cli-intro =
    PhoeCord - локальный мод для Discord: он запускает на этом компьютере небольшой прокси и меняет то, что показывает Discord.
    Большинству нужно приложение-установщик (PhoeCord.app на Mac, установщик PhoeCord на Windows), а не эта программа.

    Команды для командной строки:
      phoecord setup     настроить PhoeCord (сертификат, автозапуск)
      phoecord launch    запустить Discord через PhoeCord
      phoecord run       запустить только прокси в этом окне
      phoecord --help    все команды

cli-setup-done = Готово. Полностью закройте Discord и запустите его командой `phoecord launch`.
cli-uninstall-done = PhoeCord удалён. Перезапустите Discord без `phoecord launch`.
cli-uninstall-incomplete = не все шаги удалось отменить; они остались в install.log, запустите `phoecord uninstall` ещё раз
cli-setup-steps = Новых шагов установки записано в install.log: { $count }
cli-uninstall-removed = удалено   { $what }
cli-uninstall-kept = оставлено { $what }
cli-uninstall-failed = ошибка    { $what }: { $why }
cli-paths-config = конфиг
cli-paths-ca = CA
cli-paths-plugins = плагины
cli-paths-language = язык
lang-source-flag = флаг --lang
lang-source-config = config.toml
lang-source-os = язык системы
lang-source-default = по умолчанию

# CLI: --help и подкоманды
cmd-about = Плагины в стиле Vencord для Discord через локальный прокси
cmd-run = Запустить прокси (по умолчанию). Направьте Discord на него: `--proxy-server=http://127.0.0.1:18463`
cmd-launch = Запустить прокси и Discord через него (сначала закройте Discord). Можно указать команду клиента
cmd-setup = Первая настройка: создать конфиг и локальный сертификат CA, добавить его в доверенные и включить запуск прокси при входе
cmd-uninstall = Отменить всё, что сделал `setup`: автозапуск, доверие к CA, созданные файлы (ваш конфиг останется)
cmd-trust = Добавить локальный сертификат CA в хранилища, которые использует Discord
cmd-untrust = Убрать локальный сертификат CA из этих хранилищ
cmd-plugins = Показать плагины и их состояние
cmd-enable = Включить плагин
cmd-disable = Выключить плагин
cmd-toggle = Переключить плагин (например, `phoecord toggle game_activity_toggle`)
cmd-log = Показать последние удалённые и изменённые сообщения, сохранённые плагином message_logger
cmd-import-vencord = Включить плагины PhoeCord, которые совпадают с включёнными плагинами Vencord / Vesktop / Equicord
cmd-selftest = Проверить, что всё работает с настоящим Discord (вход в аккаунт не нужен)
cmd-paths = Показать, где лежат конфиг, сертификат CA и данные плагинов
cli-arg-import-path = Путь к settings.json Vencord (если не указан, ищется сам)
cli-import-none = настройки Vencord, Vesktop или Equicord не найдены; укажите файл через --path FILE
cli-import-found = Читаю настройки { $name }: { $path }
cli-import-read = не удалось прочитать { $path }
cli-import-summary = Включено: { $enabled }, уже было включено: { $already }, без аналога в PhoeCord: { $unmatched } плагин(ов) Vencord.
cli-help-usage = Использование:
cli-help-commands = Команды
cli-help-options = Параметры
cli-help-arguments = Аргументы
cli-help-help = Показать справку
cli-help-version = Показать версию
cli-arg-lang = Язык интерфейса: en или ru (по умолчанию: `language` в config.toml, затем язык системы)
cli-arg-log-n = Сколько записей показать (по умолчанию 20)

# CLI: run, launch
cli-listening = PhoeCord слушает на { $listen } (плагины: { $plugins })
cli-starting = запускаю { $program }
cli-launcher-exited = лаунчер завершился ({ $status }); прокси продолжает работать, остановить: Ctrl+C
cli-client-exited = Discord завершился ({ $status })
cli-err-listen = не удалось слушать на { $listen }
cli-err-start-discord = не удалось запустить Discord
cli-err-no-discord-linux = Discord не найден в PATH; укажите команду: phoecord launch -- /путь/к/discord
cli-err-no-discord-windows = Discord не найден в %LOCALAPPDATA%; укажите команду: phoecord launch -- C:\путь\к\Discord.exe
cli-err-no-discord-macos = Discord не найден в /Applications; укажите команду: phoecord launch -- /путь/к/Discord
cli-error-prefix = ошибка
cli-warning-prefix = предупреждение

# CLI: plugins, enable, disable, toggle
cli-plugin-on = вкл
cli-plugin-off = выкл
cli-plugins-vencord = (в Vencord: { $name })
cli-no-such-plugin = плагина «{ $name }» нет (список: `phoecord plugins`)
cli-plugin-state-on = { $name }: включён
cli-plugin-state-off = { $name }: выключен
cli-gateway-note = (плагин шлюза: заработает после переподключения Discord; его вызывает Ctrl+R в Discord)
cli-config-line = конфиг: { $path }
cli-ca-line = локальный сертификат CA: { $path } (действует только для { $hosts })
plugin-desc-fake_nitro = Отправляйте любые эмодзи без Nitro (ссылками на картинки)
plugin-desc-game_activity_toggle = Пока включён, в вашем статусе не видно игру, в которую вы играете
plugin-desc-message_logger = Сохраняет удалённые сообщения и показывает историю правок
plugin-desc-no_track = Блокирует аналитику и отчёты о сбоях Discord
plugin-desc-relationship_notifier = Сообщает, когда вас убрали из друзей, с сервера или из группы
plugin-desc-silent_typing = Индикатор набора текста никогда не отправляется
plugin-desc-clear_urls = Убирает параметры отслеживания из ссылок, которые вы отправляете
plugin-desc-unindent = Убирает общий отступ в блоках кода, которые вы отправляете
plugin-desc-friend_invites = Создаёт, показывает и отзывает ссылки-приглашения в друзья командами
plugin-desc-server_info = Показывает сведения о сервере: владелец, возраст, бусты, каналы, роли
plugin-desc-view_raw = Показывает исходный JSON сообщения, канала, сервера, роли или пользователя
plugin-desc-unsuppress_embeds = Снова показывает встраивания сообщения или скрывает их
plugin-desc-commands = Слэш-команда /phoecord: plugins, toggle
plugin-desc-i_love_spam = Не скрывает сообщения «вероятных спамеров»
plugin-desc-no_blocked_messages = Полностью скрывает сообщения заблокированных и игнорируемых пользователей
plugin-desc-no_reply_mention = По умолчанию отключает уведомление автору при ответе
plugin-desc-text_replace = Заменяет текст в отправляемых сообщениях по вашим правилам
plugin-desc-send_timestamps = Превращает `12:34` в сообщениях в метку времени Discord
plugin-desc-anonymise_file_names = Заменяет имена загружаемых файлов на случайные, расширение сохраняется
plugin-desc-silent_message_toggle = Отправляет сообщения без уведомлений
plugin-desc-better_sessions = Точное время ваших сессий и предупреждения о новых
plugin-desc-experiments = Эксперименты Discord и функции для разработчиков
plugin-desc-implicit_relationships = Люди, с которыми вы много общаетесь, но не дружите
plugin-desc-member_count = Число участников в сети, всего и в голосе по серверам
plugin-desc-message_latency = Сообщения, которые долго отправлялись
plugin-desc-message_link_embeds = Предпросмотр сообщений по ссылкам на Discord
plugin-desc-mutual_group_dms = Общие групповые чаты с человеком
plugin-desc-new_guild_settings = Заглушает новые серверы и применяет ваши настройки при входе
plugin-desc-override_forum_defaults = Свой вид и сортировка форумов по умолчанию
plugin-desc-pause_invites_forever = Приостанавливает приглашения на сервер бессрочно
plugin-desc-platform_indicators = Показывает, с компьютера, телефона или из браузера сидит человек
plugin-desc-read_all_notifications_button = Отмечает все уведомления серверов прочитанными разом
plugin-desc-show_hidden_channels = Список каналов, которые вам нельзя открыть
plugin-desc-show_hidden_things = Показывает тайм-ауты, приостановленные приглашения и другое скрытое
plugin-desc-typing_indicator = Показывает, кто печатает в каждом канале
plugin-desc-user_voice_show = Показывает, кто сидит в голосовых каналах
plugin-desc-valid_reply = Исправляет «Не удалось загрузить сообщение» в ответах
plugin-desc-valid_user = Исправляет упоминания неизвестных пользователей (@unknown-user)
plugin-desc-vc_narrator = Уведомляет, когда кто-то заходит в ваш голосовой канал или выходит (без озвучки)
plugin-desc-who_reacted = Кто поставил реакцию на сообщение
plugin-desc-fake_deafen = Выглядеть для других с выключенным звуком, но всё слышать
plugin-desc-fake_deafen_detector = Отмечать участников, которые говорят с выключенным микрофоном или звуком
plugin-desc-follow_user = Ходить за пользователем по голосовым каналам
plugin-desc-anti_rickroll = Находить ссылки на рикролл в сообщениях
plugin-desc-local_dev_badges = Значки сотрудника и разработчика на своём профиле, только у себя
plugin-desc-voice_chat_utilities = Переместить, заглушить или отключить всех в голосовом канале
plugin-desc-rmcord = Массово удалить свои сообщения
plugin-desc-silent_delete = Подменить сообщение и удалить его, чтобы логгеры сохранили подмену
plugin-desc-quest_master = Автоматически выполнять квесты Discord

# CLI: log
cli-log-empty = пока ничего не записано ({ $path })
cli-log-place-channel = в канале { $channel }
cli-log-place-guild = на сервере { $guild }, в канале { $channel }
cli-log-edited = { $time }  ✏ { $author } { $place }: { $was } → { $content }{ $tail }
cli-log-deleted = { $time }  🗑 { $author } { $place }: { $content }{ $tail }

# CLI: отчёт удаления (что убрано)
cli-step-dir = папка { $path }
cli-step-file = файл { $path }
cli-step-ca-trusted = сертификат CA { $path } из системных хранилищ доверенных сертификатов
cli-step-run-key = значение Run в реестре { $name }
cli-step-launch-agent = LaunchAgent { $label }
cli-step-systemd-unit = пользовательская служба systemd { $unit }
cli-kept-already-gone = уже удалено
cli-kept-not-empty = не пуста
cli-kept-holds-log = в ней журнал неудавшихся шагов
cli-kept-line = оставлено { $what } ({ $why })

# CLI: selftest
cli-selftest-rest-ok = ✓ REST: discord.com перехвачен, локальный сертификат CA принят, API доступен
cli-selftest-rest-fail = ✗ REST: { $reason }
cli-selftest-rest-timeout = ✗ REST: время ожидания истекло
cli-selftest-gateway-ok = ✓ шлюз { $combo }: HELLO прошёл туда и обратно (heartbeat { $ms } мс)
cli-selftest-gateway-fail = ✗ шлюз { $combo }: { $reason }
cli-selftest-gateway-timeout = ✗ шлюз { $combo }: время ожидания истекло
cli-selftest-undecodable = ✗ PhoeCord не смог разобрать пакетов шлюза: { $count }
cli-selftest-decoded = прокси разобрал пакетов: { $count }
cli-selftest-failed = не прошло проверок: { $count }
cli-selftest-passed = все проверки пройдены
cli-selftest-err-connect-closed = прокси закрыл соединение CONNECT
cli-selftest-err-proxy-answered = прокси ответил: { $line }
cli-selftest-err-http = HTTP { $status }: { $body }
cli-selftest-err-handshake = рукопожатие
cli-selftest-err-closed-before-hello = соединение закрыто до HELLO
cli-selftest-err-expected-hello = ожидался HELLO, получен op { $op }

# Условия использования (TERMS.ru.md)
terms-prompt = Введите `да` (или `yes`), чтобы принять условия и установить; всё остальное отменяет:
terms-declined = Условия не приняты; ничего не установлено.
cli-arg-accept-terms = Не спрашивать про условия: вы прочитали TERMS.ru.md и принимаете их

# Исправления 1.4.8
cli-uninstall-stopped = остановлен прокси PhoeCord (процесс { $pid })
cli-uninstall-stop-failed = не удалось остановить работающий прокси PhoeCord: { $why }
cli-arg-channel = Какой Discord запустить: stable, ptb или canary (нужно, если установлено несколько)
cli-err-channel-not-installed = Discord { $channel } не установлен
cli-err-several-clients = установлено несколько клиентов Discord, поэтому ни один не выбран за вас. Укажите --channel:
    { $list }
cli-proxy-reused = Прокси PhoeCord (версия { $version }) уже работает на { $listen }; запускаю Discord через него.
cli-err-port-taken = Адрес { $listen } занят другой программой, не PhoeCord. Закройте её или измените `listen` в config.toml.
cli-arg-setup-channel = Добавить ярлык в меню «Пуск», который запускает этот Discord через PhoeCord: stable, ptb или canary (по умолчанию: единственный установленный)
cli-shortcut-created = ярлык «{ $name }» добавлен: { $path }
cli-shortcut-unsupported = В этой системе ярлык не создаётся; запускайте Discord командой `phoecord launch --channel { $channel }`.
cli-shortcut-skipped = Ярлык не добавлен: { $why }
cli-warn-ca-untrusted = Внимание: локальный сертификат CA ({ $path }) не добавлен в хранилище доверенных, поэтому Discord не подключится через PhoeCord и может бесконечно перезапускаться. Выполните `phoecord trust` (или `phoecord setup`), чтобы это исправить.

# Окно установщика (1.4.3)
gui-tab-install = Установка
gui-tab-plugins = Плагины
gui-tab-settings = Настройки
gui-tab-about = О программе
gui-dry-run = Пробный запуск
gui-proxy-running = PhoeCord работает
gui-proxy-stopped = PhoeCord не запущен
gui-proxy-port-taken = Порт занят
gui-proxy-tip = Локальный прокси на { $listen }. Discord подключается через него, когда запущен с PhoeCord.
gui-client-stable = Discord
gui-client-ptb = Discord PTB
gui-client-canary = Discord Canary
gui-client-phoenix = Discord Phoenix
gui-version = Версия { $version }
gui-version-unknown = Версия неизвестна
gui-state-on = PhoeCord включён
gui-state-off = Не установлен
gui-state-repair = Нужно починить
gui-install-title = Какой Discord у вас?
gui-install-title-on = PhoeCord установлен
gui-install-sub = PhoeCord работает рядом с Discord и запускает его через локальный прокси. Всё можно отменить одной кнопкой.
gui-install-sub-on = Выберите Discord, чтобы открыть его с PhoeCord, или добавьте PhoeCord в другой.
gui-install-action = Установить PhoeCord
gui-open-action = Открыть { $name }
gui-add-action = Добавить в { $name }
gui-install-hint = Займёт несколько секунд. Система может попросить подтвердить сертификат.
gui-open-hint = Ярлык «(PhoeCord)» в меню открывает его так же.
gui-repair = Починить
gui-uninstall = Удалить
gui-repair-banner-title = PhoeCord нужно починить
gui-repair-banner = Система больше не доверяет сертификату PhoeCord, поэтому Discord не может подключиться через него. «Починить» это исправит.
gui-missing-binary-title = Не хватает файла PhoeCord
gui-missing-binary = Основная программа не найдена: { $path }. Скачайте релиз целиком ещё раз и держите все его файлы в одной папке.
gui-no-clients-title = Discord не найден
gui-no-clients = Если Discord стоит в другом месте, выберите его папку. Если нет, скачайте его, откройте один раз и проверьте снова.
gui-no-clients-looked = Где искали:
gui-choose-folder = Выбрать папку…
gui-choose-folder-title = Выберите папку, где установлен Discord
gui-download-discord = Скачать Discord
gui-elsewhere = Discord стоит в другом месте?
gui-folder-no-discord = В { $path } нет Discord. Выберите папку, в которой лежит сам Discord.
gui-folder-added = Добавлен { $name }. PhoeCord теперь будет искать и в этой папке.
gui-folder-already-listed = { $name } из этой папки уже есть в списке. Добавлять её ещё раз не нужно.
gui-folder-save-failed = Не удалось запомнить папку: { $error }. Проверьте, что папка настроек доступна для записи, и попробуйте снова.
gui-check-again = Проверить снова
gui-install-anyway = Установить без Discord
gui-fact-adds-title = Что добавляется
gui-fact-adds = Сертификат, который работает только для сайтов Discord, локальный прокси, который запускается при входе в систему, и ярлык в меню.
gui-fact-never-title = Чего никогда не будет
gui-fact-never = Ваши данные никуда не отправляются, файлы Discord не меняются.
gui-progress-title = Установка в { $name }
gui-progress-title-plain = Установка PhoeCord
gui-progress-repair-title = Починка PhoeCord
gui-progress-failed-title = Установка остановилась
gui-progress-sub = Компьютером можно пользоваться. Это займёт несколько секунд.
gui-progress-failed-sub = Завершённые шаги записаны, поэтому «Удалить» всё равно их уберёт.
gui-phase-cert = Сертификат для Discord
gui-phase-cert-sub = Создаётся на этом компьютере и добавляется в доверенные. Система может попросить подтверждение.
gui-phase-autostart = Запуск при входе в систему
gui-phase-autostart-sub = Прокси запускается вместе с компьютером.
gui-phase-shortcut = Ярлык в меню
gui-phase-shortcut-sub = Открывает { $name } через PhoeCord.
gui-phase-shortcut-mac = На macOS не нужен: открывайте Discord из этого окна.
gui-phase-shortcut-none = Пропущено: Discord не выбран.
gui-err-cert-title = Сертификат не добавлен в доверенные
gui-err-cert = Система отклонила или отменила сертификат. Попробуйте ещё раз и подтвердите системный запрос. Если запроса нет, перезагрузите компьютер и попробуйте снова.
gui-err-autostart-title = Не удалось включить запуск при входе
gui-err-autostart = PhoeCord всё равно работает, если открывать Discord из этого окна. Попробуйте ещё раз; если не выйдет, скопируйте подробности и сообщите о проблеме.
gui-err-shortcut-title = Не удалось создать ярлык
gui-err-shortcut = Всё остальное установлено. Открывайте Discord из этого окна или попробуйте ещё раз.
gui-err-generic-title = Что-то пошло не так
gui-err-generic = Попробуйте ещё раз. Если не выйдет, скопируйте подробности и сообщите о проблеме на GitHub.
gui-try-again = Попробовать снова
gui-copy-details = Скопировать подробности
gui-back = Назад
gui-done = Готово
gui-ready-title = PhoeCord готов
gui-ready-title-repair = Починено
gui-ready-sub = Открывайте { $name } отсюда или ярлыком «(PhoeCord)» в меню.
gui-ready-sub-plain = Когда Discord будет установлен, открывайте его из этого окна.
gui-removing-title = Удаление PhoeCord
gui-removing-sub = Останавливаем прокси и отменяем каждый сделанный шаг.
gui-removed-title = PhoeCord удалён
gui-removed-partly-title = PhoeCord удалён не полностью
gui-removed-sub = Удалено пунктов: { $count }. Плагины и файл настроек остаются; удалите папку данных вручную, если они не нужны.
gui-removed-failed-title = Некоторые шаги не удалось отменить
gui-removed-failed = Они остаются записанными, и «Удалить» попробует снова. Закройте Discord и повторите.
gui-uninstall-title = Удалить PhoeCord?
gui-uninstall-body = Будут удалены сертификат, запуск прокси при входе и ярлыки для всех Discord. Сам Discord, ваши плагины и настройки останутся.
gui-uninstall-app-windows = Само это приложение останется. Чтобы удалить и его, откройте Параметры → Приложения.
gui-uninstall-app-macos = Само это приложение останется. Чтобы удалить и его, перенесите PhoeCord из «Программ» в Корзину.
gui-cancel = Отмена
gui-close = Закрыть
gui-save = Сохранить
gui-plugins-title = Плагины
gui-plugins-sub = Включайте и выключайте плагины или добавьте новый из файла или по ссылке. Изменения применяются без перезапуска Discord.
gui-plugins-from-file = Установить из файла
gui-plugins-from-link = По ссылке
gui-plugins-pick-title = Выберите плагин (.zip или plugin.toml)
gui-plugins-pick-filter = Плагин PhoeCord
gui-plugins-drop-hint = Или перетащите .zip или папку плагина в это окно.
gui-plugins-search = Поиск плагинов
gui-plugins-yours = Добавленные вами
gui-plugins-builtin = Встроенные
gui-plugins-none = Вы ещё не добавили плагинов. Добавьте .zip или папку с plugin.toml и plugin.wasm.
gui-plugins-no-match = Ничего не найдено.
gui-plugins-meta = { $version }, автор: { $authors }
gui-plugins-meta-version = Версия { $version }
gui-plugins-broken = Сломан
gui-plugins-remove-label = Удалить { $name }
gui-drop-title = Отпустите, чтобы установить плагин
gui-drop-body = .zip или папка с plugin.toml и plugin.wasm.
gui-confirm-plugin-title = Установить { $name }?
gui-confirm-plugin-replaces = Заменит установленную версию { $version }.
gui-confirm-plugin-trust = Ставьте плагины только от тех, кому доверяете. PhoeCord разрешает плагину только то, что указано здесь.
gui-confirm-install = Установить
gui-asks-sees = Что увидит
gui-asks-sees-nothing = Никаких событий Discord
gui-asks-can = Что сможет
gui-asks-can-nothing = Ничего, кроме чтения событий выше
gui-asks-connects = Куда сможет подключаться
gui-event-rest-request = Запросы, которые отправляет ваш Discord
gui-event-rest-response = Ответы от Discord
gui-event-dispatch = События от Discord в реальном времени
gui-event-send = События, которые отправляет ваш Discord
gui-event-all = все
gui-right-kv = Запоминать свои настройки и данные
gui-right-notify = Показывать уведомления
gui-right-emit-send = Отправлять события от вашего имени, например менять статус
gui-right-emit-dispatch = Показывать в вашем Discord выдуманные события, только на этом компьютере
gui-right-guilds = Видеть эмодзи серверов и к какому серверу относится канал
gui-right-fetch = Подключаться к сайтам из списка ниже
gui-right-rest = Пользоваться API Discord от вашего имени: отправлять, менять и удалять
gui-right-plugins = Видеть и переключать другие ваши плагины
gui-right-data = Сохранять файлы в своей папке
gui-link-title = Установка по ссылке
gui-link-sub = Вставьте ссылку https:// на .zip плагина. PhoeCord покажет, что он просит, до установки.
gui-link-label = Ссылка
gui-link-download = Скачать
gui-remove-title = Удалить { $name }?
gui-remove-body = Плагин и сохранённые им данные будут удалены. Это нельзя отменить.
gui-remove = Удалить
gui-pkg-err-not-package = Это не плагин PhoeCord. Выберите .zip или папку с plugin.toml и plugin.wasm.
gui-pkg-err-no-wasm = В пакете нет plugin.wasm. Попросите у автора полный пакет.
gui-pkg-err-too-big = Плагин больше { $mb } МБ. Проверьте, тот ли файл вы выбрали.
gui-pkg-err-unsafe = Архив пытается записать файл за пределами своей папки ({ $path }), установка отклонена. Не ставьте его.
gui-pkg-err-manifest = Ошибка в plugin.toml: { $why }. Попросите у автора исправленную версию.
gui-pkg-err-broken = Код плагина не загружается: { $why }. Попросите у автора версию для этого PhoeCord.
gui-pkg-err-https = Разрешены только ссылки https://.
gui-pkg-err-download = Не удалось скачать: { $why }. Проверьте ссылку и интернет, затем попробуйте снова.
gui-pkg-err-io = Не удалось записать файл: { $why }. Проверьте, что папка не открыта в другой программе, и попробуйте снова.
gui-toast-starting = Запускаем Discord через PhoeCord
gui-start-failed-title = Discord не запустился
gui-start-failed-body = PhoeCord остановился при запуске. Что он сообщил:
gui-start-failed-silent = PhoeCord остановился при запуске и не сообщил причину.
gui-start-failed-log = Полный вывод: { $path }
gui-open-log = Открыть лог
gui-open-failed = Не удалось открыть: { $why }
gui-toast-plugin-installed = { $name } установлен и включён
gui-toast-plugin-removed = { $name } удалён
gui-toast-gateway = Плагин заработает после переподключения Discord. Если нет, перезапустите Discord.
gui-toast-copied = Подробности скопированы
gui-toast-wait = Дождитесь, пока PhoeCord закончит, и потом закройте окно.
gui-settings-title = Настройки
gui-settings-lang = Язык
gui-settings-lang-hint = Для этого окна и сообщений PhoeCord.
gui-settings-lang-system = Системный
gui-settings-motion = Анимации
gui-settings-motion-hint = Выключить анимации в этом окне.
gui-settings-motion-switch = Меньше движения
gui-settings-upstream = Подключение через прокси
gui-settings-upstream-hint = Для VPN и zapret: PhoeCord пускает свои соединения через этот прокси. Оставьте пустым для прямого подключения.
gui-settings-upstream-label = Адрес прокси
gui-settings-upstream-saved = Сохранено. Новые соединения пойдут через него.
gui-settings-upstream-bad = Формат: socks5://хост:порт или http://хост:порт.
gui-settings-files = Файлы
gui-settings-files-hint = Где PhoeCord хранит сертификат, плагины и настройки.
gui-settings-open-data = Открыть папку данных
gui-settings-open-config = Открыть папку настроек
gui-about-version = Версия { $version }
gui-about-body = PhoeCord: загрузчик модов для Discord с открытым кодом, написанный на Rust. Он меняет поведение Discord через локальный прокси и плагины, не трогая файлы Discord.
gui-about-source = Исходный код
gui-about-report = Сообщить о проблеме
gui-about-terms = Условия использования
gui-terms-title = Перед установкой
gui-terms-title-view = Условия использования
gui-terms-sub = Версия { $version }. Прочитайте до конца и примите, чтобы продолжить.
gui-terms-accept = Принимаю
gui-terms-quit = Выйти
gui-terms-scroll = Прокрутите до конца, чтобы принять
gui-open-hint-mac = Открывайте его из этого окна: на macOS нет ярлыка PhoeCord.
gui-ready-sub-mac = Открывайте { $name } из этого окна, когда нужен PhoeCord.

## Installer window: first start
gui-onboard-step = Шаг { $n } из { $total }
gui-onboard-back = Назад
gui-onboard-next = Дальше
gui-onboard-skip = Пропустить
gui-onboard-later = Не сейчас
gui-onboard-start = Начать
gui-onboard-welcome-title = Добро пожаловать в PhoeCord
gui-onboard-welcome-sub = Загрузчик модов для Discord. Три вещи, которые стоит знать:
gui-onboard-point-proxy-title = Работает рядом с Discord
gui-onboard-point-proxy = Маленький локальный прокси меняет то, что Discord отправляет и получает. Файлы самого Discord не трогаются.
gui-onboard-point-plugins-title = Плагины выбираете вы
gui-onboard-point-plugins = Добавляйте плагины из файла или по ссылке. До установки вы видите, что плагин сможет делать.
gui-onboard-point-undo-title = Всё убирается одной кнопкой
gui-onboard-point-undo = Удаление убирает сертификат, прокси и ярлык и ничего после себя не оставляет.
gui-onboard-pick-sub = PhoeCord запускает этот Discord через свой прокси. Другой можно добавить потом.
gui-onboard-looking = Ищем Discord…
gui-onboard-done-title = Всё готово
gui-onboard-done-what = Что произойдёт
gui-onboard-done-sub = PhoeCord будет настроен для { $name }. Это займёт несколько секунд, и всё отменяется одной кнопкой.

## Installer window: credits
gui-credits = Благодарности

# Установка и переустановка загрузчика (1.5.6)
cli-step-loader-wanted = выбор загрузчика для Discord { $channel }
cli-step-loader-copy = DLL загрузчика { $path }
cli-kept-not-ours = изменён другой программой, оставлен на месте
cli-arg-setup-loader = Также установить нативный загрузчик (version.dll) в выбранный Discord; только Windows. Каждый `phoecord launch` и автозапуск возвращают его на место после обновления Discord
cli-loader-installed = загрузчик установлен для Discord { $channel }: { $path }
cli-loader-up-to-date = загрузчик для Discord { $channel } уже на месте: { $path }
cli-loader-foreign = загрузчик для Discord { $channel } не установлен: { $path } принадлежит другой программе, PhoeCord его не трогает. Удалите или переименуйте его сами, если хотите поставить туда загрузчик PhoeCord.
cli-loader-source-missing = загрузчик для Discord { $channel } не установлен: нет файла { $path } (он поставляется рядом с программой phoecord)
cli-loader-no-build = загрузчик для Discord { $channel } не установлен: в { $path } нет сборки app-x.y.z
cli-loader-failed = не удалось поставить загрузчик для Discord { $channel }: { $why }
cli-loader-unsupported = Загрузчик работает только в Windows; ничего не установлено.
cli-loader-skipped = Загрузчик не установлен: { $why }
cli-loader-check-failed = не удалось проверить загрузчик: { $why }
