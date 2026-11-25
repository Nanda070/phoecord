//! Hello: logs every `MESSAGE_CREATE`. It logs ids and a running count, never the text.

use phoecord::{Event, Manifest, Outcome, Plugin, host};

struct Hello {
    seen: u64,
}

#[phoecord::plugin]
impl Plugin for Hello {
    fn new() -> Self {
        Hello { seen: 0 }
    }

    fn manifest(&mut self) -> Manifest {
        let mut m = phoecord::manifest(
            "hello_plugin",
            "Hello (SDK example)",
            "Logs the ids of every new message",
        );
        m.authors = vec!["buslan07".into()];
        m
    }

    fn on_event(&mut self, event: Event) -> Outcome {
        if let Event::Dispatch(d) = event
            && d.name == "MESSAGE_CREATE"
        {
            // The payload is the whole gateway frame; the message is under `d`.
            let id = |key| d.payload.id_at(&["d", key]).unwrap_or(0);
            self.seen += 1;
            host::log(&format!(
                "MESSAGE_CREATE #{}: message {} in channel {}",
                self.seen,
                id("id"),
                id("channel_id")
            ));
        }
        Outcome::pass()
    }
}
