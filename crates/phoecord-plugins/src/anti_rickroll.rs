//! AntiRickroll: find rickroll links in messages.
//!
//! Ported from the owner's userplugin `AntiRickroll`
//! (https://github.com/Nanda070/vencord-plugins/tree/main/plugins/AntiRickroll, MIT, Copyright (c)
//! 2024 ryanamay), modified for PhoeCord 2026-11-25. The MIT notice, as required:
//!
//! > MIT License. Copyright (c) 2024 ryanamay. Permission is hereby granted, free of charge, to any
//! > person obtaining a copy of this software and associated documentation files (the "Software"),
//! > to deal in the Software without restriction, including without limitation the rights to use,
//! > copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and
//! > to permit persons to whom the Software is furnished to do so, subject to the following
//! > conditions: The above copyright notice and this permission notice shall be included in all
//! > copies or substantial portions of the Software. THE SOFTWARE IS PROVIDED "AS IS", WITHOUT
//! > WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
//! > MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
//! > AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN
//! > AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
//! > SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
//!
//! The script puts a red "this link is a known rickroll" line under every message that has such a
//! link. Here the same check runs on every `MESSAGE_CREATE`, `MESSAGE_UPDATE` and page of channel
//! history (`GET /channels/{id}/messages`), and every message that has one is remembered (newest
//! 200, until it is deleted); `/phoecord rickrolls` lists them. Traffic is never changed and
//! nothing leaves the machine: the check is purely local.
//!
//! Switching it on: `[plugins.anti_rickroll] enabled = true`, or `/phoecord toggle anti_rickroll`.
//!
//! Matching rules, as in the script: links come from `[text](url)`, `<url>` and bare `http(s)://`
//! text in `content`; for each link in order the first match wins. A link is a custom match
//! when the setting `customLinks` (comma separated, trimmed) has an entry contained in the link's
//! normalised `href`, or, for `youtube.com` and `youtu.be`, when its video id is in
//! `customVideoIds`. Otherwise it is a known rickroll when the host (first `www.` removed) is
//! `youtube.com` or `youtu.be` with a video id of the built-in list (or of `customVideoIds`;
//! `v` or `V` for youtube.com, the path for youtu.be), or the host is in the built-in host list.
//!
//! Not ported: the warning line under the message (Phase 2). The URL is parsed by hand to follow
//! the WHATWG parser for `http`/`https`; internationalised host names are compared as typed (not
//! converted to punycode) and numeric IPv4 shorthands are not normalised.

use crate::commands::{Args, FnCmd, Subcommand};
use crate::state::Shared;
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};
use regex::Regex;
use std::sync::OnceLock;

const PLUGIN_ID: &str = "anti_rickroll";
/// Messages with a rickroll link kept for the command, at most.
const HITS_CAP: usize = 200;

/// The script's knownHosts.ts, verbatim (entries with a path or a scheme never equal a bare
/// hostname, as in the script).
const KNOWN_HOSTS: &[&str] = &[
    "rickroll.link",
    "rickrolled.fr",
    "dub.sh/MetropolisOffline",
    "my-cool-app.com",
    "1227.com",
    "203.24.182.170",
    "74.125.95.104/search?q=cache:YLZLcPVmjrsJ:smouch.net/lol/+rick+roll&hl=en&ct=clnk&cd=5&gl=us",
    "8chan.henriwatson.com/b",
    "adurah.com/img/hp7_spoilers.jpg",
    "ali3ns.net",
    "almightypickle.deviantart.com/art/a-very-stupid-flash-movie-83440015",
    "apoke.com",
    "asongbyagayguy.ytmnd.com",
    "bit.ly/1628nB",
    "bringvictory.com",
    "choose.yudia.net",
    "classpc.nl",
    "collegehumor.com/video:1809841",
    "comicwonder.com/joke/8a46a9b4a",
    "crappypc.com",
    "ebaumsworld.com/video/watch/411687",
    "ecritters.biz/sr",
    "epicwinrar.com",
    "evanball.com",
    "finalclan.net",
    "freeppcsecrets.com",
    "fuckdreamhost.com",
    "furryartpile.com",
    "i65games.tk",
    "internetisseriousbusiness.com",
    "irc.infinitynet.info",
    "johncow.com/moo.html",
    "just-different.info/secret",
    "keiraknightley.ytmnd.com",
    "krazykustoms.co.cc/pages",
    "krazykustoms.co.cc/phpBB3/index.php",
    "liner.org",
    "members.tele2.nl/class-pc",
    "mxweas.com/docs/jailbreakservhack",
    "newroupdates.tk",
    "niya.cc/flash/rick",
    "noelurl.easyasweb.eu/u0Rx744fi",
    "palmsout.net/music/remixsunday/104/Never%20Gonna%20Give%20You%20Up%20%28Solly%20Remix%29.mp3",
    "paulsmom.prohosts.org",
    "pottermisleading.ytmnd.com",
    "prankdialer.com/rickroll.php",
    "pspemporium.webs.com/rickdance.html",
    "punahou.com",
    "rasher.dk/r",
    "rasher.dk/r/linode-dns",
    "ravenstorm.byethost15.com",
    "raygoldmodels.com",
    "reichroll.com",
    "reichrolled.com",
    "rick-rolld.com",
    "rickroll.net",
    "rickrolling.com/lol",
    "rr.rezbit.com",
    "rr.rezbit.com/blog/021107/rick-astley-speaks-about-rickroll.html",
    "rurl.org/2vq",
    "sirpnut.info/sorry.jpg",
    "smouch.net/lol",
    "sprigler.com/steven",
    "stucknut.com/locker/files/jessica.gif",
    "t1ny.us/7wp5i",
    "technocolbert.co.nr",
    "techsmartly.net",
    "thekickback.com/rickroll/rickroll.php",
    "tinyurl.com/interfont",
    "tinyurl.com/244u3n",
    "tinyurl.com/2nmscj",
    "tinyurl.com/2tcnbl",
    "tinyurl.com/2w4apm",
    "tinyurl.com/37ws8e",
    "tinyurl.com/4xur8r",
    "tinyurl.com/5a3jqf",
    "tinyurl.com/5sb29l",
    "tinyurl.com/CELEBRITY-XXX-VIDS",
    "tinyurl.com/KIM-KARDASHIAN-SEX-TAPE",
    "tinyurl.com/britneyspearsXXX",
    "tinyurl.com/rickrolll",
    "tinyurl.com/ynupj4",
    "tinyurl.com/yowxeq",
    "tobi-x.com/kate_moss_nude",
    "todaysbigthing.com/2008/04/07",
    "todo.is",
    "url.uppix.net/NFWGJ",
    "video.hexxeh.net",
    "video.yahoo.com/watch/1313289/4562193",
    "video.yahoo.com/watch/2013707/v2138974",
    "video.yahoo.com/watch/804885/3375801",
    "vids.myspace.com/index.cfm?fuseaction=vids.individual&videoid=25992858",
    "warlocksnerfed.ytmnd.com",
    "ww.rickroll.net",
    "you-host.info",
    "yougotrickrolled.com",
    "zeroboard.com/16979949#0",
    "zideo.nl/index.php?option=com_podfeed&zideo=6c4971596d513d3d&playzideo=6c344f596d31593d&Itemid=",
    "https://only-fans.uk/",
    "https://r.mtdv.me/",
    "https://rroll.derlin.ch",
    "https://spotify-rickroll.vercel.app/",
    "https://www.temporary-url.com/",
    "https://www.latlmes.com/breaking/news-rickroll-generator-1",
    "https://streamable.com/lf027o",
    "https://prank-website.rafidhoda.repl.co/",
];

/// The script's knownVideoIDs.ts, verbatim (the comments are dropped).
const KNOWN_VIDEO_IDS: &[&str] = &[
    "dQw4w9WgXcQ",
    "oHg5SJYRHA0",
    "p7YXXieghto",
    "E4WlUXrJgy4",
    "6_b7RDuLwcI",
    "G8iEMVr7GFg",
    "AyOqGRjVtls",
    "6mhmcwmgWbA",
    "SpZ2FsEfwP4",
    "H01BwSD9eyQ",
    "nrsnN23tmUA",
    "8mkofgRW1II",
    "rAx5LIul1N8",
    "sO4wVSA9UPs",
    "rrs0B_LM898",
    "doEqUhFiQS4",
    "epyRUp0BhrA",
    "uK5WDo_3s7s",
    "wzSVOcgKq04",
    "7B--1KArxow",
    "rbsPu1z3ugQ",
    "ptw2FLKXDQE",
    "E50L-JYWm3w",
    "8leAAwMIigI",
    "ByqFY-Boq5Y",
    "E4ihJMQUmUQ",
    "cjBHXvBYw5s",
    "xaazUgEKuVA",
    "TzXXHVhGXTQ",
    "Uj1ykZWtPYI",
    "EE-xtCF3T94",
    "V-_O7nl0Ii0",
    "cqF6M25kqq4",
    "0SoNH07Slj0",
    "xfr64zoBTAQ",
    "j5a0jTc9S10",
    "dPmZqsQNzGA",
    "nHRbZW097Uk",
    "BjDebmqFRuc",
    "Gc2u6AFImn8",
    "8VFzHYtOARw",
    "cSAp9sBzPbc",
    "Dx5i1t0mN78",
    "Oo0twK2ZbLU",
    "cvh0nX08nRw",
    "lXMskKTw3Bc",
    "7z_1E8VGJOw",
    "VgojnNgmgVs",
    "5wOXc03RwVA",
    "2xx_2XNxxfA",
    "lpiB2wMc49g",
    "H8ZH_mkfPUY",
    "Svj1bZz2mXw",
    "iik25wqIuFo",
    "hvL1339luv0",
    "N9w1lCZfaWI",
    "ivV-kssG3NI",
    "0SoNH07Slj0",
    "2xx_2XNxxfA",
    "5wOXc03RwVA",
    "6_b7RDuLwcI",
    "6mhmcwmgWbA",
    "7B--1KArxow",
    "7z_1E8VGJOw",
    "8VFzHYtOARw",
    "8leAAwMIigI",
    "8mkofgRW1II",
    "AyOqGRjVtls",
    "BjDebmqFRuc",
    "ByqFY-Boq5Y",
    "Dx5i1t0mN78",
    "E4WlUXrJgy4",
    "E4ihJMQUmUQ",
    "E50L-JYWm3w",
    "EE-xtCF3T94",
    "G8iEMVr7GFg",
    "Gc2u6AFImn8",
    "H01BwSD9eyQ",
    "H8ZH_mkfPUY",
    "N9w1lCZfaWI",
    "Oo0twK2ZbLU",
    "SpZ2FsEfwP4",
    "Svj1bZz2mXw",
    "TzXXHVhGXTQ",
    "Uj1ykZWtPYI",
    "V-_O7nl0Ii0",
    "VgojnNgmgVs",
    "cSAp9sBzPbc",
    "cjBHXvBYw5s",
    "cqF6M25kqq4",
    "cvh0nX08nRw",
    "dPmZqsQNzGA",
    "dQw4w9WgXcQ",
    "doEqUhFiQS4",
    "epyRUp0BhrA",
    "hvL1339luv0",
    "iik25wqIuFo",
    "ivV-kssG3NI",
    "j5a0jTc9S10",
    "lXMskKTw3Bc",
    "lpiB2wMc49g",
    "nHRbZW097Uk",
    "nrsnN23tmUA",
    "oHg5SJYRHA0",
    "p7YXXieghto",
    "ptw2FLKXDQE",
    "rAx5LIul1N8",
    "rbsPu1z3ugQ",
    "rrs0B_LM898",
    "sO4wVSA9UPs",
    "uK5WDo_3s7s",
    "wzSVOcgKq04",
    "xaazUgEKuVA",
    "xfr64zoBTAQ",
    "AuKR2fQbMBk",
    "xvFZjo5PgG0",
    "BBJa32lCaaY",
    "QB7ACr7pUuE",
    "pKskW7wJ0v0",
    "j7gKwxRe7MQ",
    "LWErcuHm_C4",
    "a6pbjksYUHY",
    "ll-mQPDCn-U",
    "dRV6NaciZVk",
    "MCjlo7PtXMQ",
    "mrThFRR3n8A",
    "HnfkEVtetuE",
    "j8PxqgliIno",
    "AACOcpA8i-U",
    "bIwVIx5pp88",
    "OyHQlZeyEr4",
    "8ybW48rKBME",
    "ZK1pNGmNBEc",
    "cSQTZoZPJzs",
    "6lhwMJuBIEw",
    "IR_rbAer7kY",
    "5iqIug1Rox8",
    "EkfDmzprwrw",
    "EjahYsvSTts",
    "8NBdab8iors",
    "UHYLsSLSUCw",
    "QMIaYRaUC5E",
    "JDuaYGd0zUI",
    "dy4bWkj4_RA",
    "55C1L77QrGk",
    "TSi6_A74lCw",
    "w-kk-bU_QVw",
    "uPbl8o3bjY0",
    "dQw4w9WgXcQ",
    "hVzINKRbekY",
    "wzSVOcgKq04",
    "xaazUgEKuVA",
    "xfr64zoBTAQ",
    "xm3YgoEiEDc",
    "xvFZjo5PgG0",
    "zL19uMsnpSU",
    "-HRRGn6tNd4",
    "-umtVthRGJI",
    "1V_aE_Xdde8",
    "3HrSN7176XI",
    "3KANI2dpXLw",
    "4TJB9FQo45E",
    "4ynGOr9vmyc",
    "5F5nc1bAaKw",
    "5kQKh2tdisQ",
    "5uZr3JWYdy8",
    "65I0HNvTDH4",
    "67KOFEEOhkI",
    "6SYVdI7Llrg",
    "8Set3JpJJ4w",
    "8aJjMOy-Ops",
    "A1sgzEDUG-o",
    "ABUhOJxZQmg",
    "AP12uZvfvag",
    "AS35zlAdaSQ",
    "CZoJt0Sbqrs",
    "DpPhnECPe2I",
    "EEbzptEFsKk",
    "FGEUClII8x0",
    "G_vas-7a7is",
    "HiaBjpzLgQI",
    "I6_0tpqg3ZE",
    "IHKAgwIxUAY",
    "IVvl_R2jYMo",
    "IpjGmx2v6bM",
    "J3VnZMoh7sk",
    "Kmt4wrn1MTk",
    "LeSnAn-Sc0g",
    "LqXTU8YAGzo",
    "MJCH_wT9TaU",
    "MoUPdJrjkCM",
    "NZ-AAD7Ci_c",
    "PIMrL4qXtJ0",
    "PXrHCBoEj7g",
    "QumbExFAj-U",
    "R5P1_U7LZX8",
    "RSsJ19sy3JI",
    "RgIDuaxiT0w",
    "RzoZGNsJ71w",
    "SGi7qi_y0Jw",
    "UDTGvgE5eJw",
    "VBY4TV5qK-4",
    "VVjUWKSZSsc",
    "WpEVccrkYQ0",
    "X-j828DqqnY",
    "XfTUDW93z6E",
    "Xsvi9uNrDSI",
    "YWn54TjfBkk",
    "Yu_moia-oVI",
    "ZIQZHqNQODo",
    "ZOU8GIRUd_g",
    "Zc2tpMgz6MI",
    "_0719DxMOUY",
    "_FyXC-SsAPE",
    "aSzhxllE0RM",
    "atiNprQmjks",
    "b1WWpKEPdT4",
    "b43GgNbz9fg",
    "cjeogv9VUAE",
    "cxwxBheZniM",
    "dS9DO6kx-Ek",
    "eKDcl0V6o6k",
    "eLiXPfl8EPY",
    "f2b1D5w82yU",
    "f8FHqEIzSlE",
    "fMzkMpMraFY",
    "fmxGLQd3J0U",
    "gLVtavM962w",
    "gdpijMRhoT8",
    "gvUiLtwlEl8",
    "hq05IRf0dBQ",
    "hyX_krrTBZ0",
    "iJbwc4wm5Y0",
    "lAQIiNTH46I",
    "lfO4Z5WEUuE",
    "lfao5IToml4",
    "lp6nr-rD_g8",
    "lzSjyzqfegI",
    "oHg5SJYRHA0",
    "piLTpv1eKdU",
    "q6NwqFlctZY",
    "r8tXjJL3xcM",
    "rfp7FbsnsbU",
    "tazYxtJcwCc",
    "thNTPin2RBY",
    "uYMIMPVK1vU",
    "ub_VBGajh-s",
    "uwnuL5Fy5g8",
    "uxIsiTo4VJo",
    "v7cyQ-dVaAM",
    "veFrQTKQy7A",
    "vf79MCuQ8jM",
    "vp8fkB0uwd0",
    "vvs7cXmVwN8",
    "xAp3HqpE7V8",
    "xm_EMOdpDhc",
    "xpupxRzumYs",
    "y5Ja-E529sU",
    "yXGqsnkLg0A",
    "ywoqy9PBN-0",
    "yxnWl63Avo4",
    "z-HWXfRKkJU",
    "z2kThcO6ig8",
    "zGm0nGF_y2E",
    "BUykFA7FCo4",
    "0SoNH07Slj0",
    "2mun7xy12H0",
    "34Ig3X59_qA",
    "5wOXc03RwVA",
    "6-HUgzYPm9g",
    "6_b7RDuLwcI",
    "6mhmcwmgWbA",
    "7B--1KArxow",
    "7z_1E8VGJOw",
    "828hEBkvA5g",
    "8VFzHYtOARw",
    "8leAAwMIigI",
    "8mkofgRW1II",
    "AyOqGRjVtls",
    "BjDebmqFRuc",
    "ByqFY-Boq5Y",
    "DLzxrzFCyOs",
    "Dx5i1t0mN78",
    "E4ihJMQUmUQ",
    "EE-xtCF3T94",
    "G8iEMVr7GFg",
    "Gc2u6AFImn8",
    "H01BwSD9eyQ",
    "IdkCEioCp24",
    "Oo0twK2ZbLU",
    "Rtqkxkt7Hyg",
    "SpZ2FsEfwP4",
    "TOcAAaZ1HhU",
    "TzXXHVhGXTQ",
    "Uj1ykZWtPYI",
    "UmOi40tnUcM",
    "V-_O7nl0Ii0",
    "VbUuB1aN2DA",
    "VgojnNgmgVs",
    "YJWSAJjkTsI",
    "ZzUsKizhb8o",
    "a3Z7zEc7AXQ",
    "cSAp9sBzPbc",
    "cqF6M25kqq4",
    "cvh0nX08nRw",
    "dPmZqsQNzGA",
    "dQw4w9WgXcQ",
    "doEqUhFiQS4",
    "epyRUp0BhrA",
    "gvGyS5j9aFY",
    "hLZW4U86cL4",
    "iik25wqIuFo",
    "j5a0jTc9S10",
    "lXMskKTw3Bc",
    "nHRbZW097Uk",
    "nrsnN23tmUA",
    "oHg5SJYRHA0",
    "oT3mCybbhf0",
    "ptw2FLKXDQE",
    "r7VYFiPoZk4",
    "r8tXjJL3xcM",
    "rAx5LIul1N8",
    "rbsPu1z3ugQ",
    "rrs0B_LM898",
    "sO4wVSA9UPs",
    "tzYPr9w22VU",
    "uK5WDo_3s7s",
    "ub82Xb1C8os",
    "wpV-gGA4PSk",
    "wuggc60TVzU",
    "cSAp9sBzPbc",
    "8ybW48rKBME",
    "Tt7bzxurJ1I",
    "gidHrKbTpog",
    "vcq30ywIKlc",
    "y6120QOlsfU",
    "QhuZIEkKkXA",
    "rW7hXs-81hM",
    "Oo0twK2ZbLU",
    "0SoNH07Slj0",
    "LLFhKaqnWwk",
    "SpZ2FsEfwP4",
    "SO4c1NlGCpY",
    "MCjlo7PtXMQ",
    "TCUPeAlpDVE",
    "5wOXc03RwVA",
    "oJl25bABYac",
    "a6pbjksYUHY",
    "Dx5i1t0mN78",
    "Uj1ykZWtPYI",
    "E50L-JYWm3w",
    "BjDebmqFRuc",
    "PibwQR9nRus",
    "rrs0B_LM898",
    "cH1NLToRz_o",
    "j5a0jTc9S10",
    "p7YXXieghto",
    "qxeokP0n6V0",
    "GHMjD0Lp5DY",
    "bIwVIx5pp88",
    "7z_1E8VGJOw",
    "QB7ACr7pUuE",
    "LjQZaD9EEJ0",
    "cvh0nX08nRw",
    "fZi4JxbTwPo",
    "2ocykBzWDiM",
    "H01BwSD9eyQ",
    "PzqQSOaCcnw",
    "lXMskKTw3Bc",
    "JzjldPdpT7Q",
    "7B--1KArxow",
    "lHyeAtiiL18",
    "s0akG-V_Y30",
    "HV8IxKtAlrA",
    "6_b7RDuLwcI",
    "rbsPu1z3ugQ",
    "8O_ifyIIrN4",
    "xvFZjo5PgG0",
    "N3MKlU5_Gs0",
    "AyOqGRjVtls",
    "bCh5IlXTckA",
    "pSxJqmMNU-A",
    "ByqFY-Boq5Y",
    "G8iEMVr7GFg",
    "s99hC5WUwjo",
    "uK5WDo_3s7s",
    "TzXXHVhGXTQ",
    "OYPG7j8rKZY",
    "Kiz3PM387FI",
    "2l51wyMM9hs",
    "ptw2FLKXDQE",
    "wzSVOcgKq04",
    "rAx5LIul1N8",
    "ll-mQPDCn-U",
    "hxCXEX2JOKE",
    "6RyOsIseJbk",
    "HUU3kOhjFfg",
    "3KANI2dpXLw",
    "-51AfyMqnpI",
    "6mhmcwmgWbA",
    "7lEodsmjeQU",
    "8leAAwMIigI",
    "xaazUgEKuVA",
    "dQw4w9WgXcQ",
    "nHRbZW097Uk",
    "vkbQmH5MPME",
    "6vBnqJhP7f8",
    "xfr64zoBTAQ",
    "EE-xtCF3T94",
    "zL19uMsnpSU",
    "mrThFRR3n8A",
    "E4ihJMQUmUQ",
    "kczWpSN6emg",
    "bkOVXdvijmQ",
    "cqF6M25kqq4",
    "cAN30xJp2Cs",
    "doEqUhFiQS4",
    "iiUIi5b9RlE",
    "H8ZH_mkfPUY",
    "HPk-VhRjNI8",
    "INbRpclIFsE",
    "ikFZLI4HLpQ",
    "dPmZqsQNzGA",
    "cjBHXvBYw5s",
    "SjTOn5scRmY",
    "JkcHqN6YswU",
    "IO9XlQrEt2Y",
    "oHg5SJYRHA0",
    "lpiB2wMc49g",
    "DIs7dtlMCP0",
    "gvGyS5j9aFY",
    "VgojnNgmgVs",
    "SsiHLiXBlcM",
    "Q53-mWHvxJo",
    "XfIIQYnBHrQ",
    "LWErcuHm_C4",
    "AACOcpA8i-U",
    "QdNEtVH9bZE",
    "dRV6NaciZVk",
    "ub82Xb1C8os",
    "ENYalQvZG4Q",
    "BXuxOeg6PiQ",
    "0Z0i-nimHkU",
    "Mdq6sQPKYqQ",
    "C5GGeuFPVVE",
    "Gc2u6AFImn8",
    "klqi_h9FElc",
    "Eg7XtrH0duI",
    "aaP6xIkMqzo",
    "xbXoWRs2C8M",
    "pKskW7wJ0v0",
    "HnfkEVtetuE",
    "9oxRDJjAQKI",
    "j7gKwxRe7MQ",
    "42OleX0HR4E",
    "nrsnN23tmUA",
    "iik25wqIuFo",
    "E458nb_pPmk",
    "06pBzAlItwI",
    "rTgj1HxmUbg",
    "epyRUp0BhrA",
    "rEtZzcxQ_pA",
    "Yb6dZ1IFlKc",
    "eBGIQ7ZuuiU",
    "soq9cLVa5Xo",
    "r8tXjJL3xcM",
    "XB3EQrj1maw",
    "RvBwypGUkPo",
    "d4N5LnsNpF0",
    "j8PxqgliIno",
    "th0pH9srXRY",
    "sO4wVSA9UPs",
    "8mkofgRW1II",
    "fMnIpIMuBJI",
    "gPkZS92WCIo",
    "_sWG4uhLiWo",
    "8VFzHYtOARw",
    "V-_O7nl0Ii0",
    "eBsIpefmXvE",
    "nW0P2hYBm9A",
    "MAM--whuMcU",
    "QKr_0DMYV5g",
    "VZrDxD0Za9I",
    "WSBK_NePew",
    "1kj837fWpo",
    "oAyaw1ohiWs",
    "iadFGiLRgFo",
    "A4hJPP0Zn9I",
    "r1JAZlyhKMc",
    "DdpBpn6txTk",
    "yS6LYj-y1HA",
    "WyltKn3pi9U",
    "hB7CDrVnNCs",
];

/// A parsed `http`/`https` URL, the parts the script reads.
struct Url {
    href: String,
    hostname: String,
    pathname: String,
    query: String,
}

fn pct_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push(h * 16 + l);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

fn hex(c: u8) -> Option<u8> {
    (c as char).to_digit(16).map(|d| d as u8)
}

/// Percent-encode bytes that are non-ASCII, control, or in `extra`.
fn encode(s: &str, extra: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b <= 0x20 || b >= 0x7f || extra.as_bytes().contains(&b) {
            out.push_str(&format!("%{b:02X}"));
        } else {
            out.push(b as char);
        }
    }
    out
}

/// `new URL(url)` for http and https; `None` where it throws.
fn parse_url(url: &str) -> Option<Url> {
    let (scheme, rest) = if let Some(r) = url.strip_prefix("https://") {
        ("https", r)
    } else {
        ("http", url.strip_prefix("http://")?)
    };
    let rest = rest.trim_start_matches(['/', '\\']);
    let auth_end = rest.find(['/', '\\', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(auth_end);
    let (userinfo, hostport) = match authority.rfind('@') {
        Some(i) => (Some(&authority[..i]), &authority[i + 1..]),
        None => (None, authority),
    };
    let (host_raw, port_raw) = if hostport.starts_with('[') {
        let end = hostport.find(']')?;
        let after = &hostport[end + 1..];
        match after.strip_prefix(':') {
            Some(p) => (&hostport[..=end], Some(p)),
            None if after.is_empty() => (&hostport[..=end], None),
            None => return None,
        }
    } else {
        match hostport.rfind(':') {
            Some(i) => (&hostport[..i], Some(&hostport[i + 1..])),
            None => (hostport, None),
        }
    };
    let port = match port_raw {
        None | Some("") => None,
        Some(p) => {
            if !p.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let n: u32 = p.parse().ok()?;
            if n > 65535 {
                return None;
            }
            let default = if scheme == "https" { 443 } else { 80 };
            (n != default).then_some(n)
        }
    };
    let host = String::from_utf8(pct_decode(host_raw)).ok()?.to_lowercase();
    if host.is_empty()
        || host.chars().any(|c| {
            c.is_control()
                || c.is_whitespace()
                || "#/:<>?@\\^|%".contains(c)
                || (c == '[' || c == ']') && !(host.starts_with('[') && host.ends_with(']'))
        })
    {
        return None;
    }

    let (before_frag, fragment) = match tail.split_once('#') {
        Some((a, f)) => (a, Some(f)),
        None => (tail, None),
    };
    let (path_raw, query) = match before_frag.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (before_frag, None),
    };
    // Path: backslashes are slashes, dot segments are resolved.
    let path_raw = path_raw.replace('\\', "/");
    let mut segs: Vec<&str> = Vec::new();
    let mut parts = path_raw.split('/').skip(1).peekable();
    while let Some(seg) = parts.next() {
        let last = parts.peek().is_none();
        match seg.to_ascii_lowercase().as_str() {
            "." | "%2e" => {
                if last {
                    segs.push("");
                }
            }
            ".." | ".%2e" | "%2e." | "%2e%2e" => {
                segs.pop();
                if last {
                    segs.push("");
                }
            }
            _ => segs.push(seg),
        }
    }
    let pathname = format!("/{}", encode(&segs.join("/"), "\"#<>?`{}"));
    let query = query.map(|q| encode(q, "\"#<>'"));
    let mut href = format!("{scheme}://");
    if let Some(u) = userinfo {
        href.push_str(&encode(u, "\"#<>?`{}/:;=@[\\]^|"));
        href.push('@');
    }
    href.push_str(&host);
    if let Some(p) = port {
        href.push_str(&format!(":{p}"));
    }
    href.push_str(&pathname);
    if let Some(q) = &query {
        href.push('?');
        href.push_str(q);
    }
    if let Some(f) = fragment {
        href.push('#');
        href.push_str(&encode(f, "\"<>`"));
    }
    Some(Url {
        href,
        hostname: host,
        pathname,
        query: query.unwrap_or_default(),
    })
}

fn form_decode(s: &str) -> String {
    String::from_utf8_lossy(&pct_decode(&s.replace('+', " "))).into_owned()
}

/// `parsedUrl.searchParams.get(name)`.
fn search_param(query: &str, name: &str) -> Option<String> {
    query.split('&').filter(|p| !p.is_empty()).find_map(|p| {
        let (k, v) = p.split_once('=').unwrap_or((p, ""));
        (form_decode(k) == name).then(|| form_decode(v))
    })
}

/// A comma separated setting: trimmed, empty entries dropped.
fn list_setting(host: &dyn Host, key: &str) -> Vec<String> {
    host.setting(key)
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

struct Rules {
    custom_links: Vec<String>,
    custom_ids: Vec<String>,
}

impl Rules {
    fn of(host: &dyn Host) -> Rules {
        Rules {
            custom_links: list_setting(host, "customLinks"),
            custom_ids: list_setting(host, "customVideoIds"),
        }
    }

    /// `isPotentialRickroll`.
    fn is_potential(&self, url: &str) -> bool {
        let Some(u) = parse_url(url) else {
            return false;
        };
        let hostname = u.hostname.replacen("www.", "", 1);
        if self
            .custom_links
            .iter()
            .any(|l| u.href.contains(l.as_str()))
        {
            return true;
        }
        if hostname == "youtube.com" || hostname == "youtu.be" {
            let id = if hostname == "youtube.com" {
                search_param(&u.query, "v")
                    .filter(|s| !s.is_empty())
                    .or_else(|| search_param(&u.query, "V"))
                    .unwrap_or_default()
            } else {
                u.pathname[1..].to_string()
            };
            if KNOWN_VIDEO_IDS.contains(&id.as_str()) || self.custom_ids.contains(&id) {
                return true;
            }
        }
        KNOWN_HOSTS.contains(&hostname.as_str())
    }

    /// `isCustomRickroll`.
    fn is_custom(&self, url: &str) -> bool {
        let Some(u) = parse_url(url) else {
            return false;
        };
        if self
            .custom_links
            .iter()
            .any(|l| u.href.contains(l.as_str()))
        {
            return true;
        }
        let hostname = u.hostname.replacen("www.", "", 1);
        if hostname == "youtube.com" || hostname == "youtu.be" {
            let id = if hostname == "youtube.com" {
                search_param(&u.query, "v").unwrap_or_default()
            } else {
                u.pathname[1..].to_string()
            };
            return self.custom_ids.contains(&id);
        }
        false
    }
}

/// `extractUrls`: markdown links, then bare links, then `<link>`s.
fn extract_urls(content: &str) -> Vec<String> {
    static RES: OnceLock<[Regex; 3]> = OnceLock::new();
    let [md, plain, masked] = RES.get_or_init(|| {
        [
            Regex::new(r"\[.*?\]\((<)?(https?://[^\s>]+)(>)?\)").expect("static regex"),
            Regex::new(r"https?://[^\s<]+").expect("static regex"),
            Regex::new(r"<(https?://[^\s>]+)>").expect("static regex"),
        ]
    });
    let mut urls: Vec<String> = md
        .captures_iter(content)
        .map(|c| c[2].to_string())
        .collect();
    urls.extend(plain.find_iter(content).map(|m| m.as_str().to_string()));
    urls.extend(masked.captures_iter(content).map(|c| c[1].to_string()));
    urls
}

/// What the warning accessory shows: the first link that is a rickroll, and whether it matched
/// the user's own filters. `RickrollWarningAccessory`.
fn check(rules: &Rules, content: &str) -> Option<(String, bool)> {
    for url in extract_urls(content) {
        let custom = rules.is_custom(&url);
        if rules.is_potential(&url) || custom {
            return Some((url, custom));
        }
    }
    None
}

#[derive(Clone, Debug, PartialEq)]
struct Hit {
    message: u64,
    channel: u64,
    url: String,
    custom: bool,
}

#[derive(Default)]
struct State {
    hits: Vec<Hit>,
}

impl State {
    /// One message object (`id`, `channel_id`, `content`). A message without `content` (a partial
    /// update) is left as it was.
    fn message(&mut self, rules: &Rules, m: &Val) {
        let (Some(id), Some(content)) = (m.id_at(&["id"]), m.get("content").and_then(Val::as_str))
        else {
            return;
        };
        let found = check(rules, content);
        self.hits.retain(|h| h.message != id);
        if let Some((url, custom)) = found {
            self.hits.push(Hit {
                message: id,
                channel: m.id_at(&["channel_id"]).unwrap_or(0),
                url,
                custom,
            });
            if self.hits.len() > HITS_CAP {
                self.hits.remove(0);
            }
        }
    }

    fn deleted(&mut self, d: &Val) {
        if let Some(id) = d.id_at(&["id"]) {
            self.hits.retain(|h| h.message != id);
        }
        for id in d.get("ids").map(Val::as_list).unwrap_or(&[]) {
            if let Some(id) = id.snowflake() {
                self.hits.retain(|h| h.message != id);
            }
        }
    }

    fn report(&self, host: &dyn Host) -> String {
        if self.hits.is_empty() {
            return host.t("anti_rickroll-none", &[]);
        }
        let mut out = format!(
            "**{}**",
            host.t(
                "anti_rickroll-header",
                &[("count", &self.hits.len().to_string())]
            )
        );
        for h in self.hits.iter().rev() {
            let key = if h.custom {
                "anti_rickroll-line-custom"
            } else {
                "anti_rickroll-line-known"
            };
            out.push('\n');
            out.push_str(&host.t(
                key,
                &[
                    ("channel", &format!("<#{}>", h.channel)),
                    ("message", &h.message.to_string()),
                    ("url", &h.url.replace('`', "'")),
                ],
            ));
        }
        out
    }
}

/// `GET /channels/{id}/messages`.
fn is_history(method: &str, route: &str) -> bool {
    let seg: Vec<&str> = route
        .split('?')
        .next()
        .unwrap_or("")
        .trim_matches('/')
        .split('/')
        .collect();
    method == "GET" && matches!(seg.as_slice(), ["channels", _, "messages"])
}

fn wanted(name: &str) -> bool {
    matches!(
        name,
        "MESSAGE_CREATE" | "MESSAGE_UPDATE" | "MESSAGE_DELETE" | "MESSAGE_DELETE_BULK"
    )
}

pub struct AntiRickroll {
    manifest: Manifest,
    state: Shared<State>,
}

impl AntiRickroll {
    pub fn new() -> AntiRickroll {
        AntiRickroll {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "AntiRickroll",
                    env!("CARGO_PKG_VERSION"),
                    "Find rickroll links in messages (local check)",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("AntiRickroll".into());
                m
            },
            state: Shared::default(),
        }
    }

    /// The `/phoecord rickrolls` command. Register it with `Commands::with`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        let state = self.state.clone();
        FnCmd::new(
            PLUGIN_ID,
            "rickrolls",
            "List messages with rickroll links",
            move |host: &mut dyn Host, _args: &Args| state.lock().report(host),
        )
        .boxed()
    }
}

impl Default for AntiRickroll {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for AntiRickroll {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        match event {
            EventKind::Dispatch { name } => wanted(name),
            EventKind::RestResponse { method, route } => is_history(method, route),
            _ => false,
        }
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        match event {
            Event::Dispatch { name, payload, .. } if wanted(name) => {
                // The proxy hands over the whole `{op, t, d}` frame.
                let payload = payload.get("d").unwrap_or(payload);
                if name.starts_with("MESSAGE_DELETE") {
                    self.state.lock().deleted(payload);
                } else {
                    let rules = Rules::of(host);
                    self.state.lock().message(&rules, payload);
                }
            }
            Event::RestResponse {
                method,
                route,
                status,
                body,
            } if is_history(method, route) && (200..300).contains(&status) => {
                let rules = Rules::of(host);
                let mut st = self.state.lock();
                for m in body.as_list() {
                    st.message(&rules, m);
                }
            }
            _ => {}
        }
        Verdict::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{Rec, j};
    use phoecord_plugin_runtime::{Chain, Outcome};

    fn rules(links: &str, ids: &str) -> Rules {
        let h = Rec::default()
            .with("customLinks", Val::Str(links.into()))
            .with("customVideoIds", Val::Str(ids.into()));
        Rules::of(&h)
    }

    #[test]
    fn known_video_ids_and_hosts() {
        let r = rules("", "");
        for url in [
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com/watch?x=1&V=dQw4w9WgXcQ",
            "https://youtu.be/dQw4w9WgXcQ",
            "https://youtu.be/dQw4w9WgXcQ?t=3",
            "https://www.rickroll.link/anything",
            "http://rickroll.net:80/",
            "https://RICKROLL.NET/",
        ] {
            assert!(r.is_potential(url), "{url}");
        }
        for url in [
            "https://www.youtube.com/watch?v=aaaaaaaaaaa",
            "https://m.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtu.be/dQw4w9WgXcQ/",
            "https://example.com/rickroll.link",
            "https://",
            "https://exa mple.com",
            "https://a.com:99999/",
            "ftp://rickroll.net",
        ] {
            assert!(!r.is_potential(url), "{url}");
        }
        // The first "www." anywhere is removed, as String.replace does.
        assert!(!rules("", "").is_potential("https://wwwrickroll.net"));
        assert_eq!(KNOWN_HOSTS.len(), 106);
        assert_eq!(KNOWN_VIDEO_IDS.len(), 493);
    }

    #[test]
    fn custom_rules() {
        let r = rules(" evil.example/x , ,", "abc123, ");
        assert!(r.is_potential("https://evil.example/x/y"));
        assert!(r.is_custom("https://evil.example/x/y"));
        assert!(r.is_potential("https://youtu.be/abc123"));
        assert!(r.is_custom("https://youtube.com/watch?v=abc123"));
        // Capital V counts for the built-in list only.
        assert!(!r.is_custom("https://youtube.com/watch?V=abc123"));
        assert!(r.is_potential("https://youtube.com/watch?V=dQw4w9WgXcQ"));
        assert!(!r.is_custom("https://youtube.com/watch?v=dQw4w9WgXcQ"));
        assert!(!r.is_potential("https://other.example/"));
    }

    #[test]
    fn links_are_found_in_markdown_masked_and_bare_form() {
        let urls = extract_urls(
            "see [this](https://a.example/1) and <https://b.example/2> and https://c.example/3.",
        );
        assert_eq!(urls[0], "https://a.example/1");
        assert!(urls.contains(&"https://c.example/3.".to_string()));
        // The bare-link pattern only stops at `<`, so it also yields the link with its `>`.
        assert!(urls.contains(&"https://b.example/2>".to_string()));
        assert!(urls.contains(&"https://b.example/2".to_string()));
        let r = rules("", "");
        let (url, custom) = check(&r, "x [click](<https://youtu.be/dQw4w9WgXcQ>) y").unwrap();
        assert_eq!(url, "https://youtu.be/dQw4w9WgXcQ");
        assert!(!custom);
        assert!(check(&r, "no links here").is_none());
    }

    #[test]
    fn messages_and_history_are_remembered_until_deleted() {
        let p = AntiRickroll::new();
        let cmd = p.command();
        let chain = Chain::new(vec![Box::new(p)]);
        let mut h = Rec::default();
        let mut d =
            j(r#"{"id":"10","channel_id":"5","content":"look https://youtu.be/dQw4w9WgXcQ"}"#);
        assert_eq!(
            chain.dispatch(&mut h, "MESSAGE_CREATE", &mut d, &mut Vec::new()),
            Outcome::Untouched
        );
        let mut page = j(r#"[{"id":"11","channel_id":"5","content":"hi"},
            {"id":"12","channel_id":"5","content":"https://rickroll.link"}, 7]"#);
        let out = chain.rest_response(
            &mut h,
            "GET",
            "/channels/5/messages?limit=50",
            200,
            &mut page,
        );
        assert_eq!(out, Outcome::Untouched);
        let text = cmd.run(&mut h, &Args { options: vec![] });
        assert!(text.contains("links: 2"), "{text}");
        assert!(text.contains("https://rickroll.link") && text.contains("dQw4w9WgXcQ"));
        // An edit without content leaves it; an edit that removes the link drops it.
        let mut e = j(r#"{"id":"10","channel_id":"5"}"#);
        chain.dispatch(&mut h, "MESSAGE_UPDATE", &mut e, &mut Vec::new());
        let mut e = j(r#"{"id":"12","channel_id":"5","content":"sorry"}"#);
        chain.dispatch(&mut h, "MESSAGE_UPDATE", &mut e, &mut Vec::new());
        let mut gone = j(r#"{"id":"10","channel_id":"5"}"#);
        chain.dispatch(&mut h, "MESSAGE_DELETE", &mut gone, &mut Vec::new());
        assert!(
            cmd.run(&mut h, &Args { options: vec![] })
                .contains("No rickroll")
        );
        assert!(h.rest_calls.is_empty(), "purely local");
    }

    #[test]
    fn real_gateway_frames_are_read_from_d() {
        let p = AntiRickroll::new();
        let cmd = p.command();
        let chain = Chain::new(vec![Box::new(p)]);
        let mut h = Rec::default();
        let mut f = j(r#"{"op":0,"t":"MESSAGE_CREATE","s":3,
            "d":{"id":"10","channel_id":"5","content":"https://youtu.be/dQw4w9WgXcQ"}}"#);
        chain.dispatch(&mut h, "MESSAGE_CREATE", &mut f, &mut Vec::new());
        assert!(
            cmd.run(&mut h, &Args { options: vec![] })
                .contains("links: 1")
        );
        let mut gone = j(r#"{"op":0,"t":"MESSAGE_DELETE","d":{"id":"10","channel_id":"5"}}"#);
        chain.dispatch(&mut h, "MESSAGE_DELETE", &mut gone, &mut Vec::new());
        assert!(
            cmd.run(&mut h, &Args { options: vec![] })
                .contains("No rickroll")
        );
    }

    #[test]
    fn custom_matches_are_labelled() {
        let p = AntiRickroll::new();
        let cmd = p.command();
        let chain = Chain::new(vec![Box::new(p)]);
        let mut h = Rec::default().with("customLinks", Val::Str("bad.example".into()));
        let mut d = j(r#"{"id":"1","channel_id":"2","content":"https://bad.example/a"}"#);
        chain.dispatch(&mut h, "MESSAGE_CREATE", &mut d, &mut Vec::new());
        assert!(
            cmd.run(&mut h, &Args { options: vec![] })
                .contains("filters")
        );
    }

    #[test]
    fn malformed_payloads_do_not_panic() {
        let chain = Chain::new(vec![Box::new(AntiRickroll::new())]);
        let mut h = Rec::default();
        for d in [
            "{}",
            "[]",
            r#"{"id":"x","content":5}"#,
            r#"{"id":"1","content":null}"#,
        ] {
            let mut v = j(d);
            for n in [
                "MESSAGE_CREATE",
                "MESSAGE_UPDATE",
                "MESSAGE_DELETE",
                "MESSAGE_DELETE_BULK",
            ] {
                chain.dispatch(&mut h, n, &mut v, &mut Vec::new());
            }
            chain.rest_response(&mut h, "GET", "/channels/1/messages", 200, &mut v);
        }
        let mut err = j(r#"{"message":"no"}"#);
        chain.rest_response(&mut h, "GET", "/channels/1/messages", 403, &mut err);
        for u in [
            "https://[::1",
            "https://a@",
            "https://:80",
            "https://a/%zz",
            "http://\\\\",
        ] {
            let _ = parse_url(u);
        }
    }

    #[test]
    fn url_normalisation() {
        let u = parse_url("https://User@WWW.Example.com:443/a/./b/../c d?x=1#f").unwrap();
        assert_eq!(u.href, "https://User@www.example.com/a/c%20d?x=1#f");
        assert_eq!(parse_url("http://a.com").unwrap().pathname, "/");
        assert_eq!(
            parse_url("https://a.com:8080/x").unwrap().href,
            "https://a.com:8080/x"
        );
    }
}
