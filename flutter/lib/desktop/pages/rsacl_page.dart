// rsacl: the employee window of rsacl-ra.exe, the 1안 세로 카드 (rs-acl design canvas, artboards `Main` and
// `Connected`), drawn exactly: 400x480, rs-ui light tokens, Pretendard, the company logo.
//
// The Rust side (src/rsacl.rs) reports the RustDesk ID to acl and keeps the state; this page polls it with
// `main_get_common_sync("rsacl-state")` and ends through `main_get_common("rsacl-quit")`.

import 'dart:async';
import 'dart:convert';
import 'dart:io' show exit;

import 'package:flutter/material.dart';
import 'package:window_manager/window_manager.dart';

import '../../models/platform_model.dart';

const String kRsaclWindowTitle = '원격 지원 요청'; // src/rsacl.rs `win::WINDOW_TITLE` finds the window by it
const Size kRsaclWindowSize = Size(400, 480);
const String _font = 'RsaclPretendard';

// rs-ui tokens (light), from the canvas's ds/rsui/_ds_bundle.css.
const _white = Color(0xFFFFFFFF); // --rsui-white
const _ink900 = Color(0xFF23262B); // --rsui-ink-900
const _ink700 = Color(0xFF3A3F47); // --rsui-ink-700
const _ink600 = Color(0xFF5A6069); // --rsui-ink-600
const _surface2 = Color(0xFFF3F3EF); // --rsui-surface-2
const _surface3 = Color(0xFFEEEEEA); // --rsui-surface-3
const _border = Color(0xFFEAEAE6); // --rsui-border
const _success = Color(0xFF16A34A); // --rsui-success
const _primary = Color(0xFF2C66E0); // --rsui-primary
const _primary50 = Color(0xFFEEF3FD); // --rsui-primary-50
const _primary600 = Color(0xFF2557D6); // --rsui-primary-600
const _danger = Color(0xFFC23A38); // --rsui-danger
const _onFill = Color(0xFFFFFFFF); // --rsui-on-fill
const double _radiusLg = 9; // --rsui-radius-lg
const double _radiusPill = 99; // --rsui-radius-pill

/// Started by `runMainApp` instead of RustDesk's main window when the Rust side says `rsacl-mode` is on.
void runRsaclApp() {
  runApp(const RsaclApp());
  const options = WindowOptions(
    size: kRsaclWindowSize,
    center: true,
    backgroundColor: _white,
    skipTaskbar: false,
    titleBarStyle: TitleBarStyle.hidden,
    title: kRsaclWindowTitle,
  );
  windowManager.waitUntilReadyToShow(options, () async {
    await windowManager.setTitle(kRsaclWindowTitle);
    await windowManager.setSize(kRsaclWindowSize);
    await windowManager.setResizable(false);
    await windowManager.setMaximizable(false);
    await windowManager.center();
    await windowManager.setOpacity(1);
    await windowManager.show();
    await windowManager.focus();
  });
}

bool isRsaclMode() => bind.mainGetCommonSync(key: 'rsacl-mode') == 'Y';

class RsaclApp extends StatelessWidget {
  const RsaclApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: kRsaclWindowTitle,
      debugShowCheckedModeBanner: false,
      theme: ThemeData(
        useMaterial3: false,
        fontFamily: _font,
        scaffoldBackgroundColor: _white,
      ),
      home: const RsaclPage(),
    );
  }
}

class RsaclPage extends StatefulWidget {
  const RsaclPage({super.key});

  @override
  State<RsaclPage> createState() => _RsaclPageState();
}

class _RsaclPageState extends State<RsaclPage> with WindowListener {
  Map<String, dynamic> _s = const {'phase': 'wait'};
  String _raw = '';
  Timer? _timer;
  bool _quitting = false;

  @override
  void initState() {
    super.initState();
    windowManager.addListener(this);
    _poll();
    _timer = Timer.periodic(const Duration(milliseconds: 500), (_) => _poll());
  }

  @override
  void dispose() {
    _timer?.cancel();
    windowManager.removeListener(this);
    super.dispose();
  }

  void _poll() {
    final raw = bind.mainGetCommonSync(key: 'rsacl-state');
    if (raw == _raw || raw.isEmpty) return;
    try {
      final s = jsonDecode(raw) as Map<String, dynamic>;
      setState(() {
        _raw = raw;
        _s = s;
      });
    } catch (_) {}
  }

  /// 닫기, 지원 끝내기 and the window's own close: the Rust side withdraws the request, ends the sessions and
  /// the connection manager, and answers; then this window closes and the process ends on its own thread.
  /// The window stays up until the request is withdrawn (at most a few seconds), so a stalled end is visible;
  /// the Rust side also terminates the process 10 s after the request.
  Future<void> _quit() async {
    if (_quitting) return;
    _quitting = true;
    try {
      await bind
          .mainGetCommon(key: 'rsacl-quit')
          .timeout(const Duration(seconds: 6));
    } catch (_) {}
    try {
      await windowManager.setPreventClose(false);
      await windowManager.destroy();
    } catch (_) {}
    exit(0);
  }

  @override
  void onWindowClose() {
    _quit();
  }

  @override
  Widget build(BuildContext context) => RsaclCard(state: _s, onQuit: _quit);
}

/// The card itself, for a given state (`phase` wait / ready / connected / error, `number`, `until`,
/// `target`, `helper`, `error`); no FFI, so it can be drawn in a test.
class RsaclCard extends StatelessWidget {
  final Map<String, dynamic> state;
  final VoidCallback onQuit;
  const RsaclCard({super.key, required this.state, required this.onQuit});

  String _str(String k) => (state[k] ?? '').toString();

  @override
  Widget build(BuildContext context) {
    final phase = _str('phase');
    late final Widget body;
    late final Widget footer;
    switch (phase) {
      case 'connected':
        body = _connected();
        footer = _footer(null,
            _Btn(label: '지원 끝내기', danger: true, onPressed: onQuit));
        break;
      case 'ready':
        body = _main();
        footer = _footer('${_str('until')}까지 유지 · ${_str('target')}',
            _Btn(label: '닫기', onPressed: onQuit));
        break;
      case 'error':
        body = _error();
        footer = _footer(null, _Btn(label: '닫기', onPressed: onQuit));
        break;
      default:
        body = _waiting();
        footer = _footer(_str('target'), _Btn(label: '닫기', onPressed: onQuit));
    }
    return Scaffold(
      backgroundColor: _white,
      body: DefaultTextStyle(
        style: const TextStyle(fontFamily: _font, color: _ink900),
        child: SizedBox(
          width: kRsaclWindowSize.width,
          height: kRsaclWindowSize.height,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              _header(),
              Expanded(
                child: Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 32),
                  child: body,
                ),
              ),
              footer,
            ],
          ),
        ),
      ),
    );
  }

  // header: logo (height 26) and 원격 지원; also the window's drag handle (no title bar).
  Widget _header() {
    return GestureDetector(
      behavior: HitTestBehavior.translucent,
      onPanStart: (_) => windowManager.startDragging(),
      child: Padding(
        padding: const EdgeInsets.fromLTRB(24, 20, 24, 0),
        child: Row(
          mainAxisAlignment: MainAxisAlignment.spaceBetween,
          children: [
            Image.asset('assets/rsacl/rsa-ci.png',
                height: 26,
                filterQuality: FilterQuality.high,
                semanticLabel: 'RS Automation'),
            const Text('원격 지원',
                style: TextStyle(fontSize: 13, color: _ink600)),
          ],
        ),
      ),
    );
  }

  Widget _footer(String? left, Widget button) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 16),
      decoration: const BoxDecoration(
          border: Border(top: BorderSide(color: _border, width: 1))),
      child: Row(
        mainAxisAlignment: left == null
            ? MainAxisAlignment.end
            : MainAxisAlignment.spaceBetween,
        children: [
          if (left != null)
            Flexible(
              child: Text(left,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(fontSize: 12, color: _ink600)),
            ),
          button,
        ],
      ),
    );
  }

  Widget _center(List<Widget> children) {
    final spaced = <Widget>[];
    for (var i = 0; i < children.length; i++) {
      if (i > 0) spaced.add(const SizedBox(height: 14)); // gap: 14px
      spaced.add(children[i]);
    }
    return Column(
      mainAxisAlignment: MainAxisAlignment.center,
      crossAxisAlignment: CrossAxisAlignment.center,
      children: spaced,
    );
  }

  static const _guide = TextStyle(fontSize: 15, height: 1.6, color: _ink700);

  // Main artboard.
  Widget _main() {
    return _center([
      const Text('접수 번호', style: TextStyle(fontSize: 14, color: _ink600)),
      Text(
        _str('number'),
        style: const TextStyle(
          fontSize: 64,
          fontWeight: FontWeight.w700,
          letterSpacing: 8,
          color: _ink900,
          fontFeatures: [FontFeature.tabularFigures()],
        ),
      ),
      const Text(
        'IT팀에 이 번호를 알려 주세요.\n담당자가 연결하면 뜨는 창에서 [수락]을 누르세요.',
        textAlign: TextAlign.center,
        style: _guide,
      ),
      Padding(
        padding: const EdgeInsets.only(top: 6),
        child: _Pill(
          background: _surface2,
          dot: _success,
          text: '담당자 연결을 기다리는 중',
          style: const TextStyle(fontSize: 13, color: _ink700),
        ),
      ),
    ]);
  }

  // Connected artboard.
  Widget _connected() {
    return _center([
      _Pill(
        background: _primary50,
        dot: _primary,
        text: '지원 중',
        style: const TextStyle(
            fontSize: 13, color: _primary600, fontWeight: FontWeight.w600),
      ),
      Text(
        '${_str('helper')} 님이 연결했습니다',
        textAlign: TextAlign.center,
        style: const TextStyle(
            fontSize: 24, fontWeight: FontWeight.w700, color: _ink900),
      ),
      const Text(
        '담당자가 이 PC 화면을 보고 조작하고 있습니다.\n지원이 끝나면 [지원 끝내기]를 누르세요.',
        textAlign: TextAlign.center,
        style: _guide,
      ),
      Text('접수 번호 ${_str('number')}',
          style: const TextStyle(fontSize: 13, color: _ink600)),
    ]);
  }

  // Before acl answers: the Main layout's label with "번호를 받는 중…" in the number's place.
  Widget _waiting() {
    return _center([
      const Text('접수 번호', style: TextStyle(fontSize: 14, color: _ink600)),
      const Text('번호를 받는 중…',
          style: TextStyle(fontSize: 20, color: _ink600)),
    ]);
  }

  // The request failed: no number, the retired rsacl-ra.ps1 wording (first paragraph as the heading).
  Widget _error() {
    final parts = _str('error').split('\n\n');
    return _center([
      Text(parts.first,
          textAlign: TextAlign.center,
          style: const TextStyle(
              fontSize: 17, fontWeight: FontWeight.w700, color: _ink900)),
      if (parts.length > 1)
        Text(parts.sublist(1).join('\n\n'),
            textAlign: TextAlign.center,
            style: const TextStyle(fontSize: 14, height: 1.6, color: _ink700)),
    ]);
  }
}

/// Status pill: padding 6x12, pill radius, an 8px dot, gap 8.
class _Pill extends StatelessWidget {
  final Color background;
  final Color dot;
  final String text;
  final TextStyle style;
  const _Pill(
      {required this.background,
      required this.dot,
      required this.text,
      required this.style});

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
      decoration: BoxDecoration(
        color: background,
        borderRadius: BorderRadius.circular(_radiusPill),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Container(
            width: 8,
            height: 8,
            decoration: BoxDecoration(color: dot, shape: BoxShape.circle),
          ),
          const SizedBox(width: 8),
          Text(text, style: style),
        ],
      ),
    );
  }
}

/// RsUi.Button size md: height 36, padding 0 16, 14px/20px weight 500, radius-lg. Variant `subtle`
/// (surface-2, ink-700, 1px border; hover surface-3) or `danger` (danger fill, on-fill; hover 10% darker).
class _Btn extends StatefulWidget {
  final String label;
  final bool danger;
  final VoidCallback onPressed;
  const _Btn(
      {required this.label, required this.onPressed, this.danger = false});

  @override
  State<_Btn> createState() => _BtnState();
}

class _BtnState extends State<_Btn> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final Color bg = widget.danger
        ? (_hover ? Color.lerp(_danger, Colors.black, 0.1)! : _danger)
        : (_hover ? _surface3 : _surface2);
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        onTap: widget.onPressed,
        child: Container(
          height: 36,
          padding: const EdgeInsets.symmetric(horizontal: 16),
          alignment: Alignment.center,
          decoration: BoxDecoration(
            color: bg,
            borderRadius: BorderRadius.circular(_radiusLg),
            border: widget.danger ? null : Border.all(color: _border, width: 1),
          ),
          child: Text(
            widget.label,
            style: TextStyle(
              fontSize: 14,
              height: 20 / 14,
              fontWeight: FontWeight.w500,
              color: widget.danger ? _onFill : _ink700,
            ),
          ),
        ),
      ),
    );
  }
}
