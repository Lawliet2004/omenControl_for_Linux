#!/usr/bin/env python3
"""Native GTK + portal protocol fixtures; no host shortcut registration or control writes."""
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import tomllib

ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = Path(os.environ.get("OMEN_TEST_EVIDENCE_DIR", ROOT / "docs/evidence"))
PROFILE = os.environ.get("OMEN_TEST_PROFILE", "debug")
BACKEND = os.environ.get("GDK_BACKEND", "auto")
BIN = ROOT / "target" / PROFILE
DEST = "org.freedesktop.portal.Desktop"
PATH = "/org/freedesktop/portal/desktop"
IFACE = "org.freedesktop.portal.GlobalShortcuts"
ACTION = "show-window"


def portal():
    from gi.repository import Gio, GLib
    connection = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    state = {"scenario": "grant", "version": 2, "creates": 0, "binds": 0, "configs": 0, "closes": 0, "request_closes": 0,
             "registered": set(), "sessions": {}, "requests": {}, "parents": [], "preferred_triggers": []}
    registrations = []
    def register(path, xml, callback, prop=None):
        info = Gio.DBusNodeInfo.new_for_xml(xml)
        for interface in info.interfaces:
            registrations.append(connection.register_object(path, interface, callback, prop, None))

    def shortcuts(key="Meta+O"):
        return [(ACTION, {"description": GLib.Variant("s", "Show OMEN Control"), "trigger_description": GLib.Variant("s", key)}),
                ("toggle-overlay", {"description": GLib.Variant("s", "Toggle OMEN status overlay"), "trigger_description": GLib.Variant("s", "Meta+M")})]

    def emit_response(request, data, code=0):
        owner = state["requests"][request]["owner"]
        connection.emit_signal(owner, request, "org.freedesktop.portal.Request", "Response", GLib.Variant("(ua{sv})", (code, data)))

    def session_method(_c, _sender, path, _iface, method, _params, invocation):
        assert method == "Close"
        state["sessions"].pop(path, None)
        state["closes"] += 1
        invocation.return_value(None)

    def request_method(_c, _sender, path, _iface, method, _params, invocation):
        assert method == "Close"
        request = state["requests"].get(path)
        if request:
            state["sessions"].pop(request.get("session"), None)
        state["request_closes"] += 1
        invocation.return_value(None)

    def method(_c, sender, _path, interface, method, params, invocation):
        args = params.unpack()
        if interface == "org.freedesktop.host.portal.Registry":
            assert method == "Register" and args[0] == "com.omenlinux.Gui"
            if state["scenario"] == "registration_error":
                invocation.return_dbus_error("org.freedesktop.portal.Error.NotAllowed", "fixture registration refusal")
                return
            state["registered"].add(sender)
            if state["scenario"] == "slow_registration":
                GLib.timeout_add(350, lambda: (invocation.return_value(None), False)[1])
                return
            invocation.return_value(None)
            return
        assert sender in state["registered"], "portal used before host-app registration"
        if method == "ConfigureShortcuts":
            assert state["version"] >= 2
            state["configs"] += 1
            session = args[0]
            assert session in state["sessions"]
            # Emit before the reply, as permitted by the asynchronous protocol.
            connection.emit_signal(sender, PATH, IFACE, "ShortcutsChanged", GLib.Variant("(oa(sa{sv}))", (session, shortcuts("Meta+Shift+O"))))
            invocation.return_value(None)
            return
        options = args[0] if method == "CreateSession" else args[-1]
        encoded = sender[1:].replace(".", "_")
        request = f"{PATH}/request/{encoded}/{options['handle_token']}"
        session = f"{PATH}/session/{encoded}/{options['session_handle_token']}" if method == "CreateSession" else args[0]
        state["requests"][request] = {"owner": sender, "session": session}
        register(request, '<node><interface name="org.freedesktop.portal.Request"><method name="Close"/></interface></node>', request_method)
        if method == "CreateSession":
            state["creates"] += 1
            state["sessions"][session] = {"owner": sender}
            register(session, '<node><interface name="org.freedesktop.portal.Session"><method name="Close"/></interface></node>', session_method)
            data = {"session_handle": GLib.Variant("s", session if state["scenario"] != "bad_session" else "/unexpected/session")}
        elif method == "BindShortcuts":
            state["binds"] += 1
            assert session in state["sessions"]
            assert [item[0] for item in args[1]] == [ACTION, "toggle-overlay"]
            state["parents"].append(args[2])
            state["preferred_triggers"].append(args[1][0][1].get("preferred_trigger"))
            data = {"shortcuts": GLib.Variant("a(sa{sv})", [] if state["scenario"] == "empty" else shortcuts())}
        else:
            raise AssertionError(method)
        code = 1 if state["scenario"] == "cancel" and method == "BindShortcuts" else 2 if state["scenario"] == "deny" and method == "BindShortcuts" else 0
        hold = state["scenario"] == "hold_create" and method == "CreateSession" or state["scenario"] == "hold_bind" and method == "BindShortcuts"
        if not hold:
            emit_response(request, data, code)
        invocation.return_value(GLib.Variant("(o)", ("/unexpected/request" if state["scenario"] == "bad_handle" else request,)))

    def prop(_c, sender, _path, interface, name):
        assert sender in state["registered"] or interface != IFACE
        return GLib.Variant("u", state["version"] if interface == IFACE else 1)

    xml = '''<node>
      <interface name="org.freedesktop.host.portal.Registry"><method name="Register"><arg type="s" direction="in"/><arg type="a{sv}" direction="in"/></method><property name="version" type="u" access="read"/></interface>
      <interface name="org.freedesktop.portal.GlobalShortcuts">
        <method name="CreateSession"><arg type="a{sv}" direction="in"/><arg type="o" direction="out"/></method>
        <method name="BindShortcuts"><arg type="o" direction="in"/><arg type="a(sa{sv})" direction="in"/><arg type="s" direction="in"/><arg type="a{sv}" direction="in"/><arg type="o" direction="out"/></method>
        <method name="ConfigureShortcuts"><arg type="o" direction="in"/><arg type="s" direction="in"/><arg type="a{sv}" direction="in"/></method>
        <property name="version" type="u" access="read"/>
      </interface></node>'''
    register(PATH, xml, method, prop)
    owner = [None]
    def own():
        owner[0] = Gio.bus_own_name_on_connection(connection, DEST, Gio.BusNameOwnerFlags.NONE, None, None)
    def control(_c, _s, _p, _i, method, params, invocation):
        args = params.unpack()
        if method == "State":
            data = {k: v for k, v in state.items() if k not in ["registered", "requests"]}
            invocation.return_value(GLib.Variant("(s)", (json.dumps(data),)))
            return
        if method == "Scenario":
            state["scenario"] = args[0]
        elif method == "Version":
            state["version"] = args[0]
        elif method == "Release":
            Gio.bus_unown_name(owner[0])
        elif method == "Acquire":
            own()
        elif method == "Fire":
            session, data = next(iter(state["sessions"].items()))
            selected = "/unrelated/session" if args[0] == "wrong_session" else session
            identifier = "unrelated" if args[0] == "wrong_action" else "toggle-overlay" if args[0] == "overlay" else ACTION
            connection.emit_signal(data["owner"], PATH, IFACE, "Activated", GLib.Variant("(osta{sv})", (selected, identifier, 12345, {})))
        elif method == "Closed":
            session, data = next(iter(state["sessions"].items()))
            connection.emit_signal(data["owner"], session, "org.freedesktop.portal.Session", "Closed", GLib.Variant("(a{sv})", ({},)))
            state["sessions"].pop(session)
        else:
            raise AssertionError(method)
        invocation.return_value(None)
    register('/com/omenlinux/Test', '''<node><interface name="com.omenlinux.Test">
      <method name="State"><arg type="s" direction="out"/></method>
      <method name="Scenario"><arg type="s" direction="in"/></method>
      <method name="Version"><arg type="u" direction="in"/></method>
      <method name="Fire"><arg type="s" direction="in"/></method>
      <method name="Closed"/><method name="Release"/><method name="Acquire"/>
      </interface></node>''', control)
    def disconnected(_c, _s, _p, _i, _m, params):
        name, _old, new = params.unpack()
        if name.startswith(":") and not new:
            state["registered"].discard(name)
            state["sessions"] = {path: s for path, s in state["sessions"].items() if s["owner"] != name}
    connection.signal_subscribe('org.freedesktop.DBus', 'org.freedesktop.DBus', 'NameOwnerChanged', '/org/freedesktop/DBus', None,
                                Gio.DBusSignalFlags.NONE, disconnected)
    own()
    print(connection.get_unique_name(), flush=True)
    GLib.MainLoop().run()


def main():
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    import tray
    import gui_settings
    import gi
    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi, GLib
    assert os.geteuid()!=0, 'run as the desktop user'
    assert BACKEND in ['x11','wayland','auto'], 'select one GTK backend for reproducible tests'
    result = {"result": "failed", "profile": PROFILE, "host_shortcuts_changed": False, "hardware_writes": False,
              "requested_gtk_backend": BACKEND,
              "method": "AT-SPI actions on real GTK and DBusMenu actions; private portal protocol and tray fixtures",
              "gui_sha256": hashlib.sha256((BIN / 'omen-gui').read_bytes()).hexdigest(), "checks_passed": []}
    with tempfile.TemporaryDirectory(prefix='omen-shortcuts-', dir=ROOT / 'target') as directory:
        temp = Path(directory)
        processes, logs = [], []
        app = None
        def launch(command, name, pipe=False):
            log = (temp / (name + '.log')).open('w'); logs.append(log)
            child = subprocess.Popen(command, env=os.environ, stdout=subprocess.PIPE if pipe else log, stderr=log, text=True)
            processes.append(child)
            return child
        def passed(label):
            result['checks_passed'].append(label); print('PASS', label, flush=True)
        try:
            for key, value in {'GTK_A11Y': 'atspi', 'GDK_DEBUG': 'no-portals', 'GSETTINGS_BACKEND': 'memory',
                               'XDG_CONFIG_HOME': str(temp/'xdg-config'), 'XDG_DATA_HOME': str(temp/'xdg-data'),
                               'XDG_CACHE_HOME': str(temp/'cache'), 'TMPDIR': str(temp), 'OMEN_HOME': str(temp/'games'),
                               'OMEN_SYSTEM_CONFIG_DIR': str(temp/'system'), 'LC_ALL': 'C.UTF-8'}.items():
                os.environ[key] = value
            applications=temp/'xdg-data/applications'; applications.mkdir(parents=True)
            entry=(ROOT/'packaging/com.omenlinux.Gui.desktop').read_text().replace('Exec=omen-gui','Exec='+str(BIN/'omen-gui'))
            (applications/'com.omenlinux.Gui.desktop').write_text(entry)
            config = temp / 'bus.conf'
            config.write_text(f'<busconfig><type>session</type><listen>unix:path={temp}/bus.sock</listen><auth>EXTERNAL</auth><policy context="default"><allow user="{os.getuid()}"/><allow own="*"/><allow send_destination="*"/><allow receive_type="method_call"/><allow receive_type="method_return"/><allow receive_type="error"/><allow receive_type="signal"/></policy></busconfig>')
            bus = launch(['dbus-daemon', '--config-file='+str(config), '--nofork', '--print-address=1'], 'bus', True)
            os.environ['DBUS_SESSION_BUS_ADDRESS'] = bus.stdout.readline().strip()
            os.environ['AT_SPI_BUS_ADDRESS'] = 'unix:path='+str(temp/'a11y.sock')
            a11y = launch(['dbus-daemon', '--config-file=/usr/share/defaults/at-spi2/accessibility.conf', '--address='+os.environ['AT_SPI_BUS_ADDRESS'], '--nofork', '--print-address=1'], 'a11y', True)
            assert a11y.stdout.readline().strip()
            service = launch([sys.executable, __file__, '--portal'], 'portal', True)
            portal_name = service.stdout.readline().strip(); assert portal_name.startswith(':')
            watcher = launch([sys.executable, str(ROOT/'tests/tray.py'), '--watcher'], 'watcher', True)
            assert watcher.stdout.readline().strip() == 'READY'
            daemon = launch([str(BIN/'omen-daemon'), '--mock'], 'daemon', True)
            endpoint = daemon.stdout.readline().strip(); assert gui_settings.call(endpoint,'get_info')['mock']
            app = tray.App(temp/'desktop', socket=endpoint, address=os.environ['DBUS_SESSION_BUS_ADDRESS'], page='settings')
            app.ready(); Atspi.init(); Atspi.set_timeout(2000,5000)
            context = GLib.MainContext.default()
            def wait(test, message, timeout=12):
                limit = time.monotonic()+timeout
                while time.monotonic()<limit:
                    while context.pending(): context.iteration(False)
                    try:
                        value = test()
                        if value: return value
                    except (GLib.Error, StopIteration, subprocess.CalledProcessError): pass
                    time.sleep(.05)
                if app and app.child.poll() is None:
                    result['failure_process_threads']=[{'name': (p/'comm').read_text().strip(), 'wchan': (p/'wchan').read_text().strip()} for p in Path(f'/proc/{app.child.pid}/task').iterdir()]
                    result['failure_bus_names']=subprocess.run(['busctl','--user','--list'],capture_output=True,text=True,timeout=3).stdout
                    result['failure_tray_query']=subprocess.run(tray.BUS+['get-property',app.service,'/StatusNotifierItem','org.kde.StatusNotifierItem','Menu'],env=app.env,capture_output=True,text=True,timeout=4).stderr
                raise RuntimeError(message)
            def own_app():
                desktop = Atspi.get_desktop(0)
                return next(a for i in range(desktop.get_child_count()) if (a:=desktop.get_child_at_index(i)).get_process_id()==app.child.pid)
            accessible = wait(own_app,'own GTK application missing'); accessible.set_cache_mask(Atspi.Cache.NONE); accessible.clear_cache()
            def walk(node=None):
                node = node or accessible
                try:
                    if node.get_state_set().contains(Atspi.StateType.DEFUNCT): return
                    yield node
                    for i in range(node.get_child_count()): yield from walk(node.get_child_at_index(i))
                except GLib.Error: return
            def label(text): return any(n.get_role_name()=='label' and text in n.get_name() for n in walk())
            def button(text): return next(n for n in walk() if n.get_role_name()=='button' and n.get_name()==text)
            def click(text):
                node = wait(lambda:button(text),'missing button '+text); assert node.get_state_set().contains(Atspi.StateType.SENSITIVE)
                action=node.get_action_iface(); index=next(i for i in range(action.get_n_actions()) if action.get_action_name(i)=='click'); assert action.do_action(index)
            def control(method, signature='', *args):
                data=tray.bus('call',portal_name,'/com/omenlinux/Test','com.omenlinux.Test',method,*(([signature]+list(args)) if signature else []))
                return json.loads(data['data'][0]) if method=='State' else data
            def state(): return control('State')
            def saved(): return tomllib.loads((temp/'desktop/config.toml').read_text()).get('general',{}).get('global_shortcut_enabled',False)
            def no_sessions(): return not state()['sessions']
            wait(lambda:label('Global shortcut available; disabled'),'support probe failed')
            assert state()['creates']==0 and saved() is False
            passed('startup only probes support; no shortcut session or preference write before opt-in')
            secondary=subprocess.run([str(BIN/'omen-gui')],env=app.env,capture_output=True,text=True,timeout=8)
            assert secondary.returncode==0 and 'OMEN_UI_BACKEND ' not in secondary.stderr
            assert app.child.poll() is None and app.text().count('OMEN_UI_BACKEND ')==1
            assert state()['creates']==0
            passed('a second ordinary launch activates the existing window without building another UI or shortcut controller')
            click('Enable global shortcut…'); wait(lambda:label('Show OMEN Control: Meta+O'),'grant failed')
            assert saved() is True and state()['binds']==1 and all(t is None for t in state()['preferred_triggers'])
            assert state()['parents'][-1].startswith(('wayland:','x11:'))
            passed('native enable registers app identity, parents the portal dialog and saves only after approval; no suggested key')
            app.click('Hide window'); wait(lambda:app.visible() is False,'could not hide fixture window')
            control('Fire','s','wrong_session'); control('Fire','s','wrong_action'); time.sleep(.2); assert app.visible() is False
            control('Fire','s','right'); wait(lambda:app.visible() is True,'valid activation did not show window')
            passed('only the current session and requested Show action reveal the GTK window')
            def overlay_click():
                wait(lambda:any(item[0]=='Show status overlay' for item in app.labels()),'overlay tray state did not refresh')
                app.click('Show status overlay')
            def overlay_visible():
                return any(n.get_name()=='OMEN Status Overlay' and n.get_state_set().contains(Atspi.StateType.SHOWING) for n in walk())
            def overlay_value(name):
                title=next(n for n in walk() if n.get_role_name()=='label' and n.get_name()==name)
                parent=title.get_parent()
                return parent.get_child_at_index(title.get_index_in_parent()+1).get_name()
            assert not overlay_visible() and 'Overlay: Meta+M' in next(n.get_name() for n in walk() if 'Show OMEN Control: Meta+O' in n.get_name())
            overlay_click(); wait(overlay_visible,'tray did not enable overlay')
            wait(lambda:overlay_value('CPU temperature (highest)')=='62 °C','overlay missing real fixture CPU measurement')
            assert tomllib.loads((app.directory/'config.toml').read_text())['general']['overlay_enabled'] is True
            assert 'MOCK' in next(n.get_name() for n in walk() if n.get_role_name()=='label' and 'MOCK' in n.get_name())
            passed('tray opt-in opens accessible compact window with fixture source, measured CPU data and saved preference')
            app.click('Hide window'); wait(lambda:app.visible() is False,'main did not hide during overlay test')
            assert overlay_visible()
            control('Fire','s','overlay'); wait(lambda:not overlay_visible(),'global action did not close overlay')
            assert app.visible() is False
            control('Fire','s','overlay'); wait(overlay_visible,'global action did not reopen overlay')
            assert app.visible() is False
            passed('approved portal action toggles overlay while main window remains hidden')
            control('Fire','s','right'); wait(lambda:app.visible() is True,'Show action did not retain main window identity')
            secondary=subprocess.run([str(BIN/'omen-gui')],env=app.env,capture_output=True,text=True,timeout=8)
            assert secondary.returncode==0 and app.text().count('OMEN_UI_BACKEND ')==1
            passed('normal second activation reuses main window even with overlay present')
            daemon.send_signal(signal.SIGSTOP)
            try:
                wait(lambda:label('Monitoring delayed · values are stale'),'stalled poller left apparently current overlay values',timeout=18)
                assert overlay_value('CPU fan')=='Stale' and overlay_value('Power profile')=='Stale'
                wait(lambda: button('Data stale'), 'main window did not report stalled monitoring', timeout=18)
                assert label('Monitoring data is stale; waiting for an update.')
            finally:
                daemon.send_signal(signal.SIGCONT)
            wait(lambda:overlay_value('CPU temperature (highest)')=='62 °C','overlay did not recover from delayed data')
            wait(lambda:button('Fixture'), 'main window did not recover from stalled monitoring', timeout=18)
            passed('main window and overlay independently mark stalled snapshots stale and recover after service resumes')
            click('Close overlay'); wait(lambda:not overlay_visible(),'native close action failed')
            assert tomllib.loads((app.directory/'config.toml').read_text())['general'].get('overlay_enabled',False) is False
            passed('native close action saves overlay opt-out without quitting main window')
            wait(lambda:any(item[0]=='Show status overlay' for item in app.labels()),'native close did not update tray')
            original=(app.directory/'config.toml').read_bytes()
            (app.directory/'config.toml').write_text('{invalid config')
            overlay_click(); wait(lambda:label('Overlay preference was not saved'),'bad config failure missing')
            assert not overlay_visible() and (app.directory/'config.toml').read_text()=='{invalid config'
            (app.directory/'config.toml').write_bytes(original)
            passed('corrupt preferences are preserved and prevent enabling overlay')
            overlay_click(); wait(overlay_visible,'overlay did not re-enable')
            (app.directory).chmod(0o500)
            try:
                click('Close overlay'); wait(lambda:not overlay_visible(),'unwritable preferences made overlay impossible to close')
                assert label('Overlay preference was not saved')
            finally:
                (app.directory).chmod(0o700)
            passed('overlay stays closable after preference persistence fails')
            # Restore enabled preference to verify startup restoration below.
            overlay_click(); wait(overlay_visible,'overlay final enable failed')
            click('Change shortcut…'); wait(lambda:label('Show OMEN Control: Meta+Shift+O'),'early ShortcutsChanged was lost')
            assert state()['configs']==1
            passed('version 2 Configure opens asynchronously and preserves a change signal arriving before its reply')
            click('Disable global shortcut'); wait(no_sessions,'disable leaked a portal session'); assert saved() is False
            passed('native disable releases the session and saves startup opt-out')
            for scenario, fragment in [('cancel','Shortcut setup cancelled'),('deny','Desktop declined'),('empty','did not enable'),('bad_handle','unexpected shortcut request handle'),('bad_session','unexpected shortcut session handle'),('registration_error','registration failed')]:
                control('Scenario','s',scenario); before=(temp/'desktop/config.toml').read_bytes()
                click('Enable global shortcut…'); wait(lambda:label(fragment),'missing '+scenario+' failure'); wait(no_sessions,'leaked '+scenario+' session')
                assert (temp/'desktop/config.toml').read_bytes()==before
                passed(scenario+' response leaves the shortcut disabled and preferences unchanged')
            for scenario in ['hold_create','hold_bind']:
                control('Scenario','s',scenario); previous=state()['creates']; binds=state()['binds']; click('Enable global shortcut…')
                wait(lambda:state()['creates']>previous,'request did not start')
                if scenario=='hold_bind': wait(lambda:state()['binds']>binds,'pending bind missing')
                click('Cancel shortcut setup'); wait(no_sessions,'cancel leaked '+scenario+' session'); assert saved() is False
                passed('cancel during '+scenario+' closes pending work without saving or retaining a session')
            control('Scenario','s','hold_bind'); previous=state()['binds']; click('Enable global shortcut…')
            wait(lambda:state()['binds']>previous,'pending loss test did not reach BindShortcuts')
            control('Release'); wait(lambda:label('service changed during setup'),'pending owner loss was not reported promptly')
            wait(no_sessions,'pending owner loss retained a session'); assert saved() is False
            control('Acquire'); time.sleep(.1)
            passed('portal loss during approval fails promptly and releases pending sessions')
            control('Scenario','s','grant')
            original=(temp/'desktop/config.toml').read_bytes(); before=state()['creates']
            (temp/'desktop/config.toml').write_text('{invalid config')
            click('Enable global shortcut…'); wait(lambda:label('Shortcut preference could not be read'),'invalid preferences not reported')
            assert state()['creates']==before
            (temp/'desktop/config.toml').write_bytes(original)
            passed('corrupt preferences fail before creating a portal session and are not overwritten')
            (temp/'desktop').chmod(0o500)
            try:
                click('Enable global shortcut…'); wait(lambda:label('Permission denied'),'preference write failure not reported')
                wait(no_sessions,'preference write failure retained an approved session')
                assert (temp/'desktop/config.toml').read_bytes()==original
            finally:
                (temp/'desktop').chmod(0o700)
            passed('failed preference persistence releases the approved session and retains the original file')
            control('Scenario','s','grant'); control('Version','u','1')
            click('Enable global shortcut…'); wait(lambda:label('Show OMEN Control: Meta+O'),'version 1 enable failed')
            assert not any(n.get_role_name()=='button' and n.get_name()=='Change shortcut…' and n.get_state_set().contains(Atspi.StateType.VISIBLE) for n in walk())
            passed('version 1 portal binds normally and hides unsupported Configure action')
            control('Closed'); wait(lambda:label('desktop closed the shortcut session'),'session loss not reported'); wait(no_sessions,'closed session retained')
            assert saved() is True
            passed('backend session closure disables activation and retains the requested startup preference')
            control('Version','u','2'); click('Enable global shortcut…'); wait(lambda:label('Show OMEN Control: Meta+O'),'re-enable failed')
            control('Release'); wait(lambda:label('shortcut service changed'),'portal owner loss not reported'); wait(no_sessions,'owner loss retained session')
            control('Acquire'); time.sleep(.1); click('Enable global shortcut…'); wait(lambda:label('Show OMEN Control: Meta+O'),'portal recovery failed')
            passed('portal owner loss releases the old session; re-enable registers a fresh connection after recovery')
            app.quit(); wait(no_sessions,'Quit leaked active shortcuts')
            passed('quitting releases the active portal session without clearing the startup preference')
            restore=(temp/'desktop/config.toml').read_text(); app.close()
            (EVIDENCE/f'shortcuts-{PROFILE}-{BACKEND}-first-gui.log').write_text(app.logpath.read_text())
            before=state()['binds']
            app=tray.App(temp/'restored', socket=endpoint, address=os.environ['DBUS_SESSION_BUS_ADDRESS'], page='settings', config_text=restore)
            app.ready(); accessible=wait(own_app,'restored GTK application missing'); accessible.set_cache_mask(Atspi.Cache.NONE); accessible.clear_cache()
            wait(overlay_visible,'saved overlay preference did not restore compact window')
            click('Close overlay'); wait(lambda:not overlay_visible(),'restored overlay close failed')
            passed('overlay startup opt-in restores the compact window and closing persists opt-out')
            wait(lambda:label('Show OMEN Control: Meta+O'),'saved startup preference did not restore the shortcut')
            assert state()['binds']==before+1
            passed('saved opt-in restores a fresh approved session after application restart')
            app.quit(); wait(no_sessions,'restored Quit leaked session'); app.close()
            (EVIDENCE/f'shortcuts-{PROFILE}-{BACKEND}-restored-gui.log').write_text(app.logpath.read_text())
            control('Scenario','s','slow_registration'); before=state()['binds']
            minimized=restore.replace('start_minimized = false','start_minimized = true')
            assert minimized!=restore
            app=tray.App(temp/'minimized', socket=endpoint, address=os.environ['DBUS_SESSION_BUS_ADDRESS'], page='settings', config_text=minimized)
            app.ready(); wait(lambda:app.visible() is False,'startup preference did not hide the window')
            wait(lambda:state()['binds']>before,'minimized startup lost shortcut restoration')
            assert state()['parents'][-1]==''
            control('Fire','s','right'); wait(lambda:app.visible() is True,'restored shortcut did not show minimized startup window')
            accessible=wait(own_app,'minimized GTK application missing'); accessible.set_cache_mask(Atspi.Cache.NONE); accessible.clear_cache()
            wait(lambda:label('Show OMEN Control: Meta+O'),'minimized startup shortcut was not active')
            passed('slow portal startup restores the shortcut while minimized and activation reveals the window')
            result['portal_state']=state()
            result['observed_parent_backends']=sorted({p.split(':',1)[0] for p in state()['parents'] if p})
            result['gtk_display_type']=next(line.split(' ',1)[1] for line in app.text().splitlines() if line.startswith('OMEN_UI_BACKEND '))
            wait(lambda:any(item[0]=='Hide window' for item in app.labels()),'tray menu did not reflect the shown window')
            app.click('Hide window'); wait(lambda:app.visible() is False,'restart window could not hide')
            bus.terminate(); bus.wait(timeout=5)
            wait(lambda:any(label(text) for text in ['Desktop bus disconnected','shortcut service changed','desktop closed the shortcut session']), 'whole-bus loss was not reported')
            wait(lambda:app.visible() is True,'bus loss left the window hidden')
            assert app.child.poll() is None
            click('Disable global shortcut'); wait(lambda:label('Global shortcut disabled'),'offline disable failed')
            assert tomllib.loads((app.directory/'config.toml').read_text())['general'].get('global_shortcut_enabled',False) is False
            passed('whole-bus loss keeps GTK alive and permits saving startup opt-out while disconnected')
            result['result']='passed'
        except BaseException as error:
            result['error']=str(error)
            if app: result['gui_exit_code_before_cleanup']=app.child.poll()
            if 'walk' in locals(): result['operation_labels']=[n.get_name() for n in walk() if n.get_role_name()=='label']
            raise
        finally:
            if app:
                app.close()
                (EVIDENCE/f'shortcuts-{PROFILE}-{BACKEND}-gui.log').write_text(app.logpath.read_text())
            for child in reversed(processes):
                if child.poll() is None: child.terminate(); child.wait(timeout=8)
            for log in logs: log.close()
            for name in ['portal','daemon','bus','a11y']:
                (EVIDENCE/f'shortcuts-{PROFILE}-{BACKEND}-{name}.log').write_text((temp/(name+'.log')).read_text())
            result['cleanup_complete']=True
            (EVIDENCE/f'shortcuts-{PROFILE}.json').write_text(json.dumps(result,indent=2)+'\n')
            (EVIDENCE/f'shortcuts-{PROFILE}-{BACKEND}.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2))


if __name__=='__main__':
    portal() if '--portal' in sys.argv else main()
