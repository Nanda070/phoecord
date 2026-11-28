//! The degradation plan (Phase 2, prep for 2.2.7 and 2.3.7): given what the locators found in this
//! client build, which network entry points run in-process and which UI features are on.
//!
//! The rules (`docs/ARCHITECTURE.md`, "Degradation"):
//!
//! - an unknown build: every entry point stays with the proxy and the UI layer is off;
//! - a failed locator costs only the feature that needs it, plus the entry points that cannot
//!   work without that one ([`requires`]).
//!
//! No OS code and no binary access: the caller says which locators resolved, so every case is
//! tested on every OS. `crate::transport` applies the plan.

use crate::found::HookPlan;

/// A network entry point of `phoecord_transport::Engine`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryPoint {
    /// `Engine::on_request`: a REST request.
    Request,
    /// `Engine::on_response`: a REST response; takes the method and route of its request.
    Response,
    /// `Engine::on_dispatch`: Gateway, server → client.
    Dispatch,
    /// `Engine::on_send`: Gateway, client → server.
    Send,
}

impl EntryPoint {
    /// In declaration order, which is also the index into [`Plan`].
    pub const ALL: [EntryPoint; 4] = [
        EntryPoint::Request,
        EntryPoint::Response,
        EntryPoint::Dispatch,
        EntryPoint::Send,
    ];

    pub fn name(self) -> &'static str {
        match self {
            EntryPoint::Request => "request",
            EntryPoint::Response => "response",
            EntryPoint::Dispatch => "dispatch",
            EntryPoint::Send => "send",
        }
    }
}

/// A feature of the UI layer, each with its own hook.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiFeature {
    /// 2.3.1: the palette hook.
    Theme,
    /// 2.3.3: the PhoeCord settings tab.
    SettingsTab,
    /// 2.3.4: deleted messages and edit history.
    DeletedMessages,
    /// 2.3.5: message context menu entries.
    ContextMenu,
    /// 2.3.6: local (optimistic) deletes.
    OptimisticDelete,
}

impl UiFeature {
    /// In declaration order, which is also the index into [`Plan`].
    pub const ALL: [UiFeature; 5] = [
        UiFeature::Theme,
        UiFeature::SettingsTab,
        UiFeature::DeletedMessages,
        UiFeature::ContextMenu,
        UiFeature::OptimisticDelete,
    ];

    pub fn name(self) -> &'static str {
        match self {
            UiFeature::Theme => "theme",
            UiFeature::SettingsTab => "settings tab",
            UiFeature::DeletedMessages => "deleted messages",
            UiFeature::ContextMenu => "context menu",
            UiFeature::OptimisticDelete => "optimistic delete",
        }
    }
}

/// What a locator is asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Net(EntryPoint),
    Ui(UiFeature),
}

/// `Ok` when everything the target needs was found, else why not (one line, for the log).
pub type Probe = Result<(), String>;

/// A locator's result as a probe, whichever locator framework it comes from. A target that needs
/// several locators joins them with `Result::and`, so the first failure is the one reported.
pub fn probe<T, E: std::fmt::Display>(found: &Result<T, E>) -> Probe {
    found.as_ref().map(|_| ()).map_err(ToString::to_string)
}

/// Why something is not running in-process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    /// No signatures or locators for this client build.
    UnknownBuild,
    /// The target's own locator failed.
    Locator(String),
    /// Its locator resolved, but this entry point it depends on is not in-process.
    Needs(EntryPoint),
}

impl std::fmt::Display for Reason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Reason::UnknownBuild => f.write_str("unknown client build"),
            Reason::Locator(why) => f.write_str(why),
            Reason::Needs(other) => write!(f, "needs the {} hook", other.name()),
        }
    }
}

/// Where an entry point's traffic is handled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    InProcess,
    Proxy(Reason),
}

/// The decision for one client build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    net: [Route; 4],
    /// `None`: the feature is on.
    ui: [Option<Reason>; 5],
}

impl Plan {
    /// The plan for a build nothing is known about: proxy only, no UI.
    pub fn unknown_build() -> Plan {
        Plan {
            net: EntryPoint::ALL.map(|_| Route::Proxy(Reason::UnknownBuild)),
            ui: UiFeature::ALL.map(|_| Some(Reason::UnknownBuild)),
        }
    }

    /// The plan for a known build; `probe` runs the locators and is asked once per target.
    pub fn resolve(probe: impl FnMut(Target) -> Probe) -> Plan {
        Plan::resolve_with(probe, requires)
    }

    /// The plan for what the hook locators found in this build. `HookPlan::send` is the REST
    /// transport's `send`, so it is the *request* entry point; sending on the gateway is
    /// `HookPlan::send_text`, or `HookPlan::gateway_send` when it is missing (2.2.4). No UI feature has a locator yet. `phoecord launch` builds
    /// the same plan from `Discord.exe` on disk ([`crate::check::transport_plan`]).
    pub fn from_hooks(hooks: &HookPlan) -> Plan {
        Plan::resolve(|target| match target {
            Target::Net(EntryPoint::Request) => probe(&hooks.send),
            Target::Net(EntryPoint::Response) => probe(&hooks.poll),
            Target::Net(EntryPoint::Dispatch) => probe(&hooks.inflate),
            Target::Net(EntryPoint::Send) => match &hooks.send_text {
                Ok(_) => Ok(()),
                Err(_) => probe(&hooks.gateway_send),
            },
            Target::Ui(_) => Err("no locator".into()),
        })
    }

    fn resolve_with(
        mut probe: impl FnMut(Target) -> Probe,
        requires: impl Fn(EntryPoint) -> &'static [EntryPoint],
    ) -> Plan {
        let mut net = EntryPoint::ALL.map(|p| match probe(Target::Net(p)) {
            Ok(()) => Route::InProcess,
            Err(why) => Route::Proxy(Reason::Locator(why)),
        });
        // Demote until nothing changes: a demotion can take further points with it, and two
        // points that require each other stay in-process only together.
        loop {
            let lost = EntryPoint::ALL.into_iter().find_map(|p| {
                if net[p as usize] != Route::InProcess {
                    return None;
                }
                requires(p)
                    .iter()
                    .find(|&&need| net[need as usize] != Route::InProcess)
                    .map(|&need| (p, need))
            });
            let Some((point, need)) = lost else { break };
            net[point as usize] = Route::Proxy(Reason::Needs(need));
        }
        let ui = UiFeature::ALL.map(|f| probe(Target::Ui(f)).err().map(Reason::Locator));
        Plan { net, ui }
    }

    pub fn route(&self, point: EntryPoint) -> &Route {
        &self.net[point as usize]
    }

    /// Why a UI feature is off; `None` when it is on.
    pub fn ui_off(&self, feature: UiFeature) -> Option<&Reason> {
        self.ui[feature as usize].as_ref()
    }

    /// Every entry point runs in-process: the condition for a Phoenix launch without the proxy
    /// (2.2.8, ruling 2026-11-26: the transport is all or nothing per launch).
    pub fn all_in_process(&self) -> bool {
        self.net.iter().all(|r| *r == Route::InProcess)
    }

    /// One line per entry point that is not in-process (the network half of [`Plan::report`]).
    pub fn net_report(&self) -> Vec<String> {
        EntryPoint::ALL
            .into_iter()
            .filter_map(|p| match self.route(p) {
                Route::InProcess => None,
                Route::Proxy(why) => Some(format!("{}: {why}", p.name())),
            })
            .collect()
    }

    /// No entry point runs in-process.
    pub fn proxy_only(&self) -> bool {
        self.net.iter().all(|r| *r != Route::InProcess)
    }

    /// One line per target that is not running in-process, for `loader.log`.
    pub fn report(&self) -> Vec<String> {
        let net = EntryPoint::ALL
            .into_iter()
            .filter_map(|p| match self.route(p) {
                Route::InProcess => None,
                Route::Proxy(why) => Some(format!("{}: through the proxy ({why})", p.name())),
            });
        let ui = UiFeature::ALL.into_iter().filter_map(|f| {
            self.ui_off(f)
                .map(|why| format!("{}: off ({why})", f.name()))
        });
        net.chain(ui).collect()
    }
}

/// The entry points that must also be in-process for `point` to be.
///
/// An entry point is only moved off the proxy when everything listed here moves with it. Only what
/// cannot work alone is listed, so that one failed locator costs as little as possible.
///
/// - `Response` needs `Request`: `on_response` takes the method and route of the request it
///   answers, which only the request side knows.
/// - `Send` needs `Dispatch`: both are directions of one gateway connection, which the proxy cannot
///   take over one direction at a time, and what `on_send` queues for the client
///   (`SendResult::locals`) is delivered through the dispatch hook.
/// - `Dispatch` does not need `Send`: Phoenix may have no outgoing send function (spec, open
///   question 3), and dispatch stays in-process then. What plugins queue for the server
///   (`DispatchResult::sends`, `Effects::frames`) has no hook to leave through in that case; the
///   transport (2.2.7) has to deal with it.
/// - `Request` needs nothing: REST keeps working in-process when the gateway locators fail.
fn requires(point: EntryPoint) -> &'static [EntryPoint] {
    match point {
        EntryPoint::Request | EntryPoint::Dispatch => &[],
        EntryPoint::Response => &[EntryPoint::Request],
        EntryPoint::Send => &[EntryPoint::Dispatch],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locators::{Found, Located, LocatorError};

    /// Fake locators: everything resolves except `missing`.
    fn without(missing: &'static [Target]) -> impl FnMut(Target) -> Probe {
        move |t| {
            if missing.contains(&t) {
                Err("not found".into())
            } else {
                Ok(())
            }
        }
    }

    fn independent(_: EntryPoint) -> &'static [EntryPoint] {
        &[]
    }

    #[test]
    fn unknown_build_is_proxy_only_without_ui() {
        let plan = Plan::unknown_build();
        assert!(plan.proxy_only());
        for p in EntryPoint::ALL {
            assert_eq!(plan.route(p), &Route::Proxy(Reason::UnknownBuild));
        }
        for f in UiFeature::ALL {
            assert_eq!(plan.ui_off(f), Some(&Reason::UnknownBuild));
        }
        assert_eq!(plan.report().len(), 9);
    }

    #[test]
    fn everything_found_runs_in_process() {
        let plan = Plan::resolve(without(&[]));
        for p in EntryPoint::ALL {
            assert_eq!(plan.route(p), &Route::InProcess);
        }
        assert!(UiFeature::ALL.iter().all(|&f| plan.ui_off(f).is_none()));
        assert!(plan.report().is_empty());
    }

    #[test]
    fn a_missing_ui_locator_costs_only_that_feature() {
        let plan = Plan::resolve(without(&[Target::Ui(UiFeature::Theme)]));
        assert_eq!(
            plan.ui_off(UiFeature::Theme),
            Some(&Reason::Locator("not found".into()))
        );
        assert!(!plan.proxy_only());
        assert_eq!(plan.report(), ["theme: off (not found)"]);
    }

    #[test]
    fn no_network_locator_is_proxy_only_and_keeps_the_ui() {
        let plan = Plan::resolve(without(&[
            Target::Net(EntryPoint::Request),
            Target::Net(EntryPoint::Response),
            Target::Net(EntryPoint::Dispatch),
            Target::Net(EntryPoint::Send),
        ]));
        assert!(plan.proxy_only());
        assert!(UiFeature::ALL.iter().all(|&f| plan.ui_off(f).is_none()));
    }

    #[test]
    fn all_in_process_needs_every_entry_point() {
        assert!(Plan::resolve(without(&[])).all_in_process());
        // A missing UI locator does not matter for the transport.
        assert!(Plan::resolve(without(&[Target::Ui(UiFeature::Theme)])).all_in_process());
        // Dispatch stays in-process without a send function, but the launch does not.
        let no_send = Plan::resolve(without(&[Target::Net(EntryPoint::Send)]));
        assert!(!no_send.all_in_process() && !no_send.proxy_only());
        assert_eq!(no_send.net_report(), ["send: not found"]);
        assert!(!Plan::unknown_build().all_in_process());
    }

    #[test]
    fn todays_hook_locators_never_give_a_whole_launch() {
        // Every locator of `HookPlan` resolves, but the gateway send has none yet (2.2.4).
        use crate::found::{Found, GatewayInflate, HookPlan, HttpPoll, HttpSend};
        let mut hooks = HookPlan::none();
        hooks.send = Ok(Found {
            rva: 1,
            target: HttpSend,
        });
        hooks.poll = Ok(Found {
            rva: 2,
            target: HttpPoll,
        });
        hooks.inflate = Ok(Found {
            rva: 3,
            target: GatewayInflate,
        });
        let plan = Plan::from_hooks(&hooks);
        assert_eq!(plan.route(EntryPoint::Request), &Route::InProcess);
        assert!(!plan.all_in_process());
    }

    #[test]
    fn a_locator_result_becomes_a_probe() {
        let found: Located<u32> = Ok(Found::new(0x1000, "mock"));
        assert_eq!(probe(&found), Ok(()));
        let missing: Located<u32> = Err(LocatorError::Missing {
            what: "send".into(),
        });
        assert_eq!(probe(&missing), Err("send: not found".into()));
    }

    #[test]
    fn each_locator_is_asked_once() {
        let mut asked = Vec::new();
        Plan::resolve(|t| {
            asked.push(t);
            Ok(())
        });
        assert_eq!(asked.len(), EntryPoint::ALL.len() + UiFeature::ALL.len());
        for (i, t) in asked.iter().enumerate() {
            assert!(!asked[..i].contains(t), "{t:?} asked twice");
        }
    }

    #[test]
    fn a_missing_gateway_hook_keeps_rest_in_process() {
        let plan = Plan::resolve(without(&[Target::Net(EntryPoint::Dispatch)]));
        assert_eq!(plan.route(EntryPoint::Request), &Route::InProcess);
        assert_eq!(plan.route(EntryPoint::Response), &Route::InProcess);
        assert_eq!(
            plan.route(EntryPoint::Send),
            &Route::Proxy(Reason::Needs(EntryPoint::Dispatch))
        );
        assert_eq!(
            plan.report(),
            [
                "dispatch: through the proxy (not found)",
                "send: through the proxy (needs the dispatch hook)",
            ]
        );
    }

    #[test]
    fn without_a_send_function_dispatch_stays_in_process() {
        let plan = Plan::resolve(without(&[Target::Net(EntryPoint::Send)]));
        assert_eq!(plan.route(EntryPoint::Dispatch), &Route::InProcess);
        assert!(!plan.proxy_only());
    }

    #[test]
    fn a_response_cannot_run_without_its_request() {
        let plan = Plan::resolve(without(&[Target::Net(EntryPoint::Request)]));
        assert_eq!(
            plan.route(EntryPoint::Response),
            &Route::Proxy(Reason::Needs(EntryPoint::Request))
        );
        assert_eq!(plan.route(EntryPoint::Dispatch), &Route::InProcess);
    }

    #[test]
    fn independent_points_fall_back_one_by_one() {
        let plan = Plan::resolve_with(without(&[Target::Net(EntryPoint::Dispatch)]), independent);
        assert_eq!(
            plan.route(EntryPoint::Dispatch),
            &Route::Proxy(Reason::Locator("not found".into()))
        );
        for p in [EntryPoint::Request, EntryPoint::Response, EntryPoint::Send] {
            assert_eq!(plan.route(p), &Route::InProcess);
        }
    }

    #[test]
    fn a_dependency_chain_falls_back_together() {
        fn chain(p: EntryPoint) -> &'static [EntryPoint] {
            match p {
                EntryPoint::Request => &[EntryPoint::Response],
                EntryPoint::Response => &[EntryPoint::Send],
                _ => &[],
            }
        }
        let plan = Plan::resolve_with(without(&[Target::Net(EntryPoint::Send)]), chain);
        assert_eq!(
            plan.route(EntryPoint::Response),
            &Route::Proxy(Reason::Needs(EntryPoint::Send))
        );
        assert_eq!(
            plan.route(EntryPoint::Request),
            &Route::Proxy(Reason::Needs(EntryPoint::Response))
        );
        assert_eq!(plan.route(EntryPoint::Dispatch), &Route::InProcess);
    }

    #[test]
    fn mutual_dependencies_move_only_together() {
        fn pair(p: EntryPoint) -> &'static [EntryPoint] {
            match p {
                EntryPoint::Dispatch => &[EntryPoint::Send],
                EntryPoint::Send => &[EntryPoint::Dispatch],
                _ => &[],
            }
        }
        let both = Plan::resolve_with(without(&[]), pair);
        assert_eq!(both.route(EntryPoint::Dispatch), &Route::InProcess);
        assert_eq!(both.route(EntryPoint::Send), &Route::InProcess);
        let one = Plan::resolve_with(without(&[Target::Net(EntryPoint::Dispatch)]), pair);
        assert_eq!(
            one.route(EntryPoint::Send),
            &Route::Proxy(Reason::Needs(EntryPoint::Dispatch))
        );
    }

    #[test]
    fn in_process_only_when_every_entry_point_is() {
        assert!(Plan::resolve(without(&[])).all_in_process());
        // A missing UI locator does not change the transport.
        assert!(Plan::resolve(without(&[Target::Ui(UiFeature::Theme)])).all_in_process());
        assert!(!Plan::unknown_build().all_in_process());
        for p in EntryPoint::ALL {
            let plan = Plan::resolve(without(match p {
                EntryPoint::Request => &[Target::Net(EntryPoint::Request)],
                EntryPoint::Response => &[Target::Net(EntryPoint::Response)],
                EntryPoint::Dispatch => &[Target::Net(EntryPoint::Dispatch)],
                EntryPoint::Send => &[Target::Net(EntryPoint::Send)],
            }));
            assert!(!plan.all_in_process(), "{}", p.name());
        }
    }
}
