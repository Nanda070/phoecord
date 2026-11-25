# Plugins

Checked against Vencord's plugin list: 23.11.2026 (vencord.dev/plugins, 173 entries).

**Goal: every Vencord plugin that makes sense on a native client, plus the owner's own userplugins.** This page maps each one to where it lives in PhoeCord. Names match Vencord's so settings import 1:1 (`phoecord import-vencord`); our ids are the `snake_case` form (`MessageLogger` → `message_logger`). Plugins are rebuilt on the PhoeCord Plugin API; Vencord code may be adapted with attribution, since both projects are GPL-3.0-or-later. How to port one: [`PORTING_PLUGINS.md`](PORTING_PLUGINS.md).

Visual plugins and panels are in scope, not only network ones: every 🎨 plugin and every Vencord panel gets a Phoenix implementation (Phase 2 and 1.x). ⛔ is only for what does not exist on a native client (DevTools, CSS fixes, web-only features), and ♻️ for what PhoeCord replaces with its own version.

**On by default:** `no_track` only, as in Vencord. Everything else is off until the user turns it on.

**Language:** a plugin's notifications and `/phoecord` answers speak the user's language (English or Russian) through `Host::t` and the plugin's own `.ftl` bundle; how a plugin adds strings is in [`PORTING_PLUGINS.md`](PORTING_PLUGINS.md) §3.3. Translated so far: the `/phoecord` command itself (`plugins`, `toggle`, the "plugin is off" answer), RelationshipNotifier, BetterSessions (alert and `/phoecord sessions`) and VcNarrator (notification title; the message templates are the user's). The other plugins' `/phoecord` answers are still English.

| Layer | What it means | When | Count |
|:---|:---|:---|---:|
| 🌐 Network | Done fully in the proxy; works on today's Discord and on Phoenix | Phase 1 (stage 1.2) | 40 |
| 🌐 + 🎨 | Data part in the proxy now (or a `/phoecord` command), on-screen part with UI hooks | Phase 1 + Phase 2 | 29 |
| 🎨 UI | Needs hooks into the client's own code | Phase 2 (from stage 2.3) | 88 |
| 🖌️ Theme | Done by the theme engine (palette, spacing) | Phase 2 (stage 2.3) | 3 |
| ♻️ Replaced | PhoeCord provides it natively in another form | as noted | 6 |
| ⛔ N/A | Web-only, DevTools-only, Vencord internals or out of scope | never | 17 |

Total: 169 Vencord plugins, 4 Vencord core modules, 10 userplugins. Phase 1 ships **69** of them, fully or in their network part.

Release targets: **v1.0** = every 🌐 and 🌐 + 🎨 plugin, the theme engine, and the 30 most-used 🎨 plugins (ROADMAP 2.4). **v1.x** = the rest of 🎨 until parity.

## Vencord plugins

| Plugin | What it does | Layer | Where |
|:---|:---|:---|:---|
| AccountPanelServerProfile | Right-click the account panel to open your server profile | 🎨 UI | 2 |
| AddAttachments | Add attachments while editing a message | 🎨 UI | 2 |
| AlwaysAnimate | Animate everything that can be animated | 🎨 UI | 2 |
| AlwaysExpandRoles | Expand the role list in profile popouts | 🎨 UI | 2 |
| AlwaysTrust | Skip untrusted-domain and suspicious-file warnings | 🎨 UI | 2 |
| AnonymiseFileNames | Anonymise uploaded file names | 🌐 Network | done (cloud uploads; the per-file toggle button is Phase 2) |
| AutoDNDWhilePlaying | Set your status automatically while a game runs | 🌐 Network | 1.2.13 |
| BetterFolders | Server folders in their own sidebar | 🎨 UI | 2 |
| BetterGifAltText | GIF alt text from tags or file name | 🎨 UI | 2 (Vencord only changes how the client renders the image; nothing is sent) |
| BetterGifPicker | Open the GIF picker on favourites | 🎨 UI | 2 |
| BetterRoleContext | Copy colour, edit role, view icon in role menus | 🎨 UI | 2 |
| BetterRoleDot | Copy role colour on click; dot and colour together | 🎨 UI | 2 |
| BetterSearch | Extra message search options | 🎨 UI | 2 |
| BetterSessions | Exact timestamps and names for sessions, new-session alerts | 🌐 + 🎨 | done: data — new-session alerts, `/phoecord sessions` / 2 (names, badge, icons) |
| BetterSettings | Faster settings menu | 🎨 UI | 2 |
| BetterUploadButton | Single-click upload, right-click menu | 🎨 UI | 2 |
| BetterYouTubeEmbeds | No ads and no UMG blocks in YouTube embeds | 🎨 UI | 2 |
| BiggerStreamPreview | Bigger stream previews | 🎨 UI | 2 |
| BlurNSFW | Blur NSFW attachments until hovered | 🎨 UI | 2 |
| CallTimer | Timer in voice calls | 🎨 UI | 2 |
| CharacterCounter | Character counter in the chat input | 🎨 UI | 2 |
| ClearURLs | Strip tracking parameters from links you send | 🌐 Network | done |
| ClientTheme | Custom client colour | 🖌️ Theme | 2.3 |
| ColorSighted | Remove colourblind status icons | 🎨 UI | 2 |
| ConsoleJanitor | Silence console noise | ⛔ N/A | web console only |
| ConsoleShortcuts | Console aliases | ⛔ N/A | web console only |
| CopyEmojiMarkdown | Copy emoji as markdown | 🎨 UI | 2 |
| CopyFileContents | Copy text attachment contents | 🎨 UI | 2 |
| CopyUserURLs | Copy user URL from the context menu | 🎨 UI | 2 |
| CrashHandler | Recover from crashes | ♻️ Replaced | loader degradation (ARCHITECTURE.md) |
| CustomCommands | Your own slash commands and tags | 🌐 Network | 1.2.12 |
| CustomIdle | Change or disable auto-idle | 🌐 Network | 1.2.13 |
| CustomRPC | Fully custom Rich Presence | 🌐 Network | 1.2.13 |
| Dearrow | Less clickbait YouTube embed titles and thumbnails | 🌐 Network | 1.2.15 |
| Decor | Custom avatar decorations | 🌐 + 🎨 | 1.2.14 / 2 |
| DevCompanion | Vencord developer companion | ⛔ N/A | Vencord dev tool |
| DirectFileDownload | Download files directly | 🎨 UI | 2 |
| DisableCallIdle | Never kicked from idle DM calls | 🎨 UI | 2 |
| DisableDeepLinks | Stop web deep links | ⛔ N/A | web only |
| DontRoundMyTimestamps | Round relative timestamps down | 🎨 UI | 2 |
| Experiments | Discord experiments and dev features | 🌐 + 🎨 | done: data — `/phoecord experiments`; optional STAFF flag `enableIsStaff` (off, ban risk) / 2 (dev menu, settings page) |
| ExpressionManager | Clone, edit, delete emoji and stickers | 🎨 UI | 2 |
| F8Break | Pause on F8 with DevTools | ⛔ N/A | DevTools only |
| FakeNitro | Fake emoji and stickers, Nitro themes, Nitro stream quality | 🌐 Network | done: emoji; stickers, stream 1.2.5 |
| FakeProfileThemes | Profile themes hidden in the bio | 🌐 Network | 1.2.14 |
| FavoriteEmojiFirst | Favourite emoji first in autocomplete | 🎨 UI | 2 |
| FixCodeblockGap | Remove the gap under code blocks | 🖌️ Theme | 2.3 |
| FixImagesQuality | Load images at original resolution | 🌐 Network | 1.2.15 |
| FixSpotifyEmbeds | Volume for Spotify embeds | 🎨 UI | 2 |
| ForceOwnerCrown | Owner crown on large servers | 🎨 UI | 2 |
| FriendInvites | Friend invite links via commands | 🌐 Network | 1.2.12 |
| FullSearchContext | Full context menus on search results | 🎨 UI | 2 |
| GameActivityToggle | Toggle showing your game | 🌐 Network | done |
| GifPaste | Insert GIF links instead of sending | 🎨 UI | 2 |
| GifProviderSwitcher | Tenor or Giphy for GIF search | 🌐 Network | 1.2.15 |
| GreetStickerPicker | Pick any greet sticker | 🎨 UI | 2 |
| HideMedia | Hide attachments and embeds per message | 🎨 UI | 2 |
| IgnoreActivities | Hide chosen activities from your status | 🌐 Network | 1.2.13 |
| iLoveSpam | Show messages from likely spammers | 🌐 Network | done |
| ImageFilename | Image file name on hover | 🎨 UI | 2 |
| ImageLink | Never hide image links | 🎨 UI | 2 (the client decides from the embed type; no wire field keeps the link) |
| ImageZoom | Scroll to zoom images | 🎨 UI | 2 |
| ImplicitRelationships | Implicit relationships in Friends | 🌐 + 🎨 | done: data — `/phoecord implicit` / 2 (Friends tab) |
| IrcColors | Unique username colours | 🎨 UI | 2 |
| JumpToTop | Jump to the first message | 🎨 UI | 2 |
| KeepCurrentChannel | Return to the last channel after reload | 🎨 UI | 2 |
| LoadingQuotes | Custom loading quotes | 🎨 UI | 2 |
| MemberCount | Online, total and voice member counts | 🌐 + 🎨 | done: data — `/phoecord members` / 2 (counters in lists) |
| MentionAvatars | Avatars and role icons in mentions | 🎨 UI | 2 |
| MessageClickActions | Click to delete, double-click to edit or reply | 🎨 UI | 2 |
| MessageLatency | Mark messages that were slow to send | 🌐 + 🎨 | done: data — `/phoecord latency` / 2 (indicator) |
| MessageLinkEmbeds | Previews of linked messages | 🌐 + 🎨 | done: data — a rich embed per linked message, `/phoecord linkembeds` / 2 (message component) |
| MessageLogger | Keep deleted and edited messages | 🌐 + 🎨 | done: data / 2.3 |
| MoreQuickReactions | More quick reactions | 🎨 UI | 2 |
| MusicRichPresence | Last.fm and ListenBrainz Rich Presence | 🌐 Network | 1.2.13 |
| MutedServersActivityBadge | Activity badge on muted servers | 🎨 UI | 2 |
| MutualGroupDMs | Mutual group DMs on profiles | 🌐 + 🎨 | done: data — `/phoecord mutualgdms <user>` / 2 (profile tab) |
| NewGuildSettings | Auto-mute and set up servers you join | 🌐 Network | done: new servers get your mute/suppress settings, `/phoecord newguildsettings` |
| NoBlockedMessages | Hide blocked users' messages completely | 🌐 Network | done (history filtering is opt-in: `filter_history`) |
| NoDevtoolsWarning | Hide the DevTools warning | ⛔ N/A | DevTools only |
| NoF1 | Disable the F1 help key | 🎨 UI | 2 |
| NoMaskedUrlPaste | No masked link on paste over text | 🎨 UI | 2 |
| NoMiddleClickPaste | No middle-click paste on Linux | 🎨 UI | 2 |
| NoMosaic | Remove the image mosaic | 🖌️ Theme | 2.3 |
| NoOnboardingDelay | Skip the onboarding delay | 🎨 UI | 2 |
| NoPendingCount | Hide pending request counts | 🎨 UI | 2 (counts are computed on the client; hiding them on the wire would hide the requests too) |
| NoProfileThemes | Remove other people's Nitro profile themes | 🌐 Network | 1.2.14 |
| NoReplyMention | Reply without a ping by default | 🌐 Network | done |
| NoServerEmojis | No server emoji in autocomplete | 🎨 UI | 2 |
| NoSystemBadge | No taskbar unread badge | 🎨 UI | 2 |
| NoTrack | Block analytics and crash reports (on by default) | 🌐 Network | done |
| NoTypingAnimation | No typing dots animation | 🎨 UI | 2 |
| NoUnblockToJump | Jump to blocked users' messages | 🎨 UI | 2 |
| NotificationVolume | Separate notification volume | 🎨 UI | 2 |
| oneko | A cat that follows the mouse | 🎨 UI | 2 |
| OnePingPerDM | One ping per DM sender until read | 🎨 UI | 2 |
| OpenInApp | Open links in their apps | 🎨 UI | 2 |
| OverrideForumDefaults | Your own forum layout and sort | 🌐 Network | done: default forum layout and sort rewritten in payloads |
| PauseInvitesForever | Pause invites indefinitely | 🌐 + 🎨 | done: data — `/phoecord pauseinvites` / 2 (invite dialog link) |
| PermissionFreeWill | No client-side permission limits | 🎨 UI | 2 |
| PermissionsViewer | View user, channel and role permissions | 🎨 UI | 2 |
| petpet | /petpet headpat GIF command | 🌐 Network | 1.2.12 |
| PictureInPicture | Picture-in-picture for videos | 🎨 UI | 2 |
| PinDMs | Pin DMs to the top | 🎨 UI | 2 |
| PlainFolderIcon | Plain folder icons | 🎨 UI | 2 |
| PlatformIndicators | Desktop, mobile, web indicators | 🌐 + 🎨 | done: data — `/phoecord platforms` / 2 (icons) |
| PreviewMessage | Preview a message before sending | 🎨 UI | 2 |
| QuickMention | Quick mention button | 🎨 UI | 2 |
| QuickReply | Reply and edit with keybinds | 🎨 UI | 2 |
| ReactErrorDecoder | Readable React errors | ⛔ N/A | React only |
| ReadAllNotificationsButton | Read all notifications at once | 🌐 + 🎨 | done: data — `/phoecord readall` / 2 (button) |
| RelationshipNotifier | Notify when someone removes you | 🌐 Network | done |
| ReplaceGoogleSearch | Other search engines in menus | 🎨 UI | 2 |
| ReplyTimestamp | Timestamp on reply previews | 🎨 UI | 2 |
| RevealAllSpoilers | Reveal all spoilers in a message | 🎨 UI | 2 |
| ReverseImageSearch | Reverse image search in menus | 🎨 UI | 2 |
| ReviewDB | Reviews on user profiles | 🌐 + 🎨 | 1.2.17 / 2 |
| RoleColorEverywhere | Top role colour everywhere | 🎨 UI | 2 |
| SecretRingToneEnabler | Always the secret ringtone | 🎨 UI | 2 |
| SendTimestamps | Send timestamps with shortcuts | 🌐 Network | done (the picker button is Phase 2) |
| ServerInfo | Server info view | 🌐 + 🎨 | 1.2.12 / 2 |
| ServerListIndicators | Online friends or server count in the list | 🎨 UI | 2 |
| ShikiCodeblocks | VS Code-style code blocks | 🎨 UI | 2 |
| ShowAllMessageButtons | Always show all message buttons | 🎨 UI | 2 |
| ShowConnections | Connected accounts in popouts | 🎨 UI | 2 |
| ShowHiddenChannels | Show channels you cannot open | 🌐 + 🎨 | done: data — `/phoecord hidden`; optional READY rewrite `rewrite_payload` (off, ban risk) / 2 (lock icon, hidden page) |
| ShowHiddenThings | Show hidden and moderator-only things | 🌐 + 🎨 | done: data — timeouts and paused invites, `/phoecord hiddenthings` / 2 (icons, mod view) |
| ShowMeYourName | Usernames next to nicknames | 🎨 UI | 2 |
| ShowTimeoutDuration | How long a timeout lasts | 🎨 UI | 2 |
| SilentMessageToggle | Button for silent messages | 🌐 + 🎨 | done: data and `/phoecord silent` / 2 (chat bar button) |
| SilentTyping | Hide that you are typing | 🌐 Network | done |
| SortFriendRequests | Sort friend requests by date | 🎨 UI | 2 |
| SpotifyControls | Spotify player above the account panel | 🎨 UI | 2 |
| SpotifyCrack | Free listen-along, no auto-pause | ⛔ N/A | Spotify-side, out of scope |
| SpotifyShareCommands | /track, /album, /artist commands | 🌐 Network | 1.2.12 |
| StartupTimings | Startup timings in settings | ♻️ Replaced | PhoeCord status in its settings tab |
| StickerPaste | Insert stickers instead of sending | 🎨 UI | 2 |
| StreamerModeOnStream | Streamer mode while streaming | 🎨 UI | 2 |
| SuperReactionTweaks | Super reaction limits and default | 🎨 UI | 2 |
| TextReplace | Replace text in your messages by rules | 🌐 Network | done |
| ThemeAttributes | Data attributes for CSS themes | ♻️ Replaced | theme engine (no CSS in Phoenix) |
| Translate | Translate messages | 🌐 + 🎨 | 1.2.17 / 2 |
| TypingIndicator | Who is typing, per channel | 🌐 + 🎨 | done: data — `/phoecord typing` / 2 (dots and avatars) |
| TypingTweaks | Avatars and role colours in typing | 🎨 UI | 2 |
| Unindent | Trim indentation in code blocks | 🌐 Network | done: shared indentation removed from fenced code blocks of sent and edited messages (not yet inside `payload_json` of uploads) |
| UnlockedAvatarZoom | More zoom when cropping avatars | 🎨 UI | 2 |
| UnsuppressEmbeds | Unsuppress embeds | 🌐 + 🎨 | 1.2.12 / 2 |
| UserMessagesPronouns | Pronouns in chat | 🎨 UI | 2 |
| UserVoiceShow | Show who is in voice | 🌐 + 🎨 | done: data — `/phoecord voice` / 2 (badge, profile section) |
| USRBG | Community banners without Nitro | 🌐 Network | 1.2.14 |
| ValidReply | Fix 'message could not be loaded' in replies | 🌐 Network | done: missing `referenced_message` filled from the page or one lookup |
| ValidUser | Fix @unknown-user mentions | 🌐 Network | done: unknown mentioned users looked up (`GET /users/{id}`) and added to the message |
| VcNarrator | Announce voice joins and leaves | 🌐 + 🎨 | done: data — desktop notifications, no speech / 2 (TTS, test button) |
| VencordToolbox | Quick actions button | ♻️ Replaced | PhoeCord toolbox in the client (2.3) |
| VesktopPatches | Vesktop patches | ⛔ N/A | Vesktop only |
| ViewIcons | Click avatars and banners to view them | 🎨 UI | 2 |
| ViewRaw | View raw message, channel or server JSON | 🌐 + 🎨 | 1.2.12 / 2 |
| VoiceChatDoubleClick | Join voice on double-click | 🎨 UI | 2 |
| VoiceDownload | Download voice messages | 🎨 UI | 2 |
| VoiceMessages | Send voice messages like on mobile | 🌐 + 🎨 | 1.2.17 / 2 |
| VolumeBooster | Volume above 200% | 🎨 UI | 2 |
| WebContextMenus | Context menus on web | ⛔ N/A | web only |
| WebKeybinds | Keybinds on web | ⛔ N/A | web only |
| WebPWA | Install as a PWA | ⛔ N/A | browser only |
| WebRichPresence (arRPC) | Rich Presence on web | ⛔ N/A | web only |
| WebScreenShare | Screen share options on web | ⛔ N/A | browser only |
| WebScreenShareFixes | Screen share bitrate on web | ⛔ N/A | web only |
| WhoReacted | Avatars of who reacted | 🌐 + 🎨 | done: data — `/phoecord reactions <channel> <message> [emoji]` / 2 (avatars) |
| XSOverlay | Notifications to XSOverlay in VR | 🌐 Network | 1.2.17 |

## Vencord core modules

| Module | What it does | Layer | PhoeCord |
|:---|:---|:---|:---|
| Settings | Vencord settings UI | ♻️ Replaced | PhoeCord settings tab (2.3) and the `phoecord` CLI |
| FixDiscordCss | Fix Discord CSS lag | ⛔ N/A | no CSS in Phoenix |
| ConcatenatedComponentExtractor | Webpack internals | ⛔ N/A | Vencord internals |
| SupportHelper | Vencord support helpers | ♻️ Replaced | `phoecord selftest` and the bug template |

## Owner's userplugins

From Nanda's Vencord `userplugins` folder. They behave 1:1 like the owner's original scripts, with no caps or confirmations of PhoeCord's own (owner's ruling, 2026-11-25); their requests still go through the proxy's built-in rate limiter (5 requests per 5 s per plugin, 5/s overall, Discord's buckets respected). Discord can treat heavy automation of a user account as self-botting, so bulk actions (`VoiceChatUtilities`, `rmcord`, `SilentDelete`, `FollowUser`) carry a warning in their docs and command help; QuestMaster fakes quest progress, which is inauthentic engagement under Discord's Community Guidelines.

| Plugin | What it does | Layer | Where |
|:---|:---|:---|:---|
| SilentDelete | Replace a message with other text, then delete it, so loggers keep the replacement | 🌐 Network | done: data — `/phoecord silentdelete <message> [channel] [history]` and `/phoecord silentpurge <count> [channel]`: the script's placeholder post, `deleteDelay`, delete; `silentDeleteOnEmptyEdit` has no network effect / 2 (popover button, menu item) |
| FakeDeafen | Look deafened to others while still hearing | 🌐 Network | done: data — `/phoecord fakedeafen [on\|off]` rewrites outbound op 4 (`fakeMute`, `fakeDeafen`) and re-sends your voice state at once / 2 (button, keybind) |
| FakeDeafenDetector | Flag members who speak while marked muted or deafened | 🌐 + 🎨 | partial: data — `/phoecord flagged`; tracks mute/deafen claims, detection waits for a voice `SPEAKING` feed the proxy does not see yet / 2 (flag next to the name) |
| localDevBadges | Show staff and developer badges on your own profile, locally only | 🌐 Network | done: data — settings `discordStaff`, `earlyVerifiedBotDeveloper`, `activeDeveloper` add the badges to your own profile answer and set the flags on your own user (READY, USER_UPDATE, `/users/@me`, profile); nothing leaves the machine |
| FollowUser | Follow a user from voice channel to voice channel | 🌐 Network | done: data — `/phoecord follow [user]`, `/phoecord unfollow` or `followUserId`; joins via op 4 at once / 2 (context menu, header icon) |
| VoiceChatUtilities | Move, mute or disconnect everyone in a voice channel (rate-limited) | 🌐 + 🎨 | done: data — `/phoecord vcu <disconnect\|mute\|unmute\|deafen\|undeafen\|move\|stop\|status> <channel> [target]` patches every member one at a time (`waitAfter`, `waitSeconds`), within the proxy's rate limit / 2 ("Voice Tools" context menu) |
| rmcord | Bulk-delete your own messages (rate-limited) | 🌐 + 🎨 | done: data — `/phoecord rmcord action:<start\|schedule\|cancel\|pause\|resume\|stop\|status\|log\|help>` runs the script's engine (search 10 s, delete 600 ms, adaptive on 429) within the proxy's rate limit / 2 (panel, pickers, import) |
| AntiRickroll | Warn before opening rickroll links | 🌐 + 🎨 | done: data — `/phoecord rickrolls` lists messages with known or custom rickroll links, local lists only / 2 (warning under the message, click dialog) |
| BetterActivities | Activity icons in the member list, all activities shown | 🎨 UI | 2 |
| SoundboardToVoice | Play soundboard sounds through your microphone | 🎨 UI | 2 (audio pipeline) |
| QuestMaster | Quest dashboard and auto-completion | 🌐 + 🎨 | done: data — `/phoecord quests <list\|start\|stop\|status\|claim\|retry\|consent>` runs the script's engine on timers: video progress, game and stream heartbeats with the game's presence sent on the wire, the ACHIEVEMENT bypass through `*.discordsays.com`; desktop heartbeat shapes not yet checked on a live account / 2 (dashboard, sound) |
| TrustMeBro | Age verification bypass | 🌐 Network | not scheduled |


## How to read "Where"

- `done`: built and merged; `done: data` means the network part.
- `1.2.x`: a Phase 1 task in [`phase-1/ai/plan.md`](phase-1/ai/plan.md).
- `2`: Phase 2 UI work, ordered by popularity; the top 30 are listed in `ROADMAP.md` 2.4.
- `1.2.x / 2`: the network part in Phase 1, the on-screen part in Phase 2.

A new Vencord plugin appears? Add a row here in the same PR that adds it to a plan.
