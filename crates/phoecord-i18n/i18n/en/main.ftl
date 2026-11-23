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
