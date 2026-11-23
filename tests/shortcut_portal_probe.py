#!/usr/bin/env python3
"""Observe the real desktop's shortcut session API without binding/configuring keys."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import gi
gi.require_version('Gio', '2.0')
from gi.repository import Gio, GLib

ROOT = Path(__file__).resolve().parents[1]
DEST = 'org.freedesktop.portal.Desktop'
PATH = '/org/freedesktop/portal/desktop'
IFACE = 'org.freedesktop.portal.GlobalShortcuts'
APP_ID = 'com.omenlinux.Gui'


def main():
    assert os.geteuid() != 0
    result = {'result': 'unavailable', 'uid': os.geteuid(), 'hardware_writes': False,
              'bind_or_configure_requested': False,
              'scope': 'independent real-portal Register/CreateSession/ListShortcuts/Close probe; no key assignment or activation test',
              'script_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              'installed_desktop_entry': any(p.exists() for p in [Path('/usr/share/applications')/(APP_ID+'.desktop'), Path.home()/'.local/share/applications'/(APP_ID+'.desktop')]),
              'responses': []}
    address = Gio.dbus_address_get_for_bus_sync(Gio.BusType.SESSION, None)
    connection = Gio.DBusConnection.new_for_address_sync(address, Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION, None, None)
    connection.set_exit_on_close(False)
    session = None
    owner = None
    def call(destination, path, interface, method, args):
        return connection.call_sync(destination, path, interface, method, args, None, Gio.DBusCallFlags.NONE, 5000, None)
    try:
        owner = call('org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus', 'GetNameOwner', GLib.Variant('(s)', (DEST,))).unpack()[0]
        call(owner, PATH, 'org.freedesktop.host.portal.Registry', 'Register', GLib.Variant('(sa{sv})', (APP_ID, {})))
        result['version'] = call(owner, PATH, 'org.freedesktop.DBus.Properties', 'Get', GLib.Variant('(ss)', (IFACE, 'version'))).unpack()[0]
        encoded = connection.get_unique_name()[1:].replace('.', '_')
        counter = [0]
        def request(method, values):
            counter[0] += 1
            token = f'omen_probe_{os.getpid()}_{counter[0]}'
            path = f'{PATH}/request/{encoded}/{token}'
            options = {'handle_token': GLib.Variant('s', token)}
            if method == 'CreateSession':
                options['session_handle_token'] = GLib.Variant('s', f'omen_probe_{os.getpid()}')
                args = GLib.Variant('(a{sv})', (options,))
            else:
                args = GLib.Variant('(oa{sv})', (values, options))
            loop = GLib.MainLoop()
            response, timed_out = [], [False]
            def receive(_c, _s, _p, _i, _m, params):
                response.append(params.unpack())
                loop.quit()
            subscription = connection.signal_subscribe(owner, 'org.freedesktop.portal.Request', 'Response', path, None, Gio.DBusSignalFlags.NONE, receive)
            def timeout():
                timed_out[0] = True; loop.quit(); return False
            timer = GLib.timeout_add_seconds(15, timeout)
            try:
                reply = call(owner, PATH, IFACE, method, args).unpack()[0]
                assert reply == path
                if not response: loop.run()
                assert response and not timed_out[0], 'portal session request timed out'
                code, data = response[0]
                result['responses'].append({'method': method, 'code': code, 'result_fields': sorted(data)})
                assert code == 0, f'{method} declined: {code} {data}'
                return data
            finally:
                connection.signal_unsubscribe(subscription)
                if not timed_out[0]: GLib.source_remove(timer)
                if not response:
                    try: call(owner, path, 'org.freedesktop.portal.Request', 'Close', None)
                    except GLib.Error: pass
        created = request('CreateSession', None)
        session = created['session_handle']
        assert session == f'{PATH}/session/{encoded}/omen_probe_{os.getpid()}'
        listed = request('ListShortcuts', session)
        result['previous_shortcut_ids'] = [item[0] for item in listed['shortcuts']]
        result['result'] = 'passed'
    except BaseException as error:
        result['error'] = str(error)
    finally:
        if session is not None:
            try:
                call(owner, session, 'org.freedesktop.portal.Session', 'Close', None)
                result['session_closed'] = True
            except GLib.Error as error:
                result['close_error'] = str(error)
        connection.close_sync(None)
        result['connection_closed'] = True
        result['recorded_utc'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        (ROOT/'docs/evidence/shortcut-portal-host.json').write_text(json.dumps(result, indent=2)+'\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
