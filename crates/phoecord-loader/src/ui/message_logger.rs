//! What a `MessageRow::render` wrap would show for a logged message (task 2.3.4).
//!
//! Pure data in, accessory out; not called from any hook yet. `MessageRow::render`'s address is
//! not in the private `phoecord-recon` notes — only the app-level render chain
//! (`docs/re/gpui-lifecycle.md`) is mapped, not the message list inside it — and the task says
//! not to find out by rewriting message content the way `phoecord_plugins::message_logger` does
//! today for the proxy path ("do not rewrite message content; use element wrapping only"). Once
//! the real render function is found, a detour would read a message's [`MessageState`] (from
//! that plugin's cache — nothing wires the two together yet, since
//! `crates/phoecord-core/src/plugin.rs` is owner-only and no task has asked for that API) and
//! call [`Accessory::for_message`] to decide what to add.

/// What accessory, if any, a message row should show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Accessory {
    /// Strikethrough tint and a "deleted" indicator.
    Deleted,
    /// Shows `history` (oldest first) on hover; the current content is not part of it.
    Edited { history: Vec<String> },
}

/// A message's logged state, as a future hook would read it out of
/// `phoecord_plugins::message_logger`'s cache (shape mirrored here, not shared code: that
/// plugin's cache is private and runs in a different process today).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MessageState {
    pub deleted: bool,
    /// Earlier versions, oldest first; empty if the message was never edited.
    pub history: Vec<String>,
}

impl Accessory {
    /// `None` for an untouched message. A deleted message always wins over its edit history: once
    /// deleted, what it used to say stops mattering for the row's own accessory (the history is
    /// still there in `state.history` for whoever reads it directly).
    pub fn for_message(state: &MessageState) -> Option<Accessory> {
        if state.deleted {
            return Some(Accessory::Deleted);
        }
        if state.history.is_empty() {
            return None;
        }
        Some(Accessory::Edited {
            history: state.history.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untouched_message_has_no_accessory() {
        assert_eq!(Accessory::for_message(&MessageState::default()), None);
    }

    #[test]
    fn edited_message_carries_its_history_oldest_first() {
        let state = MessageState {
            deleted: false,
            history: vec!["first".into(), "second".into()],
        };
        assert_eq!(
            Accessory::for_message(&state),
            Some(Accessory::Edited {
                history: vec!["first".into(), "second".into()]
            })
        );
    }

    #[test]
    fn a_deleted_message_shows_deleted_even_with_edit_history() {
        let state = MessageState {
            deleted: true,
            history: vec!["edited once".into()],
        };
        assert_eq!(Accessory::for_message(&state), Some(Accessory::Deleted));
    }
}
