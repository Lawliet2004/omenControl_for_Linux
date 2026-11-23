#!/usr/bin/env python3
"""Native control recovery against a private daemon and ordinary-file hardware.

The IPC proxy masks initial capabilities and can fail a lighting readback. All
other requests go to the real mock daemon; successful writes are verified in its
private fixture/configuration. XTEST and AT-SPI target only the private desktop.
"""
import copy
import hashlib
import json
import os
from pathlib import Path
import signal
import socket
import socketserver
import threading
import time

from native_desktop import Desktop, ROOT

PROFILE = os.environ.get('OMEN_TEST_PROFILE', 'debug')
GUI = Path(os.environ.get('OMEN_GUI_BIN', ROOT / 'target' / PROFILE / 'omen-gui'))
OUT = Path(os.environ['OMEN_TEST_EVIDENCE_DIR']).resolve()


def ipc(path, message):
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(8)
        stream.connect(str(path))
        stream.sendall(json.dumps(message).encode() + b'\n')
        return json.loads(stream.makefile('rb').readline())


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    result = {'result': 'failed', 'fixture_only': True, 'physical_hardware_writes': False,
        'host_keyboard_input': False, 'gui_sha256': hashlib.file_digest(GUI.open('rb'), 'sha256').hexdigest(),
        'test_script_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        'profile': PROFILE, 'checks_passed': []}
    (OUT / 'test-source.py').write_bytes(Path(__file__).read_bytes())
    writes, phase = [], {'initial': True, 'lighting_error': False}
    try:
        with Desktop(OUT / 'process') as d:
            (d.root / 'user/config.toml').write_text('version=1\n[general]\nonboarding_dismissed=true\npoll_interval_ms=500\n')
            service = d.launch([str(ROOT / 'target' / PROFILE / 'omen-daemon'), '--mock-omen'], 'daemon', True)
            endpoint = service.stdout.readline().strip()
            fixture = next(p.parent.parent for p in (d.root / 'tmp').glob('*/sys/class')
                if (p / 'hwmon/hwmon0/pwm1').exists())
            rgb = fixture / 'sys/devices/platform/omen-rgb-keyboard/rgb_zones'
            rgb.mkdir(parents=True)
            for name, value in {'zone00':'FF0000', 'zone01':'00FF00', 'brightness':'60', 'animation_mode':'static'}.items():
                (rgb / name).write_text(value)
            threshold = fixture / 'sys/class/power_supply/BAT0/charge_control_end_threshold'
            threshold.write_text('80')
            choices = fixture / 'sys/firmware/acpi/platform_profile_choices'
            curve_reply=ipc(endpoint, {'v':1,'id':1,'op':{'op':'get_fan_curve'}})
            assert curve_reply['ok']
            boundary_curve=curve_reply['data']
            boundary_curve['points'][0]['temp_c']=0
            boundary_curve['points'][-1]['temp_c']=110
            assert ipc(endpoint, {'v':1,'id':2,'op':{'op':'set_fan_curve','args':{'curve':boundary_curve}}})['ok']

            class Handler(socketserver.StreamRequestHandler):
                def handle(self):
                    while True:
                        try:
                            line=self.rfile.readline()
                        except OSError:
                            return
                        if not line:
                            return
                        try:
                            message = json.loads(line)
                            operation = message['op']['op']
                            if operation.startswith('set_'):
                                writes.append(copy.deepcopy(message['op']))
                            reply = ipc(endpoint, message)
                            if operation == 'get_capabilities' and phase['initial'] and reply['ok']:
                                for name in ['performance_profiles','fan_monitoring','fan_modes','fan_control','fan_curve','lighting_zones','charge_threshold','nvidia_telemetry']:
                                    reply['data'][name] = {'status':'unavailable','detail':'Recovery fixture: provider not ready'}
                                reply['data']['profile_choices'] = []
                                reply['data']['lighting_zone_count'] = None
                            elif operation == 'get_lighting' and phase['lighting_error']:
                                reply = {'v':1,'id':message['id'],'ok':False,'error':{'code':'io','message':'fixture lighting read failed'}}
                            self.wfile.write(json.dumps(reply).encode() + b'\n')
                            self.wfile.flush()
                        except (OSError, ValueError):
                            return
            class Server(socketserver.ThreadingUnixStreamServer):
                daemon_threads = True
            proxy = Server(str(d.root / 'control-proxy.sock'), Handler)
            threading.Thread(target=proxy.serve_forever, daemon=True).start()
            d.env['OMEN_SOCK'] = str(d.root / 'control-proxy.sock')
            gui = d.launch([str(GUI), '--new-instance', '--page', 'performance'], 'gui')
            app = d.attach(gui)
            d.focus_window()

            def nodes():
                return list(d.walk(app))
            def label(fragment):
                return any(n.get_role_name() == 'label' and fragment in n.get_name() for n in nodes())
            def find(name, roles=None):
                return next(n for n in nodes() if n.get_name() == name and (roles is None or n.get_role_name() in roles))
            def sensitive(node):
                return node.get_state_set().contains(d.Atspi.StateType.SENSITIVE)
            def activate(node):
                assert sensitive(node), node.get_name()
                action = node.get_action_iface()
                index = next(i for i in range(action.get_n_actions()) if action.get_action_name(i).lower() in ['click','press','activate'])
                assert action.do_action(index), node.get_name()
            def click(name):
                activate(d.wait(lambda: find(name, ['button','toggle button']), 'missing button ' + name))
            def navigate(name):
                number = {'Performance Control':'3', 'Fan & Thermal':'4', 'Lighting Studio':'5', 'Graphics':'6', 'Battery & Power':'7'}[name]
                d.key(number, 'Control_L')
                d.drain(.2)
            def slider(name):
                return d.wait(lambda: find(name, ['slider']), 'missing slider ' + name)
            def passed(name):
                result['checks_passed'].append(name)
                print('PASS', name, flush=True)
            def read(op):
                reply = ipc(endpoint, {'v':1,'id':1,'op':{'op':op}})
                assert reply['ok'], reply
                return reply['data']
            try:
                d.wait(lambda: label('Recovery fixture: provider not ready'), 'initial capability reason absent')
                assert not any(n.get_name() == 'Performance' and n.get_role_name() == 'toggle button' for n in nodes())
                assert not writes
                phase['initial'] = False
                performance = d.wait(lambda: find('Performance', ['toggle button']), 'profile choices did not recover after initial unavailability', timeout=20)
                d.wait(lambda: sensitive(performance), 'recovered profile remains disabled')
                click('Quiet')
                d.wait(lambda: read('get_profile') == 'quiet', 'recovered profile action did not reach the real fixture')
                passed('Performance choices recover from an empty first report and a user click reaches the actual fixture backend')
                choices.write_text('quiet balanced cool performance')
                cool = d.wait(lambda: find('Cool', ['toggle button']), 'new advertised profile did not appear')
                assert sensitive(cool)
                choices.write_text('quiet balanced performance')
                d.wait(lambda: not any(n.get_name() == 'Cool' and n.get_role_name() == 'toggle button' for n in nodes()), 'removed profile remains visible')
                passed('profile choices add and remove currently advertised options without restarting or submitting a write')

                navigate('Fan & Thermal')
                manual = slider('Manual fan duty (%)')
                assert manual.get_value_iface().set_current_value(65)
                assert sensitive(find('Apply manual speed', ['button']))
                expander = d.wait(lambda: find('Edit fan curve'), 'curve disclosure did not recover')
                activate(expander)
                spin = d.wait(lambda: find('Fan duty (%) for curve point 1', ['spin button']), 'curve readback did not populate editor')
                assert find('Temperature (°C) for curve point 1',['spin button']).get_value_iface().get_current_value()==0, 'valid lower curve boundary was clamped'
                assert find('Temperature (°C) for curve point 5',['spin button']).get_value_iface().get_current_value()==110, 'valid upper curve boundary was clamped'
                assert spin.get_value_iface().set_current_value(30)
                assert not sensitive(find('Activate curve mode', ['button']))
                click('Save curve')
                d.wait(lambda: read('get_fan_curve')['points'][0]['duty_pct'] == 30, 'curve edit did not persist')
                d.wait(lambda: sensitive(find('Activate curve mode', ['button'])), 'saved curve did not become eligible for activation')
                assert spin.get_value_iface().set_current_value(35)
                assert not sensitive(find('Activate curve mode', ['button'])), 'dirty curve can activate before the next snapshot'
                passed('manual and curve editors recover; saving reaches the real daemon and unsaved changes immediately block curve activation')

                navigate('Lighting Studio')
                d.wait(lambda: label('Current lighting: 2 zones'), 'lighting readback did not recover')
                brightness = slider('Keyboard brightness (%)')
                assert brightness.get_value_iface().get_current_value() == 60
                assert brightness.get_value_iface().set_current_value(75)
                phase['lighting_error'] = True
                d.wait(lambda: label('Current lighting unavailable:'), 'failed readback was not exposed')
                assert not sensitive(find('Apply keyboard lighting', ['button']))
                phase['lighting_error'] = False
                d.wait(lambda: sensitive(find('Apply keyboard lighting', ['button'])), 'lighting did not recover after a read failure')
                assert brightness.get_value_iface().get_current_value() == 75
                click('Apply keyboard lighting')
                d.wait(lambda: (rgb / 'brightness').read_text().strip() == '75', 'lighting did not reach ordinary fixture files')
                assert (rgb / 'animation_mode').read_text() == 'static'
                passed('lighting initializes from a valid fresh readback, preserves edits through read failure and applies the observed animation without resetting it')

                navigate('Battery & Power')
                d.wait(lambda: label('Current charge threshold: 80 %'), 'threshold readback did not recover')
                charge = slider('Charge threshold (%)')
                assert charge.get_value_iface().set_current_value(85)
                threshold.write_text('95')
                d.wait(lambda: label('Current charge threshold: 95 %'), 'threshold current state did not follow external change')
                assert charge.get_value_iface().get_current_value() == 85
                passed('charge threshold appears after startup, follows actual readback and preserves a distinct requested edit')
                before_writes = len(writes)
                service.send_signal(signal.SIGSTOP)
                try:
                    d.wait(lambda: label('Monitoring data expired'), 'stalled worker did not expire controls', timeout=20)
                    assert not sensitive(find('Apply threshold', ['button']))
                    navigate('Lighting Studio')
                    assert not sensitive(find('Apply keyboard lighting', ['button']))
                    navigate('Fan & Thermal')
                    assert not sensitive(find('Apply manual speed', ['button']))
                    assert manual.get_value_iface().get_current_value() == 65
                    assert spin.get_value_iface().get_current_value() == 35
                    navigate('Performance Control')
                    assert not sensitive(find('Performance', ['toggle button']))
                finally:
                    service.send_signal(signal.SIGCONT)
                d.wait(lambda: sensitive(find('Performance', ['toggle button'])), 'fresh capability recovery did not re-enable profiles', timeout=30)
                assert len(writes) == before_writes
                navigate('Battery & Power')
                d.wait(lambda: sensitive(find('Apply threshold', ['button'])), 'threshold did not recover')
                assert charge == slider('Charge threshold (%)') and charge.get_value_iface().get_current_value() == 85
                click('Apply threshold')
                d.wait(lambda: threshold.read_text().strip() == '85', 'requested threshold did not reach real fixture')
                passed('stalled daemon expires every dependent action; recovery retains native editors and edits without implicit writes')

                navigate('Lighting Studio')
                for name, value in {'zone02':'0000FF','zone03':'FFFFFF'}.items(): (rgb / name).write_text(value)
                d.wait(lambda: label('Current lighting: 4 zones'), 'new lighting layout did not appear')
                assert label('Lighting zone layout changed.')
                assert find('Keyboard zone 4 color')
                passed('a new zone layout refreshes matching controls and explains that current values were loaded')
                navigate('Graphics')
                d.wait(lambda: label('No PCI display controllers found.'), 'initial graphics inventory absent')
                gpu = fixture / 'sys/bus/pci/devices/0000:01:00.0'
                (gpu / 'power').mkdir(parents=True)
                for name, value in {'class':'0x030200','vendor':'0x10de','power/runtime_status':'active'}.items(): (gpu / name).write_text(value)
                d.wait(lambda: label('0000:01:00.0') and label('Runtime: active'), 'new GPU inventory did not appear')
                (gpu / 'power/runtime_status').write_text('suspended')
                d.wait(lambda: label('Runtime: suspended'), 'runtime power state did not refresh')
                (gpu / 'class').unlink()
                d.wait(lambda: label('Graphics inventory is incomplete'), 'partial inventory falsely reports no GPUs')
                passed('graphics inventory and runtime state refresh from actual fixture PCI files and incomplete discovery remains explicit')

                navigate('Performance Control')
                d.tab_to(app, lambda n:n.get_name()=='Balanced' and n.get_role_name()=='toggle button', 'profile choice unreachable by Tab')
                d.key('space')
                d.wait(lambda:read('get_profile')=='balanced', 'keyboard profile activation did not reach the fixture')
                passed('real private keyboard traversal and Space activate the focused profile choice')
                before_writes=len(writes)
                navigate('Fan & Thermal')
                d.tab_to(app, lambda n:n.get_name()=='Manual fan duty (%)', 'manual slider unreachable by Tab')
                d.key('Right')
                assert manual.get_value_iface().get_current_value()==70
                d.tab_to(app, lambda n:n.get_name()=='Fan duty (%) for curve point 1', 'expanded curve field unreachable by Tab', limit=40)
                d.key('Up')
                assert spin.get_value_iface().get_current_value()==40
                assert not sensitive(find('Activate curve mode',['button']))
                assert len(writes)==before_writes
                passed('real keyboard edits fan controls without writing and immediately disables unsaved curve activation')
                navigate('Lighting Studio')
                d.tab_to(app, lambda n:n.get_name()=='Keyboard zone 1 color', 'zone color picker unreachable by Tab')
                d.key('space')
                d.wait(lambda:any(n.get_role_name()=='dialog' for n in nodes()), 'native color dialog did not open from keyboard')
                d.key('Escape')
                d.wait(lambda:not any(n.get_role_name()=='dialog' for n in nodes()), 'native color dialog did not cancel')
                # The private rootful X server has no window manager to return
                # input focus when its transient dialog is destroyed.
                d.focus_window()
                d.tab_to(app, lambda n:n.get_name()=='Keyboard brightness (%)', 'brightness unreachable after color cancel')
                d.key('Right')
                assert slider('Keyboard brightness (%)').get_value_iface().get_current_value()==80
                assert len(writes)==before_writes
                passed('native keyboard opens and cancels the color dialog, then edits brightness without implicit writes')
                navigate('Battery & Power')
                d.tab_to(app, lambda n:n.get_name()=='Charge threshold (%)', 'threshold slider unreachable by Tab')
                d.key('Right')
                assert charge.get_value_iface().get_current_value()==90
                d.tab_to(app, lambda n:n.get_name()=='Apply threshold' and n.get_role_name()=='button', 'threshold Apply unreachable by Tab')
                d.key('Return')
                d.wait(lambda:threshold.read_text().strip()=='90', 'keyboard threshold Apply did not reach the fixture')
                passed('native keyboard threshold editing and explicit Apply reach ordinary fixture hardware with the requested value')
                result['result'] = 'passed'
            finally:
                if result['result'] != 'passed':
                    result['last_nodes'] = [{'role':n.get_role_name(),'name':n.get_name()} for n in nodes()]
                result['write_operations'] = writes
                gui.terminate()
                gui.wait(timeout=10)
                proxy.shutdown()
                proxy.server_close()
        result['cleanup_complete'] = True
    except BaseException as error:
        result['error'] = type(error).__name__ + ': ' + str(error)
        raise
    finally:
        (OUT / 'control-recovery.json').write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
