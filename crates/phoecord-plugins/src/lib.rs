//! Built-in plugins, native Rust, on the unified Plugin API.
//!
//! Porting the v0 plugins (docs/ROADMAP.md, milestone 1.2): `no_track`, `silent_typing`,
//! `message_logger`, `fake_nitro` (emoji), `relationship_notifier`, `game_activity_toggle`,
//! `unindent` and the new `clear_urls` are done. Once the WASM runtime is stable, each one moves
//! to its own WASM package and this crate shrinks to the few plugins that must stay native.

use phoecord_core::Plugin;

pub mod anonymise_file_names;
pub mod anti_rickroll;
pub mod better_sessions;
pub mod clear_urls;
pub mod commands;
pub mod experiments;
pub mod fake_deafen;
pub mod fake_deafen_detector;
pub mod fake_nitro;
pub mod follow_user;
pub mod friend_invites;
pub mod game_activity_toggle;
pub mod i_love_spam;
pub mod implicit_relationships;
pub mod local_dev_badges;
pub mod member_count;
pub mod message_latency;
pub mod message_link_embeds;
pub mod message_logger;
pub mod mutual_group_dms;
pub mod new_guild_settings;
pub mod no_blocked_messages;
pub mod no_reply_mention;
pub mod no_track;
pub mod override_forum_defaults;
pub mod pause_invites_forever;
pub mod platform_indicators;
pub mod quest_master;
pub mod read_all_notifications;
pub mod relationship_notifier;
pub mod rmcord;
pub mod send_timestamps;
pub mod show_hidden_channels;
pub mod show_hidden_things;
pub mod silent_delete;
pub mod silent_message_toggle;
pub mod silent_typing;
pub mod state;
pub mod text_replace;
pub mod typing_indicator;
pub mod unindent;
pub mod unsuppress_embeds;
pub mod user_voice_show;
pub mod valid_reply;
pub mod valid_user;
pub mod vc_narrator;
pub mod view_raw;
pub mod voice_chat_utilities;
pub mod who_reacted;

#[cfg(test)]
mod testutil;

/// The built-in plugins' own strings as `(plugin id, language, Fluent text)`: one file per plugin
/// and language, `crates/phoecord-plugins/i18n/<lang>/<plugin id>.ftl`. A plugin without a file
/// has no strings of its own. `scripts/check_i18n.py` checks `ru` against `en`.
pub const I18N: &[(&str, &str, &str)] = &[
    (
        "better_sessions",
        "en",
        include_str!("../i18n/en/better_sessions.ftl"),
    ),
    (
        "better_sessions",
        "ru",
        include_str!("../i18n/ru/better_sessions.ftl"),
    ),
    ("commands", "en", include_str!("../i18n/en/commands.ftl")),
    ("commands", "ru", include_str!("../i18n/ru/commands.ftl")),
    (
        "follow_user",
        "en",
        include_str!("../i18n/en/follow_user.ftl"),
    ),
    (
        "follow_user",
        "ru",
        include_str!("../i18n/ru/follow_user.ftl"),
    ),
    (
        "friend_invites",
        "en",
        include_str!("../i18n/en/friend_invites.ftl"),
    ),
    (
        "friend_invites",
        "ru",
        include_str!("../i18n/ru/friend_invites.ftl"),
    ),
    (
        "relationship_notifier",
        "en",
        include_str!("../i18n/en/relationship_notifier.ftl"),
    ),
    (
        "relationship_notifier",
        "ru",
        include_str!("../i18n/ru/relationship_notifier.ftl"),
    ),
    (
        "unsuppress_embeds",
        "en",
        include_str!("../i18n/en/unsuppress_embeds.ftl"),
    ),
    (
        "unsuppress_embeds",
        "ru",
        include_str!("../i18n/ru/unsuppress_embeds.ftl"),
    ),
    (
        "vc_narrator",
        "en",
        include_str!("../i18n/en/vc_narrator.ftl"),
    ),
    (
        "vc_narrator",
        "ru",
        include_str!("../i18n/ru/vc_narrator.ftl"),
    ),
    ("view_raw", "en", include_str!("../i18n/en/view_raw.ftl")),
    ("view_raw", "ru", include_str!("../i18n/ru/view_raw.ftl")),
    (
        "voice_chat_utilities",
        "en",
        include_str!("../i18n/en/voice_chat_utilities.ftl"),
    ),
    (
        "voice_chat_utilities",
        "ru",
        include_str!("../i18n/ru/voice_chat_utilities.ftl"),
    ),
    ("rmcord", "en", include_str!("../i18n/en/rmcord.ftl")),
    ("rmcord", "ru", include_str!("../i18n/ru/rmcord.ftl")),
    (
        "silent_delete",
        "en",
        include_str!("../i18n/en/silent_delete.ftl"),
    ),
    (
        "silent_delete",
        "ru",
        include_str!("../i18n/ru/silent_delete.ftl"),
    ),
    (
        "quest_master",
        "en",
        include_str!("../i18n/en/quest_master.ftl"),
    ),
    (
        "quest_master",
        "ru",
        include_str!("../i18n/ru/quest_master.ftl"),
    ),
];

/// Hand [`I18N`] to the host's plugin strings, which answer `Host::t`.
pub fn register_strings(strings: &phoecord_i18n::PluginStrings) {
    for (id, lang, ftl) in I18N {
        strings.set(id, lang, ftl);
    }
}

/// Every built-in plugin, in default run order.
pub fn builtin() -> Vec<Box<dyn Plugin>> {
    let silent = silent_message_toggle::SilentMessageToggle::new();
    let silent_command = silent.command();
    // Stage 1.2.16 / 1.2.18 data parts (off by default); the ones with a `/phoecord` command
    // share their state with it.
    let message_link_embeds = message_link_embeds::MessageLinkEmbeds::new();
    let message_link_embeds_command = message_link_embeds.command();
    let new_guild_settings = new_guild_settings::NewGuildSettings::new();
    let new_guild_settings_command = new_guild_settings.command();
    let pause_invites_forever = pause_invites_forever::PauseInvitesForever::new();
    let pause_invites_forever_command = pause_invites_forever.command();
    let read_all_notifications = read_all_notifications::ReadAllNotificationsButton::new();
    let read_all_notifications_command = read_all_notifications.command();
    let platform_indicators = platform_indicators::PlatformIndicators::new();
    let platform_indicators_command = platform_indicators.command();
    let typing_indicator = typing_indicator::TypingIndicator::new();
    let typing_indicator_command = typing_indicator.command();
    let user_voice_show = user_voice_show::UserVoiceShow::new();
    let user_voice_show_command = user_voice_show.command();
    let member_count = member_count::MemberCount::new();
    let member_count_command = member_count.command();
    let better_sessions = better_sessions::BetterSessions::new();
    let better_sessions_command = better_sessions.command();
    let implicit_relationships = implicit_relationships::ImplicitRelationships::new();
    let implicit_relationships_command = implicit_relationships.command();
    let mutual_group_dms = mutual_group_dms::MutualGroupDms::new();
    let mutual_group_dms_command = mutual_group_dms.command();
    let who_reacted = who_reacted::WhoReacted::new();
    let who_reacted_command = who_reacted.command();
    let message_latency = message_latency::MessageLatency::new();
    let message_latency_command = message_latency.command();
    let show_hidden_channels = show_hidden_channels::ShowHiddenChannels::new();
    let show_hidden_channels_command = show_hidden_channels.command();
    let show_hidden_things = show_hidden_things::ShowHiddenThings::new();
    let show_hidden_things_command = show_hidden_things.command();
    let experiments = experiments::Experiments::new();
    let experiments_command = experiments.command();
    let fake_deafen = fake_deafen::FakeDeafen::new();
    let fake_deafen_command = fake_deafen.command();
    let fake_deafen_detector = fake_deafen_detector::FakeDeafenDetector::new();
    let fake_deafen_detector_command = fake_deafen_detector.command();
    let follow_user = follow_user::FollowUser::new();
    let follow_user_commands = follow_user.commands();
    let anti_rickroll = anti_rickroll::AntiRickroll::new();
    let anti_rickroll_command = anti_rickroll.command();
    let voice_chat_utilities = voice_chat_utilities::VoiceChatUtilities::new();
    let voice_chat_utilities_command = voice_chat_utilities.command();
    let rmcord = rmcord::Rmcord::new();
    let rmcord_command = rmcord.command();
    let silent_delete = silent_delete::SilentDelete::new();
    let silent_delete_commands = silent_delete.commands();
    let quest_master = quest_master::QuestMaster::new();
    let quest_master_command = quest_master.command();
    // Stage 1.2.12 commands (off by default).
    let view_raw = view_raw::ViewRaw::new();
    let view_raw_command = view_raw.command();
    let unsuppress_embeds = unsuppress_embeds::UnsuppressEmbeds::new();
    let unsuppress_embeds_command = unsuppress_embeds.command();
    let friend_invites = friend_invites::FriendInvites::new();
    let friend_invites_commands = friend_invites.commands();
    let mut plugins: Vec<Box<dyn Plugin>> = vec![
        Box::new(no_track::NoTrack::new()),
        Box::new(silent_typing::SilentTyping::new()),
        Box::new(clear_urls::ClearUrls::new()),
        Box::new(unindent::Unindent::new()),
        Box::new(game_activity_toggle::GameActivityToggle::new()),
        Box::new(relationship_notifier::RelationshipNotifier::new()),
        Box::new(fake_nitro::FakeNitro::new()),
        Box::new(message_logger::MessageLogger::new()),
        Box::new(i_love_spam::ILoveSpam::new()),
        Box::new(no_blocked_messages::NoBlockedMessages::new()),
        Box::new(no_reply_mention::NoReplyMention::new()),
        Box::new(text_replace::TextReplace::new()),
        Box::new(send_timestamps::SendTimestamps::new()),
        Box::new(anonymise_file_names::AnonymiseFileNames::new()),
        Box::new(silent),
        Box::new(valid_user::ValidUser::new()),
        Box::new(valid_reply::ValidReply::new()),
        Box::new(message_link_embeds),
        Box::new(new_guild_settings),
        Box::new(override_forum_defaults::OverrideForumDefaults::new()),
        Box::new(pause_invites_forever),
        Box::new(read_all_notifications),
        Box::new(platform_indicators),
        Box::new(typing_indicator),
        Box::new(user_voice_show),
        Box::new(vc_narrator::VcNarrator::new()),
        Box::new(member_count),
        Box::new(better_sessions),
        Box::new(implicit_relationships),
        Box::new(mutual_group_dms),
        Box::new(who_reacted),
        Box::new(message_latency),
        Box::new(show_hidden_channels),
        Box::new(show_hidden_things),
        Box::new(experiments),
        Box::new(fake_deafen),
        Box::new(fake_deafen_detector),
        Box::new(follow_user),
        Box::new(anti_rickroll),
        Box::new(local_dev_badges::LocalDevBadges::new()),
        Box::new(voice_chat_utilities),
        Box::new(rmcord),
        Box::new(silent_delete),
        Box::new(quest_master),
        Box::new(friend_invites),
        Box::new(unsuppress_embeds),
    ];
    // `/phoecord plugins` lists everything above and the commands plugin itself. Register more
    // `/phoecord` subcommands here with `Commands::with`.
    let commands = commands::Commands::empty();
    let mut infos: Vec<_> = plugins
        .iter()
        .map(|p| commands::PluginInfo::of(p.as_ref()))
        .collect();
    infos.push(commands::PluginInfo::of(&commands));
    let mut commands = commands::Commands::new(infos)
        .with(silent_command)
        .with(message_link_embeds_command)
        .with(new_guild_settings_command)
        .with(pause_invites_forever_command)
        .with(read_all_notifications_command)
        .with(platform_indicators_command)
        .with(typing_indicator_command)
        .with(user_voice_show_command)
        .with(member_count_command)
        .with(better_sessions_command)
        .with(implicit_relationships_command)
        .with(mutual_group_dms_command)
        .with(who_reacted_command)
        .with(message_latency_command)
        .with(show_hidden_channels_command)
        .with(show_hidden_things_command)
        .with(experiments_command)
        .with(fake_deafen_command)
        .with(fake_deafen_detector_command)
        .with(anti_rickroll_command)
        .with(voice_chat_utilities_command)
        .with(rmcord_command)
        .with(quest_master_command);
    for c in silent_delete_commands {
        commands = commands.with(c);
    }
    for c in follow_user_commands {
        commands = commands.with(c);
    }
    commands = commands.with(unsuppress_embeds_command);
    commands = commands.with(view_raw_command);
    for c in friend_invites_commands {
        commands = commands.with(c);
    }
    plugins.push(Box::new(commands));
    plugins
}

#[cfg(test)]
mod tests {
    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<_> = super::builtin()
            .iter()
            .map(|p| p.manifest().id.clone())
            .collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }
}
