# Команда /phoecord. Ответ любой подкоманды `/phoecord <имя>` собирается, пока работает этот
# плагин, поэтому тексты подкоманд других плагинов тоже лежат здесь, с id плагина в начале ключа.
plugins-header = Плагины PhoeCord
plugin-off = Плагин `{ $id }` выключен. Включите его командой `/phoecord toggle { $id }`.
unknown-command = Неизвестная команда `{ $name }`.
toggle-unknown = Плагина `{ $id }` нет. Попробуйте `/phoecord plugins`.
toggle-self = `{ $id }` нельзя выключить отсюда; используйте `phoecord disable { $id }`.
toggle-on = `{ $id }` теперь включён.
toggle-off = `{ $id }` теперь выключен.
toggle-reconnect = Вступит в силу после переподключения Discord (Ctrl+R).
toggle-failed = Не удалось переключить `{ $id }`: { $error }
owned-no-reply = Команда `/phoecord { $name }` не сработала. Плагин включён? Включите его: `/phoecord toggle { $id }`.
# fake_deafen
fake_deafen-on = Fake deafen **включён**: остальные видят вас с выключенными микрофоном и звуком.
fake_deafen-off = Fake deafen **выключен**.
fake_deafen-unknown-state = Неизвестное состояние `{ $state }`. Используйте `on` или `off`.
# follow_user
follow_user-following = Слежу за пользователем `{ $id }`.
follow_user-trigger = Захожу в канал отслеживаемого пользователя.
follow_user-unfollowed = Больше ни за кем не слежу.
follow_user-bad-user = `{ $user }` не похоже на id или упоминание пользователя.

# FakeDeafenDetector: `/phoecord flagged`.
fake_deafen_detector-none = Никого не поймали на разговоре при отметке «микрофон выключен» или «звук выключен».
fake_deafen_detector-header = Пойманы на разговоре при отметке «микрофон выключен» или «звук выключен»: { $count }
fake_deafen_detector-line = { $user }: пойман на разговоре { $count } раз при отметке «выключен микрофон/звук» (первый раз: { $first }, последний: { $last }), скорее всего притворяется

# AntiRickroll: `/phoecord rickrolls`.
anti_rickroll-none = Рикроллов в ссылках пока не замечено.
anti_rickroll-header = Сообщения со ссылками на рикролл: { $count }
anti_rickroll-line-known = { $channel } сообщение { $message }: `{ $url }` это известный рикролл.
anti_rickroll-line-custom = { $channel } сообщение { $message }: `{ $url }` подходит под один из ваших фильтров на рикроллы.

# VoiceChatUtilities: `/phoecord vcu`.
voice_chat_utilities-started = Запущено «{ $action }» для { $count } участников <#{ $channel }>.
voice_chat_utilities-stopped = Остановлено заданий: { $count }.
voice_chat_utilities-idle = Сейчас ни одно действие над голосовым каналом не выполняется.
voice_chat_utilities-status-line = { $action } в <#{ $channel }>: { $done } из { $count }.
voice_chat_utilities-bad-action = Неизвестное действие `{ $action }`. Доступно: disconnect, mute, unmute, deafen, undeafen, move, stop, status.
voice_chat_utilities-need-channel = Укажите голосовой канал: id или упоминание.
voice_chat_utilities-unknown-channel = Канал `{ $channel }` неизвестен.
voice_chat_utilities-not-voice = <#{ $channel }> не голосовой и не трибуна.
voice_chat_utilities-need-target = Для move нужен целевой голосовой канал.
voice_chat_utilities-bad-target = `{ $channel }` не другой голосовой канал этого сервера.
voice_chat_utilities-empty = В <#{ $channel }> никого нет.
voice_chat_utilities-nobody = В <#{ $channel }> никого, кроме вас.

# rmcord: команда запущена, но плагин не обработал взаимодействие (показывает плагин команд).
rmcord-no-reply = Команда rmcord не сработала. Плагин включён? Включите: `/phoecord toggle rmcord`.

# SilentDelete: команда запущена, но плагин не обработал взаимодействие (показывает плагин команд).
silent_delete-no-reply = Команда SilentDelete не сработала. Плагин включён? Включите: `/phoecord toggle silent_delete`.
voice_chat_utilities-no-reply = Нечего показать: запустите как `/phoecord vcu`.
# QuestMaster: `/phoecord quests`.
quest_master-starting = QuestMaster запускается; прогресс пишется в журнал PhoeCord.
quest_master-already = QuestMaster уже работает. Состояние: `/phoecord quests status`; перезапуск: `/phoecord quests retry`.
quest_master-stopping = QuestMaster останавливается.
quest_master-not-running = QuestMaster не запущен.
quest_master-retry = QuestMaster сейчас перезапустится; затем выберите задания командой `/phoecord quests start`.
quest_master-claiming = Забираю награду за `{ $id }`; результат будет в журнале.
quest_master-claim-none = У `{ $id }` нет награды, которую можно забрать.
quest_master-claim-missing = Укажите задание: `/phoecord quests claim <id задания>`.
quest_master-unknown-action = Неизвестное действие `{ $action }`. Доступны: list, start, stop, status, claim, retry.
quest_master-list-empty = Задания ещё не загружены; загружаю. Повторите команду через пару секунд.
quest_master-list-header = Заданий: { $count }
quest_master-list-line = `{ $id }` { $name }: { $type }, { $label }: { $reward }
quest_master-status-header = QuestMaster: { $phase }, цикл №{ $loop }
quest_master-status-empty = Задач пока нет.
quest_master-status-line = `{ $id }` { $name }: { $type }, { $state }, { $cur }/{ $max }
quest_master-phase-idle = не запущен
quest_master-phase-loading = загрузка заданий
quest_master-phase-picker = ждёт `/phoecord quests start`
quest_master-phase-running = работает
quest_master-phase-finished = цикл завершён
quest_master-st-queue = в очереди
quest_master-st-pending = ожидает
quest_master-st-running = выполняется
quest_master-st-completed = выполнено
quest_master-st-claimed = награда получена
quest_master-st-failed = прервано
quest_master-st-enroll = нужно действие: примите задание в Discord
quest_master-st-claimable = выполнено, заберите награду: `/phoecord quests claim { $id }`
quest_master-st-claim-waiting = получаю награду...
quest_master-st-claim-failed = нужно действие: проверьте капчу в Discord
quest_master-consent-none = Нет авторизации, ожидающей ответа.
quest_master-consent-other = Ожидающая авторизация относится к приложению `{ $id }`.
quest_master-consent-yes = Авторизую приложение `{ $id }` для задания с достижением.
quest_master-consent-no = Приложение `{ $id }` не авторизуется.
