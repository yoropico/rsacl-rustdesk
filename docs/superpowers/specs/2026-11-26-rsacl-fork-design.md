# rsacl fork of RustDesk — what the fork changes (2026-11-26, KST)

The binding design is rs-acl `docs/superpowers/specs/2026-11-26-ra-one-exe-design.md` (branch `ra-one-exe`), with
its plan `docs/superpowers/plans/2026-11-26-ra-one-exe.md` (Global Constraints, Task 3) and the measurement note
`docs/superpowers/notes/2026-11-26-rustdesk-measure.md`. This file records only how the fork carries it out and the
choices the brief left to the fork.

## Where the code is

- `src/rsacl.rs` and `src/rsacl/acl.rs` hold everything; upstream files get call sites only: `common.rs`
  (`load_custom_client` calls `rsacl::apply_settings`), `core_main.rs` (`rsacl::enter` for a start with no
  arguments), `flutter_ffi.rs` (`main_get_common` / `main_set_common` keys `rsacl-*`), `rendezvous_mediator.rs`
  (`rsacl::no_inbound` forces relay), `server/connection.rs` (`SessionKey::name`), `flutter/lib/main.dart`
  (`runRsaclApp`), `flutter/lib/desktop/pages/rsacl_page.dart` (the window), `flutter/pubspec.yaml` (assets).

## Settings

- Applied in every process of the exe (main window, connection manager, installed service), as override,
  built-in and hard settings: the server, key and every option of the plan's Global Constraints, plus
  `direct-server = N`, `disable-tcp-listen = Y` and `hide-elevate-button-in-accept-window = Y`.

## No listening socket (no Windows Firewall prompt)

- The measured prompt came when a helper connected: the helper's RustDesk asked for a TCP punch and the PC opened a
  listener on its rendezvous port. The code paths that bind an inbound socket on the controlled side are the TCP
  punch listener (`punch_tcp_until_connected`, `accept_connection_`), the UDP punch (`udp_nat_listen`), the IPv6
  punch (`start_ipv6`), the LAN listen of a `FetchLocalAddr` (`handle_intranet_`), the WebRTC answerer, the direct
  access server (`direct_server`, option `direct-server`) and LAN discovery (`lan::start_listening`, installed only).
- `no_inbound()` makes `handle_punch_hole` and `handle_intranet` relay and skips the WebRTC answer; `direct-server`
  is forced off; LAN discovery never starts in a portable run. What stays is the outbound UDP registration socket on
  an ephemeral port, which raised no prompt on either test PC.

## The employee's run

- A start with no arguments of a copy that is not the installed one: one instance per session (named mutex
  `Local\rsacl-ra-employee-window`; a second start brings the first window forward and exits), then the worker
  waits until the ID is registered (`status_num == 1`, 9–10 digits), sends `ra/request`, and watches the
  authorised connections for the helper's name.
- The installed copy (a helper's PC, installed with `--silent-install`) keeps RustDesk's own window.
- 닫기, 지원 끝내기 and the window's close: withdraw, close every session, end the connection manager, exit.
  Logoff: a hidden top-level window withdraws on `WM_ENDSESSION` (3 s timeout); the request also expires after 2 h.
- Errors show no number and the retired `ra/rsacl-ra.ps1` wording; a failed request is not retried (the text tells the
  employee to press [원격 지원 요청] again).

## Accept window

- Kept as RustDesk draws it (helper name, 수락, 취소). "수락 및 권한 상승" is hidden: the employee is a standard
  user, so it would only raise a UAC prompt for an administrator's password that the helper cannot see.
