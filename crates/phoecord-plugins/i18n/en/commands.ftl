# The /phoecord command. Every `/phoecord <name>` answer is rendered while this plugin runs, so
# the answers of other plugins' subcommands live here too, with the plugin id as key prefix.
plugins-header = PhoeCord plugins
plugin-off = The `{ $id }` plugin is off. Switch it on with `/phoecord toggle { $id }`.
unknown-command = Unknown command `{ $name }`.
toggle-unknown = No plugin named `{ $id }`. Try `/phoecord plugins`.
toggle-self = `{ $id }` cannot switch itself off here; use `phoecord disable { $id }`.
toggle-on = `{ $id }` is now on.
toggle-off = `{ $id }` is now off.
toggle-reconnect = Takes effect when Discord reconnects (Ctrl+R).
toggle-failed = Could not switch `{ $id }`: { $error }
owned-no-reply = The `/phoecord { $name }` command did not run. Is the plugin on? Switch it on with `/phoecord toggle { $id }`.
# fake_deafen
fake_deafen-on = Fake deafen is **on**: others see you muted and deafened.
fake_deafen-off = Fake deafen is **off**.
fake_deafen-unknown-state = Unknown state `{ $state }`. Use `on` or `off`.
# follow_user
follow_user-following = Following user `{ $id }`.
follow_user-trigger = Joining the followed user's channel.
follow_user-unfollowed = No longer following anyone.
follow_user-bad-user = `{ $user }` is not a user id or mention.

# FakeDeafenDetector: `/phoecord flagged`.
fake_deafen_detector-none = Nobody has been caught speaking while shown as muted or deafened.
fake_deafen_detector-header = Caught speaking while shown as muted or deafened: { $count }
fake_deafen_detector-line = { $user }: caught speaking { $count }x while shown as muted/deafened (first: { $first }, last: { $last }), likely faking it

# AntiRickroll: `/phoecord rickrolls`.
anti_rickroll-none = No rickroll links seen yet.
anti_rickroll-header = Messages with rickroll links: { $count }
anti_rickroll-line-known = { $channel } message { $message }: `{ $url }` is a known rickroll.
anti_rickroll-line-custom = { $channel } message { $message }: `{ $url }` matches one of your filters for rickrolls.

# VoiceChatUtilities: `/phoecord vcu`.
voice_chat_utilities-started = Started { $action } for { $count } member(s) of <#{ $channel }>.
voice_chat_utilities-stopped = Stopped { $count } running job(s).
voice_chat_utilities-idle = No voice channel action is running.
voice_chat_utilities-status-line = { $action } in <#{ $channel }>: { $done } of { $count } done.
voice_chat_utilities-bad-action = Unknown action `{ $action }`. Use disconnect, mute, unmute, deafen, undeafen, move, stop or status.
voice_chat_utilities-need-channel = Give the voice channel as an id or a mention.
voice_chat_utilities-unknown-channel = Channel `{ $channel }` is not known.
voice_chat_utilities-not-voice = <#{ $channel }> is not a voice or stage channel.
voice_chat_utilities-need-target = Move needs a target voice channel.
voice_chat_utilities-bad-target = `{ $channel }` is not another voice channel of the same guild.
voice_chat_utilities-empty = Nobody is in <#{ $channel }>.
voice_chat_utilities-nobody = Nobody but you is in <#{ $channel }>.

# rmcord: the command ran without the plugin handling the interaction (shown by the command plugin).
rmcord-no-reply = The rmcord command did not run. Is the plugin on? Switch it on with `/phoecord toggle rmcord`.

# SilentDelete: the command ran without the plugin handling the interaction (shown by the command plugin).
silent_delete-no-reply = The SilentDelete command did not run. Is the plugin on? Switch it on with `/phoecord toggle silent_delete`.
voice_chat_utilities-no-reply = Nothing to show: run it as `/phoecord vcu`.
# QuestMaster: `/phoecord quests`.
quest_master-starting = QuestMaster is starting; progress goes to the PhoeCord log.
quest_master-already = QuestMaster is already running. `/phoecord quests status` shows it; `/phoecord quests retry` restarts it.
quest_master-stopping = QuestMaster is stopping.
quest_master-not-running = QuestMaster is not running.
quest_master-retry = QuestMaster restarts in a moment; then pick quests with `/phoecord quests start`.
quest_master-claiming = Claiming the reward of `{ $id }`; the result goes to the log.
quest_master-claim-none = `{ $id }` has no reward to claim.
quest_master-claim-missing = Name the quest: `/phoecord quests claim <quest id>`.
quest_master-unknown-action = Unknown action `{ $action }`. Use list, start, stop, status, claim or retry.
quest_master-list-empty = No quests loaded yet; loading them now. Run the command again in a moment.
quest_master-list-header = Quests: { $count }
quest_master-list-line = `{ $id }` { $name }: { $type }, { $label }: { $reward }
quest_master-status-header = QuestMaster: { $phase }, loop #{ $loop }
quest_master-status-empty = No tasks yet.
quest_master-status-line = `{ $id }` { $name }: { $type }, { $state }, { $cur }/{ $max }
quest_master-phase-idle = not running
quest_master-phase-loading = loading quests
quest_master-phase-picker = waiting for `/phoecord quests start`
quest_master-phase-running = running
quest_master-phase-finished = cycle finished
quest_master-st-queue = in queue
quest_master-st-pending = pending
quest_master-st-running = running
quest_master-st-completed = completed
quest_master-st-claimed = claimed
quest_master-st-failed = aborted
quest_master-st-enroll = action required: accept the quest in Discord
quest_master-st-claimable = completed, claim with `/phoecord quests claim { $id }`
quest_master-st-claim-waiting = claiming...
quest_master-st-claim-failed = action required: check Discord for a captcha
quest_master-consent-none = No authorization is waiting for an answer.
quest_master-consent-other = The waiting authorization is for app `{ $id }`.
quest_master-consent-yes = Authorizing app `{ $id }` for the achievement quest.
quest_master-consent-no = Not authorizing app `{ $id }`.
