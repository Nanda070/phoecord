# PhoeCord shared strings (English, the source language).
# Every message here must exist in ../ru/main.ftl with the same { $variables }.

plugin-enabled = Plugin { $name } enabled.
plugin-disabled = Plugin { $name } disabled.
plugin-unknown = No plugin named { $name }.
plugin-failed = Plugin { $name } failed and was disabled: { $reason }

proxy-listening = Proxy listening on { $addr }.
proxy-stopped = Proxy stopped.
proxy-port-busy = Port { $port } is already in use.

ca-created = Certificate authority created at { $path }.
ca-exists = Certificate authority already exists at { $path }.
ca-trusted = Certificate authority added to the trust store.
ca-trust-failed = Could not add the certificate authority to the trust store: { $reason }

error-generic = Something went wrong: { $reason }
error-config-invalid = The configuration file { $path } is invalid: { $reason }
error-permission-denied = Permission denied. Try running as administrator.

lang-changed = Language set to { $language }.

# CLI: setup, uninstall, paths
cli-setup-done = Done. Quit Discord fully, then start it with `phoecord launch`.
cli-uninstall-done = PhoeCord is removed. Restart Discord without `phoecord launch`.
cli-uninstall-incomplete = some steps could not be undone; they stay in install.log, run `phoecord uninstall` again
cli-setup-steps = { $count } new install step(s) recorded in install.log
cli-uninstall-removed = removed  { $what }
cli-uninstall-kept = kept     { $what }
cli-uninstall-failed = failed   { $what }: { $why }
cli-paths-config = config
cli-paths-ca = CA
cli-paths-plugins = plugins
cli-paths-language = language
lang-source-flag = --lang flag
lang-source-config = config.toml
lang-source-os = OS language
lang-source-default = default
