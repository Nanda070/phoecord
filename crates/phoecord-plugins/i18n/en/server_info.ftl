# ServerInfo: answer of `/phoecord serverinfo` (field names are Vencord's).
server_info-owner = Server Owner: { $value }
server_info-created = Created At: { $value }
server_info-joined = Joined At: { $value }
server_info-vanity = Vanity Link: { $value }
server_info-locale = Preferred Locale: { $value }
server_info-verification = Verification Level: { $value }
server_info-boosts = Server Boosts: { $count } (Level { $tier })
server_info-members = Members: { $count } ({ $online } online)
server_info-channels = Channels: { $value }
server_info-roles = Roles: { $value }
server_info-bad-guild = Run it in a server, or give a server id: `guild:123…`.
server_info-failed = Discord refused: { $error }
server_info-relations-asked = Friends, Blocked and Ignored users in this server follow in a moment.
server_info-relations-asked-cut = Friends, Blocked and Ignored users in this server follow in a moment (the first { $max } only).
server_info-friends = Friends: { $value }
server_info-blocked = Blocked Users: { $value }
server_info-ignored = Ignored Users: { $value }
