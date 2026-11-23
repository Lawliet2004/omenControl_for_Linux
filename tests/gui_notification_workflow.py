#!/usr/bin/env python3
"""Native preference edits and actual delivery/actions on a private desktop bus."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import tomllib

from native_desktop import Desktop,ROOT

PROFILE=os.environ.get('OMEN_TEST_PROFILE','release')
OUT=Path(os.environ['OMEN_TEST_EVIDENCE_DIR']).resolve()
BIN=ROOT/'target'/PROFILE


def server():
    import gi
    gi.require_version('Gio','2.0')
    from gi.repository import Gio,GLib
    xml='''<node><interface name="org.freedesktop.Notifications">
    <method name="GetCapabilities"><arg type="as" direction="out"/></method>
    <method name="GetServerInformation"><arg type="s" direction="out"/><arg type="s" direction="out"/><arg type="s" direction="out"/><arg type="s" direction="out"/></method>
    <method name="Notify"><arg type="s" direction="in"/><arg type="u" direction="in"/><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="as" direction="in"/><arg type="a{sv}" direction="in"/><arg type="i" direction="in"/><arg type="u" direction="out"/></method>
    <method name="CloseNotification"><arg type="u" direction="in"/></method>
    <signal name="ActionInvoked"><arg type="u"/><arg type="s"/></signal>
    </interface></node>'''
    bus=Gio.bus_get_sync(Gio.BusType.SESSION,None)
    records=Path(os.environ['OMEN_NOTIFICATION_RECORDS']);action=Path(os.environ['OMEN_NOTIFICATION_ACTION'])
    count=0
    def method(_bus,sender,_path,_interface,name,parameters,invocation):
        nonlocal count
        if name=='GetCapabilities':invocation.return_value(GLib.Variant('(as)',(['body','actions'],)))
        elif name=='GetServerInformation':invocation.return_value(GLib.Variant('(ssss)',('Private fixture','OMEN tests','1','1.2')))
        elif name=='CloseNotification':invocation.return_value(None)
        elif name=='Notify':
            count+=1;values=parameters.unpack()
            with records.open('a') as output:
                output.write(json.dumps({'id':count,'sender':sender,'summary':values[3],'body':values[4],'actions':values[5],
                    'replaces_id':values[1],'received_monotonic':time.monotonic()})+'\n')
            invocation.return_value(GLib.Variant('(u)',(count,)))
    bus.register_object('/org/freedesktop/Notifications',Gio.DBusNodeInfo.new_for_xml(xml).interfaces[0],method,None,None)
    def dispatch():
        if action.exists():
            message=json.loads(action.read_text());action.unlink()
            bus.emit_signal(None,'/org/freedesktop/Notifications','org.freedesktop.Notifications','ActionInvoked',GLib.Variant('(us)',(message['id'],'default')))
        return True
    GLib.timeout_add(100,dispatch)
    Gio.bus_own_name_on_connection(bus,'org.freedesktop.Notifications',Gio.BusNameOwnerFlags.NONE,lambda *_:print('READY',flush=True),None)
    GLib.MainLoop().run()


def main():
    OUT.mkdir(parents=True,exist_ok=True)
    result={'result':'failed','fixture_only':True,'physical_hardware_writes':False,'host_keyboard_events':False,
        'real_desktop_notified':False,'profile':PROFILE,'binary_sha256':hashlib.file_digest((BIN/'omen-gui').open('rb'),'sha256').hexdigest(),
        'test_script_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'checks_passed':[]}
    try:
        with Desktop(OUT/'process') as d:
            config=d.root/'user/config.toml';config.write_text('version=1\n[general]\nonboarding_dismissed=true\npoll_interval_ms=500\n')
            original=config.read_bytes()
            records=d.root/'notifications.jsonl';action=d.root/'notification-action.json'
            receiver=d.launch([sys.executable,str(Path(__file__).resolve()),'--server'],'notifications',True,
                extra={'OMEN_NOTIFICATION_RECORDS':str(records),'OMEN_NOTIFICATION_ACTION':str(action)})
            assert receiver.stdout.readline().strip()=='READY'
            applications=d.root/'data/applications';applications.mkdir()
            (applications/'com.omenlinux.Gui.desktop').write_text((ROOT/'packaging/com.omenlinux.Gui.desktop').read_text().replace('Exec=omen-gui','Exec='+str(BIN/'omen-gui')))
            daemon=d.launch([str(BIN/'omen-daemon'),'--mock-omen'],'daemon',True)
            d.env['OMEN_SOCK']=daemon.stdout.readline().strip()
            fixture=next(p.parent.parent for p in (d.root/'tmp').glob('*/sys/class') if (p/'hwmon/hwmon0/pwm1').exists())
            temperatures=list((fixture/'sys/class/hwmon').glob('*/temp*_input'));assert temperatures
            gui=d.launch([str(BIN/'omen-gui'),'--page','settings'],'gui');app=d.attach(gui);d.focus_window()
            def nodes():return list(d.walk(app))
            def label(fragment):return any(n.get_role_name()=='label' and fragment in n.get_name() for n in nodes())
            def received():return [json.loads(line) for line in records.read_text().splitlines()] if records.exists() else []
            def focus(name):return d.tab_to(app,lambda n:n.get_name()==name,'native notification control unreachable: '+name,limit=100)
            def edit(name,value):
                focus(name);d.key('a','Control_L');d.type_text(str(value));d.key('Return')
            def save():focus('Save notification preferences');d.key('Return')
            def passed(text):result['checks_passed'].append(text);print('PASS',text,flush=True)
            def checked(node):return node.get_state_set().contains(d.Atspi.StateType.CHECKED)
            try:
                checkbox=focus('Notify when a temperature reaches the threshold')
                assert not checked(checkbox);d.drain(1);assert not received()
                passed('native notification checkbox is off by default and actual fresh fixture readings deliver no message')
                d.key('space');assert checked(checkbox)
                edit('Temperature threshold (°C)',40)
                edit('Rearm after cooling by (°C)',5)
                edit('Minimum time between notifications (seconds)',30)
                d.drain(1);assert config.read_bytes()==original and not received()
                passed('actual keyboard toggles and edits the request without changing stored preferences or enabling delivery before Save')
                save()
                d.wait(lambda:label('Notification preferences saved'),'save feedback absent')
                d.wait(lambda:len(received())==1,'new preferences did not take effect in the running GUI')
                stored=tomllib.loads(config.read_text())['general']['alerts']
                assert stored=={'enabled':True,'temperature_c':40,'hysteresis_c':5,'cooldown_seconds':30},stored
                assert not (d.root/'system/config.toml').exists()
                message=received()[0]
                assert 'MOCK' in message['body'] and 'not thermal protection' in message['body'] and '°C' in message['body']
                assert 'default' in message['actions']
                d.drain(1);assert len(received())==1
                passed('Save persists only user preferences, activates the current monitor, and delivers one fixture-labeled informational notification')
                d.key('1','Control_L')
                d.wait(lambda:label('Current profile:'),'notification action test did not leave Settings first')
                assert not label('Device and service health')
                action.write_text(json.dumps({'id':message['id']}))
                d.wait(lambda:label('Device and service health'),'actual notification action did not navigate to diagnostics')
                passed('the private server emits the real notification action signal and the running app opens Settings and Diagnostics')
                good=config.read_bytes();config.write_text('malformed = [')
                checkbox=focus('Notify when a temperature reaches the threshold');d.key('space');assert not checked(checkbox)
                save();d.wait(lambda:label('Notification preferences were not saved:'),'write failure did not surface')
                assert config.read_text()=='malformed = ['
                for path in temperatures:path.write_text('30000')
                d.drain(1.2)
                for path in temperatures:path.write_text('65000')
                d.wait(lambda:len(received())==2,'failed disable changed the previously active monitor or rearming failed',timeout=40)
                assert received()[1]['received_monotonic']-message['received_monotonic']>=30
                passed('a corrupt settings file remains untouched; failed disable preserves the active configuration and real cooldown/rearm still deliver correctly')
                config.write_bytes(good)
                save();d.wait(lambda:label('Notification preferences saved'),'save did not recover after repairing the file')
                assert tomllib.loads(config.read_text())['general']['alerts']['enabled'] is False
                for path in temperatures:path.write_text('30000')
                d.drain(1)
                for path in temperatures:path.write_text('70000')
                d.drain(max(0,received()[1]['received_monotonic']+31-time.monotonic()))
                d.drain(1.5);assert len(received())==2
                assert config.stat().st_mode & 0o777 == 0o600
                passed('repair and retry save the requested disabled state without service configuration or hardware-control writes')
                gui.terminate();gui.wait(timeout=10)
                gui=d.launch([str(BIN/'omen-gui'),'--page','settings'],'gui-restarted');app=d.attach(gui);d.focus_window()
                checkbox=focus('Notify when a temperature reaches the threshold')
                assert not checked(checkbox)
                d.drain(1.5);assert len(received())==2
                assert tomllib.loads(config.read_text())['general']['alerts']=={'enabled':False,'temperature_c':40,'hysteresis_c':5,'cooldown_seconds':30}
                passed('reopening the app retains disabled preferences and hot fresh readings remain silent on the new monitor')
                result['result']='passed'
            except BaseException as error:
                result['error']=type(error).__name__+': '+str(error);result['focus']=d.describe_focus(app)
                result['last_nodes']=[{'role':n.get_role_name(),'name':n.get_name()} for n in nodes()];raise
            finally:
                result['messages']=received();result['key_events']=d.keys
                (OUT/'notification-workflow.json').write_text(json.dumps(result,indent=2)+'\n')
        result['cleanup_complete']=True
    finally:(OUT/'notification-workflow.json').write_text(json.dumps(result,indent=2)+'\n')


if __name__=='__main__':server() if '--server' in sys.argv else main()
