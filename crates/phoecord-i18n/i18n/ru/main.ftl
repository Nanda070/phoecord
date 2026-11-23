# Общие строки PhoeCord (русский).
# Глоссарий: plugin — плагин, proxy — прокси, certificate authority — центр сертификации,
# trust store — хранилище доверенных сертификатов.

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
