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
