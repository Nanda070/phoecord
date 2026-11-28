//! Typed locators: `Found<T>` for the hook targets of the in-process transport.
//!
//! A locator resolves one named entry of the `signatures/*.toml` table of the running build
//! (format: `signatures/README.md`) and tags the address with what it is. The "unique or off" rule
//! of [`crate::signatures`] applies: zero matches, several matches and an address outside the
//! module are all failures, and each failure disables only the hook that needed the locator.
//!
//! Entry names this module asks for:
//!
//! | Name | Target | Used by |
//! |:---|:---|:---|
//! | `http_send` | entry of `ReqwestTransport::send` (inline hook) | REST requests |
//! | `http_poll` | the response future's vtable (through `rip`: the one instruction that loads it); `poll` is its slot at [`POLL_SLOT`] | REST responses |
//! | `bytes_static_vtable` | `bytes`'s static `Bytes` vtable (no-op drop, never unique), through `rip` | replacing a request body |
//! | `rust_alloc`, `rust_dealloc` | the host's allocator shims | replacing a body |
//! | `gateway_registry` | the build/insert function the first static pass took for the event registry; live it is not on the dispatch path (2.2.3), kept for 2.2.5 | none |
//! | `gateway_inflate` | the function that inflates one gateway frame into its JSON text (`zstd-stream`), entry for an inline hook | Gateway dispatch |
//! | `gateway_send` | entry of the outgoing frame encoder (`gateway_send_encode` in the recon notes) | Gateway send |
//! | `gateway_frame` | entry of the per-frame handler the inflate wrapper calls with each frame's text (inline hook) | queued server → client payloads |
//! | `client_state` | entry of the `ClientState` serializer `IDENTIFY`'s builder calls (inline hook, installed from `DllMain`) | `full_ready` (2.2.7) |
//! | `message_menu_push` | entry of the helper that appends one entry record to the message menu's `Vec` (inline hook) | message menu (2.3.5) |
//! | `message_menu_after_anchor` | the instruction right after the call that appends the native "Copy Message Link" entry (a return address) | message menu |
//! | `user_menu_push`, `channel_menu_push`, `server_menu_push` | entry of that menu's entry-append helper (inline hook; menus may share one) | context menus (2.3.11) |
//! | `user_menu_after_anchor`, `channel_menu_after_anchor`, `server_menu_after_anchor` | the instruction after the call that appends the menu's anchor entry ("Block", "Copy Link", "Mark As Read") | context menus |
//! | `link_website_trusted`, `link_protocol_trusted` | entry of the link classifier's "is this website / app protocol trusted" check (inline hook) | AlwaysTrust (2.3.20) |
//! | `link_suspicious_download` | entry of the suspicious-download detector the classifier calls (inline hook) | AlwaysTrust |
//! | `volume_user_ceiling`, `volume_stream_ceiling` | [`CEILING_LOAD_OFFSET`] bytes before the `movss` that loads the 200 % ceiling in the user / stream volume clamp (operand patch) | VolumeBooster (2.3.16) |
//! | `message_menu_build_call` | the `call` (E8 rel32) of the menu build function in its live caller; its target is the build function | message menu |

use crate::signatures::{ResolveError, Signature};
use std::fmt;

pub const HTTP_SEND: &str = "http_send";
pub const HTTP_POLL: &str = "http_poll";
pub const BYTES_STATIC: &str = "bytes_static_vtable";
/// Offset of `poll` in a Rust trait-object vtable for `dyn Future`: drop, size, align, then `poll`.
pub const POLL_SLOT: u32 = 0x18;
pub const RUST_ALLOC: &str = "rust_alloc";
pub const RUST_DEALLOC: &str = "rust_dealloc";
pub const GATEWAY_REGISTRY: &str = "gateway_registry";
pub const GATEWAY_SEND: &str = "gateway_send";
pub const GATEWAY_INFLATE: &str = "gateway_inflate";
pub const GATEWAY_FRAME: &str = "gateway_frame";
pub const CLIENT_STATE: &str = "client_state";
pub const MENU_PUSH: &str = "message_menu_push";
pub const MENU_AFTER_ANCHOR: &str = "message_menu_after_anchor";
pub const MENU_BUILD_CALL: &str = "message_menu_build_call";
pub const USER_PUSH: &str = "user_menu_push";
pub const USER_AFTER: &str = "user_menu_after_anchor";
pub const CHANNEL_PUSH: &str = "channel_menu_push";
pub const CHANNEL_AFTER: &str = "channel_menu_after_anchor";
pub const SERVER_PUSH: &str = "server_menu_push";
pub const SERVER_AFTER: &str = "server_menu_after_anchor";
pub const LINK_WEBSITE_TRUSTED: &str = "link_website_trusted";
pub const LINK_PROTOCOL_TRUSTED: &str = "link_protocol_trusted";
pub const LINK_SUSPICIOUS_DOWNLOAD: &str = "link_suspicious_download";
pub const VOLUME_USER_CEILING: &str = "volume_user_ceiling";
pub const VOLUME_STREAM_CEILING: &str = "volume_stream_ceiling";
/// A volume ceiling signature starts this many bytes before the instruction that loads the
/// ceiling: the bytes that tell the two clamps apart come first.
pub const CEILING_LOAD_OFFSET: u32 = 36;

/// Why a locator found nothing usable.
#[derive(Debug, PartialEq, Eq)]
pub enum LocatorError {
    /// The signature table of this build has no entry of that name.
    Missing(&'static str),
    /// The entry exists but did not resolve to exactly one place.
    Unresolved(&'static str, ResolveError),
    /// It resolved to an address outside the module image.
    OutOfImage(&'static str, u32),
}

impl fmt::Display for LocatorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LocatorError::Missing(n) => write!(f, "{n}: no signature for this build"),
            LocatorError::Unresolved(n, e) => write!(f, "{n}: {e}"),
            LocatorError::OutOfImage(n, rva) => write!(f, "{n}: rva {rva:#x} is outside the image"),
        }
    }
}

impl std::error::Error for LocatorError {}

/// A located address (an RVA) tagged with what it points at.
#[derive(Debug, PartialEq, Eq)]
pub struct Found<T> {
    pub rva: u32,
    pub target: T,
}

/// Entry of the transport's `send` function.
#[derive(Debug, PartialEq, Eq)]
pub struct HttpSend;
/// The response future's vtable; its `poll` slot is at [`POLL_SLOT`].
#[derive(Debug, PartialEq, Eq)]
pub struct HttpPoll;
/// `bytes`'s static vtable: a `Bytes` with it never frees its buffer.
#[derive(Debug, PartialEq, Eq)]
pub struct BytesStatic;
/// The host's `__rust_alloc` and `__rust_dealloc` (the second is `dealloc_rva`).
#[derive(Debug, PartialEq, Eq)]
pub struct Allocator {
    pub dealloc_rva: u32,
}
/// The gateway event registry.
#[derive(Debug, PartialEq, Eq)]
pub struct GatewayRegistry;
/// The function that inflates one incoming gateway frame into its JSON text (2.2.3).
#[derive(Debug, PartialEq, Eq)]
pub struct GatewayInflate;
/// The encoder of outgoing gateway frames (2.2.4): takes a typed command, not JSON.
#[derive(Debug, PartialEq, Eq)]
pub struct GatewaySend;
/// The client's per-frame gateway handler (2.2.3): delivers queued payloads.
#[derive(Debug, PartialEq, Eq)]
pub struct GatewayFrame;
/// The serializer of the gateway session's `ClientState`, which `IDENTIFY` carries (2.2.7).
#[derive(Debug, PartialEq, Eq)]
pub struct ClientState;
/// The link classifier's check that a website is trusted (2.3.20).
#[derive(Debug, PartialEq, Eq)]
pub struct LinkWebsiteTrusted;
/// The same check for an app protocol (`steam://`, `ms-settings:`).
#[derive(Debug, PartialEq, Eq)]
pub struct LinkProtocolTrusted;
/// The detector of download links the client warns about.
#[derive(Debug, PartialEq, Eq)]
pub struct LinkSuspiciousDownload;
/// The instruction loading the ceiling of a user's volume (2.3.16).
#[derive(Debug, PartialEq, Eq)]
pub struct UserVolumeCeiling;
/// The same instruction of a stream's volume.
#[derive(Debug, PartialEq, Eq)]
pub struct StreamVolumeCeiling;
/// Another context menu's entry-append helper (the `Found` RVA) and its anchor.
#[derive(Debug, PartialEq, Eq)]
pub struct ContextMenu {
    pub after_anchor_rva: u32,
}

/// The message menu's entry-append helper (the `Found` RVA), with the two other places the hook
/// needs.
#[derive(Debug, PartialEq, Eq)]
pub struct MessageMenu {
    /// Return address of the call that appends the native "Copy Message Link" entry.
    pub after_anchor_rva: u32,
    /// The `call` of the build function (E8 rel32) in its live caller.
    pub build_call_rva: u32,
}

/// What a locator scans: the signatures of the selected build and the client's `.text`.
pub struct Scan<'a> {
    pub sigs: &'a [Signature],
    pub text: &'a [u8],
    pub text_rva: u32,
    /// `SizeOfImage`: every result must lie below it.
    pub image_len: u32,
}

impl Scan<'_> {
    fn rva(&self, name: &'static str) -> Result<u32, LocatorError> {
        let sig = self
            .sigs
            .iter()
            .find(|s| s.name == name)
            .ok_or(LocatorError::Missing(name))?;
        let rva = sig
            .resolve(self.text, self.text_rva)
            .map_err(|e| LocatorError::Unresolved(name, e))?;
        if rva >= self.image_len {
            return Err(LocatorError::OutOfImage(name, rva));
        }
        Ok(rva)
    }

    pub fn http_send(&self) -> Result<Found<HttpSend>, LocatorError> {
        Ok(Found {
            rva: self.rva(HTTP_SEND)?,
            target: HttpSend,
        })
    }

    pub fn http_poll(&self) -> Result<Found<HttpPoll>, LocatorError> {
        Ok(Found {
            rva: self.rva(HTTP_POLL)?,
            target: HttpPoll,
        })
    }

    pub fn bytes_static(&self) -> Result<Found<BytesStatic>, LocatorError> {
        Ok(Found {
            rva: self.rva(BYTES_STATIC)?,
            target: BytesStatic,
        })
    }

    /// Both shims or neither: a body allocated by one and freed by the wrong one corrupts the heap.
    pub fn allocator(&self) -> Result<Found<Allocator>, LocatorError> {
        let alloc = self.rva(RUST_ALLOC)?;
        let dealloc_rva = self.rva(RUST_DEALLOC)?;
        Ok(Found {
            rva: alloc,
            target: Allocator { dealloc_rva },
        })
    }

    pub fn gateway_registry(&self) -> Result<Found<GatewayRegistry>, LocatorError> {
        Ok(Found {
            rva: self.rva(GATEWAY_REGISTRY)?,
            target: GatewayRegistry,
        })
    }

    pub fn gateway_inflate(&self) -> Result<Found<GatewayInflate>, LocatorError> {
        Ok(Found {
            rva: self.rva(GATEWAY_INFLATE)?,
            target: GatewayInflate,
        })
    }

    /// The append helper and anchor of another context menu (`user`, `channel`, `server`): the
    /// `Found` RVA is the helper, the target the anchor's return address. Only with the `dll`
    /// feature: `MenuKind` comes from `phoecord-core`, which the CLI, installer and proxy do not link.
    #[cfg(feature = "dll")]
    pub fn context_menu(
        &self,
        kind: phoecord_core::MenuKind,
    ) -> Result<Found<ContextMenu>, LocatorError> {
        let (push, after) = match kind {
            phoecord_core::MenuKind::User => (USER_PUSH, USER_AFTER),
            phoecord_core::MenuKind::Channel => (CHANNEL_PUSH, CHANNEL_AFTER),
            phoecord_core::MenuKind::Server => (SERVER_PUSH, SERVER_AFTER),
            _ => return Err(LocatorError::Missing(MENU_PUSH)),
        };
        Ok(Found {
            rva: self.rva(push)?,
            target: ContextMenu {
                after_anchor_rva: self.rva(after)?,
            },
        })
    }

    /// All three places of the message menu hook, or none.
    pub fn message_menu(&self) -> Result<Found<MessageMenu>, LocatorError> {
        Ok(Found {
            rva: self.rva(MENU_PUSH)?,
            target: MessageMenu {
                after_anchor_rva: self.rva(MENU_AFTER_ANCHOR)?,
                build_call_rva: self.rva(MENU_BUILD_CALL)?,
            },
        })
    }

    pub fn link_website_trusted(&self) -> Result<Found<LinkWebsiteTrusted>, LocatorError> {
        Ok(Found {
            rva: self.rva(LINK_WEBSITE_TRUSTED)?,
            target: LinkWebsiteTrusted,
        })
    }

    pub fn link_protocol_trusted(&self) -> Result<Found<LinkProtocolTrusted>, LocatorError> {
        Ok(Found {
            rva: self.rva(LINK_PROTOCOL_TRUSTED)?,
            target: LinkProtocolTrusted,
        })
    }

    pub fn link_suspicious_download(&self) -> Result<Found<LinkSuspiciousDownload>, LocatorError> {
        Ok(Found {
            rva: self.rva(LINK_SUSPICIOUS_DOWNLOAD)?,
            target: LinkSuspiciousDownload,
        })
    }

    fn ceiling_load(&self, name: &'static str) -> Result<u32, LocatorError> {
        let rva = self.rva(name)?.saturating_add(CEILING_LOAD_OFFSET);
        if rva >= self.image_len {
            return Err(LocatorError::OutOfImage(name, rva));
        }
        Ok(rva)
    }

    pub fn user_volume_ceiling(&self) -> Result<Found<UserVolumeCeiling>, LocatorError> {
        Ok(Found {
            rva: self.ceiling_load(VOLUME_USER_CEILING)?,
            target: UserVolumeCeiling,
        })
    }

    pub fn stream_volume_ceiling(&self) -> Result<Found<StreamVolumeCeiling>, LocatorError> {
        Ok(Found {
            rva: self.ceiling_load(VOLUME_STREAM_CEILING)?,
            target: StreamVolumeCeiling,
        })
    }

    pub fn gateway_send(&self) -> Result<Found<GatewaySend>, LocatorError> {
        Ok(Found {
            rva: self.rva(GATEWAY_SEND)?,
            target: GatewaySend,
        })
    }

    pub fn gateway_frame(&self) -> Result<Found<GatewayFrame>, LocatorError> {
        Ok(Found {
            rva: self.rva(GATEWAY_FRAME)?,
            target: GatewayFrame,
        })
    }

    pub fn client_state(&self) -> Result<Found<ClientState>, LocatorError> {
        Ok(Found {
            rva: self.rva(CLIENT_STATE)?,
            target: ClientState,
        })
    }
}

/// Every locator of the in-process transport, each resolved on its own.
#[derive(Debug)]
pub struct HookPlan {
    pub send: Result<Found<HttpSend>, LocatorError>,
    pub poll: Result<Found<HttpPoll>, LocatorError>,
    pub bytes_static: Result<Found<BytesStatic>, LocatorError>,
    pub allocator: Result<Found<Allocator>, LocatorError>,
    pub registry: Result<Found<GatewayRegistry>, LocatorError>,
    pub gateway_send: Result<Found<GatewaySend>, LocatorError>,
    pub inflate: Result<Found<GatewayInflate>, LocatorError>,
    /// Not needed for the transport to run: without it queued payloads are dropped.
    pub frame: Result<Found<GatewayFrame>, LocatorError>,
}

impl HookPlan {
    pub fn locate(scan: &Scan<'_>) -> HookPlan {
        HookPlan {
            send: scan.http_send(),
            poll: scan.http_poll(),
            bytes_static: scan.bytes_static(),
            allocator: scan.allocator(),
            registry: scan.gateway_registry(),
            gateway_send: scan.gateway_send(),
            inflate: scan.gateway_inflate(),
            frame: scan.gateway_frame(),
        }
    }

    /// A plan with every locator missing: no hook is on.
    pub fn none() -> HookPlan {
        HookPlan::locate(&Scan {
            sigs: &[],
            text: &[],
            text_rva: 0,
            image_len: 0,
        })
    }

    pub fn requests(&self) -> bool {
        self.send.is_ok()
    }

    pub fn responses(&self) -> bool {
        self.poll.is_ok()
    }

    /// A changed response body is a `Vec<u8>` the client frees: it needs the host's allocator.
    pub fn can_replace_body(&self) -> bool {
        self.allocator.is_ok()
    }

    /// A changed request body is a `Bytes` with the static vtable over memory PhoeCord keeps: it
    /// needs that vtable, not the allocator.
    pub fn can_replace_request_body(&self) -> bool {
        self.bytes_static.is_ok()
    }

    /// The incoming Gateway hook: the frame inflater (2.2.3), not the registry.
    pub fn gateway(&self) -> bool {
        self.inflate.is_ok()
    }

    /// One line per locator that failed, for the log.
    pub fn failures(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut note = |e: Option<&LocatorError>, off: &str| {
            if let Some(e) = e {
                out.push(format!("{e}; {off} stays off"));
            }
        };
        note(self.send.as_ref().err(), "REST request hook");
        note(self.poll.as_ref().err(), "REST response hook");
        note(self.bytes_static.as_ref().err(), "request body replacement");
        note(self.allocator.as_ref().err(), "response body replacement");
        note(self.inflate.as_ref().err(), "Gateway hook");
        note(self.gateway_send.as_ref().err(), "Gateway send hook");
        note(self.frame.as_ref().err(), "delivery of queued payloads");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aob::Pattern;

    fn sig(name: &str, aob: &str) -> Signature {
        Signature {
            name: name.into(),
            pattern: Pattern::parse(aob).unwrap(),
            rip: None,
        }
    }

    #[test]
    fn the_message_menu_needs_all_three_places() {
        let mut text = vec![0xCCu8; 0x100];
        text[0x10..0x14].copy_from_slice(&[0x55, 0x41, 0x56, 0x56]);
        text[0x40..0x44].copy_from_slice(&[0x48, 0x8B, 0x85, 0x01]);
        text[0x80..0x85].copy_from_slice(&[0xE8, 0x11, 0x22, 0x33, 0x44]);
        let all = [
            sig(MENU_PUSH, "55 41 56 56"),
            sig(MENU_AFTER_ANCHOR, "48 8B 85 01"),
            sig(MENU_BUILD_CALL, "E8 11 22 33 44"),
        ];
        let scan = |sigs: &[Signature]| {
            Scan {
                sigs,
                text: &text,
                text_rva: 0x1000,
                image_len: 0x10000,
            }
            .message_menu()
        };
        assert_eq!(
            scan(&all),
            Ok(Found {
                rva: 0x1010,
                target: MessageMenu {
                    after_anchor_rva: 0x1040,
                    build_call_rva: 0x1080,
                },
            })
        );
        assert_eq!(scan(&all[..2]), Err(LocatorError::Missing(MENU_BUILD_CALL)));
    }
}
