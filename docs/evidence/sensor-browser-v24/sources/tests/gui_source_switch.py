#!/usr/bin/env python3
"""Late service/replacement workflows, private IPC/desktop and fixture files only."""
import hashlib
import json
import os
from pathlib import Path
import socketserver
import threading

from native_desktop import Desktop, ROOT
from gui_control_recovery import ipc

PROFILE = os.environ.get('OMEN_TEST_PROFILE', 'debug')
OUT = Path(os.environ['OMEN_TEST_EVIDENCE_DIR']).resolve()
GUI = ROOT/'target'/PROFILE/'omen-gui'


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    result = {'result':'failed', 'fixture_only':True, 'physical_hardware_writes':False,
        'host_keyboard_input':False, 'checks_passed':[],
        'gui_sha256':hashlib.file_digest(GUI.open('rb'),'sha256').hexdigest(),
        'test_script_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
    proxies, writes = [], []
    try:
        with Desktop(OUT/'process') as d:
            (d.root/'user/config.toml').write_text('version=1\n[general]\nonboarding_dismissed=true\npoll_interval_ms=500\n')
            endpoint = d.root/'selected.sock'
            d.env['OMEN_SOCK'] = str(endpoint)
            gui = d.launch([str(GUI),'--new-instance','--page','settings'],'gui')
            app = d.attach(gui)
            d.focus_window()
            def nodes(parent=None): return list(d.walk(parent or app))
            def find(name, roles=None, parent=None):
                return next(n for n in nodes(parent) if n.get_name()==name and (roles is None or n.get_role_name() in roles))
            def label(fragment): return any(n.get_role_name()=='label' and fragment in n.get_name() for n in nodes())
            def sensitive(n): return n.get_state_set().contains(d.Atspi.StateType.SENSITIVE)
            def activate(n):
                assert sensitive(n), n.get_name()
                action=n.get_action_iface()
                assert action.do_action(next(i for i in range(action.get_n_actions()) if action.get_action_name(i).lower() in ['click','press','activate']))
            def click(name, parent=None): activate(d.wait(lambda:find(name,['button','toggle button'],parent),'missing '+name))
            def page(number): d.key(str(number),'Control_L'); d.drain(.15)
            def value(name): return d.wait(lambda:find(name,['slider','spin button']),'missing '+name).get_value_iface()
            def passed(text): result['checks_passed'].append(text); print('PASS',text,flush=True)
            def fixture(name):
                before=set((d.root/'tmp').glob('*/sys/class'))
                daemon=d.launch([str(ROOT/'target'/PROFILE/'omen-daemon'),'--mock-omen'],name,True)
                upstream=daemon.stdout.readline().strip()
                tree=next(p.parent.parent for p in set((d.root/'tmp').glob('*/sys/class'))-before if (p/'hwmon/hwmon0/pwm1').exists())
                rgb=tree/'sys/devices/platform/omen-rgb-keyboard/rgb_zones'; rgb.mkdir(parents=True)
                for field,content in {'zone00':'FF0000','zone01':'00FF00','brightness':'60','animation_mode':'static'}.items(): (rgb/field).write_text(content)
                (tree/'sys/class/power_supply/BAT0/charge_control_end_threshold').write_text('80')
                return upstream, tree
            def proxy(upstream, name):
                phase={'fail':False, 'mock':True}
                class Handler(socketserver.StreamRequestHandler):
                    def handle(self):
                        while True:
                            try:
                                line=self.rfile.readline()
                                if not line: return
                                message=json.loads(line); op=message['op']['op']
                                if op.startswith('set_'): writes.append({'generation':name,'op':op})
                                if phase['fail']:
                                    reply={'id':message['id'],'ok':False,'error':{'code':'daemon_unavailable','message':'temporary source outage'}}
                                else:
                                    reply=ipc(upstream,message)
                                    if op=='get_info' and reply['ok']: reply['data']['mock']=phase['mock']
                                self.wfile.write(json.dumps(reply).encode()+b'\n'); self.wfile.flush()
                            except (OSError,ValueError): return
                class Server(socketserver.ThreadingUnixStreamServer): daemon_threads=True
                server=Server(str(endpoint),Handler)
                threading.Thread(target=server.serve_forever,daemon=True).start()
                proxies.append(server)
                return server, phase
            try:
                d.wait(lambda:label('Source: monitoring source unavailable'),'private missing source did not stay unavailable')
                assert not sensitive(find('Import service policies…',['button']))
                assert not sensitive(find('Export service policies…',['button']))
                passed('missing explicit endpoint stays unavailable and never falls back to host hardware')
                upstream, tree = fixture('first-daemon')
                first, phase = proxy(upstream,'first')
                d.wait(lambda:label('Source: MOCK — fixture data, no real hardware'),'late daemon was not discovered')
                d.wait(lambda:sensitive(find('Import service policies…',['button'])),'service tools did not enable after reconnect')
                passed('a late mock service reconnects without restarting GTK and enables service backup tools')
                page(4)
                d.wait(lambda:label('Automatic (firmware)'),'fan source did not load')
                value('Manual fan duty (%)').set_current_value(65)
                activate(d.wait(lambda:find('Edit fan curve'),'curve disclosure absent'))
                value('Fan duty (%) for curve point 1').set_current_value(35)
                phase['fail']=True
                d.wait(lambda:not sensitive(find('Apply manual speed',['button'])),'outage left manual control enabled')
                assert value('Manual fan duty (%)').get_current_value()==65
                phase['fail']=False
                d.wait(lambda:sensitive(find('Apply manual speed',['button'])),'same source did not recover')
                assert value('Manual fan duty (%)').get_current_value()==65
                assert value('Fan duty (%) for curve point 1').get_current_value()==35
                passed('same-process outage disables writes and reconnect preserves unsaved manual and curve edits')
                page(5)
                d.wait(lambda:label('Current lighting: 2 zones'),'lighting not loaded')
                value('Keyboard brightness (%)').set_current_value(75)
                page(7)
                d.wait(lambda:label('Current charge threshold: 80 %'),'threshold not loaded')
                value('Charge threshold (%)').set_current_value(85)
                page(9)
                backup=d.root/'service-backup.json'
                click('Export service policies…')
                dialog=d.wait(lambda:find('Export service policies…',['dialog']),'save dialog absent')
                entry=d.wait(lambda:next(n for n in nodes(dialog) if n.get_role_name()=='text' and n.get_name() in ['Name','Name:']),'save name field absent')
                assert entry.get_editable_text_iface().set_text_contents(str(backup))
                click('Save',dialog)
                d.wait(backup.exists,'service backup was not exported')
                d.focus_window()
                click('Import service policies…')
                dialog=d.wait(lambda:find('Import service policies…',['dialog']),'open dialog absent')
                row=d.wait(lambda:next(n for n in nodes(dialog) if n.get_role_name()=='table row' and n.get_name().startswith(backup.name+' ')),'export absent from recent files')
                current=row
                while current is not None:
                    parent=current.get_parent()
                    if parent is not None and parent.get_selection_iface() is not None:
                        assert parent.get_selection_iface().select_child(current.get_index_in_parent()); break
                    current=parent
                d.wait(lambda:row.get_state_set().contains(d.Atspi.StateType.SELECTED),'backup not selected')
                click('Open',dialog)
                d.wait(lambda:find('Apply policies',['button']),'service preview absent')
                second_upstream, second_tree=fixture('second-daemon')
                first.shutdown()
                endpoint.unlink()
                second, second_phase=proxy(second_upstream,'second')
                first.server_close()
                # Allow a complete fresh report to migrate the UI while the
                # existing native preview remains open.
                d.drain(2)
                before=list(writes)
                click('Apply policies')
                d.wait(lambda:label('service endpoint changed'),'old preview was not rejected after service replacement')
                assert writes==before, writes
                assert not (d.root/'system/config.toml').exists()
                d.focus_window()
                passed('an open service-policy preview stays bound to the original peer and sends no mutation to its replacement')
                page(4)
                d.wait(lambda:value('Manual fan duty (%)').get_current_value()==50,'new source inherited old manual edit')
                activate(d.wait(lambda:find('Edit fan curve'),'curve disclosure absent after replacement')) if not any(n.get_name()=='Fan duty (%) for curve point 1' and n.get_role_name()=='spin button' for n in nodes()) else None
                d.wait(lambda:value('Fan duty (%) for curve point 1').get_current_value()!=35,'new source inherited old curve edit')
                page(5)
                d.wait(lambda:value('Keyboard brightness (%)').get_current_value()==60,'new source inherited old lighting edit')
                page(7)
                d.wait(lambda:value('Charge threshold (%)').get_current_value()==80,'new source inherited old threshold edit')
                assert not writes
                passed('a replacement source reloads manual, curve, lighting and threshold editors without an implicit write')
                page(9)
                second_phase['mock']=False
                d.wait(lambda:label('Source: service source unverified'),'user-owned nonmock endpoint received physical provenance')
                assert not label('Source: omen-daemon + local read-only GPU fallback')
                page(1)
                d.wait(lambda:label('Source: service source unverified'),'Dashboard provenance did not refresh')
                assert label('source: service source unverified')
                second_phase['mock']=True
                d.wait(lambda:label('Source: MOCK — fixture data, no real hardware'),'Dashboard did not return to fixture provenance')
                passed('same-socket metadata changes update Dashboard and sidebar labels; untrusted nonmock service remains unverified')
                result['result']='passed'
            finally:
                if result['result']!='passed': result['last_nodes']=[{'role':n.get_role_name(),'name':n.get_name()} for n in nodes()]
                result['write_operations']=writes
                gui.terminate(); gui.wait(timeout=10)
                for server in proxies: server.shutdown(); server.server_close()
        result['cleanup_complete']=True
    except BaseException as error:
        result['error']=type(error).__name__+': '+str(error); raise
    finally:
        (OUT/'source-switch.json').write_text(json.dumps(result,indent=2)+'\n')


if __name__=='__main__': main()
