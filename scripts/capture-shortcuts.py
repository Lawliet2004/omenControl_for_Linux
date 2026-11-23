#!/usr/bin/env python3
"""Capture only GTK Settings, using an isolated shortcut portal fixture."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / 'target/release'
sys.path.insert(0, str(ROOT / 'tests'))
import tray
import gui_settings


def main():
    records = []
    with tempfile.TemporaryDirectory(prefix='omen-shortcut-capture-', dir=ROOT / 'target') as directory:
        temp = Path(directory)
        env = dict(os.environ, GDK_BACKEND='wayland', GTK_A11Y='none', GSETTINGS_BACKEND='memory',
                   OMEN_CONFIG_DIR=str(temp/'user'), OMEN_SYSTEM_CONFIG_DIR=str(temp/'system'),
                   OMEN_HOME=str(temp/'games'), XDG_DATA_HOME=str(temp/'data'), XDG_CONFIG_HOME=str(temp/'config'),
                   XDG_CACHE_HOME=str(temp/'cache'), TMPDIR=str(temp), OMEN_REPORT_READY='1')
        for name in ['user','system','data/applications']:
            (temp/name).mkdir(parents=True)
        entry=(ROOT/'packaging/com.omenlinux.Gui.desktop').read_text().replace('Exec=omen-gui','Exec='+str(BIN/'omen-gui'))
        (temp/'data/applications/com.omenlinux.Gui.desktop').write_text(entry)
        config=temp/'bus.conf'
        config.write_text(f'<busconfig><type>session</type><listen>unix:path={temp}/bus.sock</listen><auth>EXTERNAL</auth><policy context="default"><allow user="{os.getuid()}"/><allow own="*"/><allow send_destination="*"/><allow receive_type="method_call"/><allow receive_type="method_return"/><allow receive_type="error"/><allow receive_type="signal"/></policy></busconfig>')
        children=[]
        def launch(command, name):
            log=(temp/(name+'.log')).open('w')
            child=subprocess.Popen(command,env=env,stdout=subprocess.PIPE,stderr=log,text=True)
            children.append((child,log))
            return child
        try:
            bus=launch(['dbus-daemon','--config-file='+str(config),'--nofork','--print-address=1'],'bus')
            env['DBUS_SESSION_BUS_ADDRESS']=bus.stdout.readline().strip()
            portal=launch([sys.executable,str(ROOT/'tests/shortcuts.py'),'--portal'],'portal')
            owner=portal.stdout.readline().strip();assert owner.startswith(':')
            watcher=launch([sys.executable,str(ROOT/'tests/tray.py'),'--watcher'],'watcher')
            assert watcher.stdout.readline().strip()=='READY'
            daemon=launch([str(BIN/'omen-daemon'),'--mock'],'daemon')
            env['OMEN_SOCK']=daemon.stdout.readline().strip();assert gui_settings.call(env['OMEN_SOCK'],'get_info')['mock']
            for enabled, name in [(False,'settings-shortcut-disabled-fixture.png'),(True,'settings-shortcut-enabled-fixture.png')]:
                (temp/'user/config.toml').write_text(f'version = 1\n[general]\nstart_minimized = false\nglobal_shortcut_enabled = {str(enabled).lower()}\n')
                path=ROOT/'docs/screenshots'/name
                run=subprocess.run([str(BIN/'omen-gui'),'--page','settings'],env=dict(env,OMEN_CAPTURE_PATH=str(path)),capture_output=True,text=True,check=True,timeout=20)
                assert 'OMEN_UI_BACKEND GdkWaylandDisplay' in run.stderr
                deadline=time.monotonic()+5
                while True:
                    reply=tray.bus('call',owner,'/com/omenlinux/Test','com.omenlinux.Test','State',env=env)
                    state=json.loads(reply['data'][0])
                    if not state['sessions']:break
                    assert time.monotonic()<deadline,'capture left a shortcut session'
                    time.sleep(.05)
                assert state['binds']==int(enabled)
                records.append({'path':str(path.relative_to(ROOT)),'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),
                                'shortcut_enabled':enabled,'portal_fixture_bind_count':state['binds'],'display_type':'GdkWaylandDisplay'})
        finally:
            for child,log in reversed(children):
                if child.poll() is None:child.terminate();child.wait(timeout=8)
                log.close()
    result={'fixture_only':True,'host_shortcuts_changed':False,'hardware_writes':False,'cleanup_complete':True,
            'method':'application-only GTK snapshot with private portal/tray/daemon fixtures',
            'gui_sha256':hashlib.sha256((BIN/'omen-gui').read_bytes()).hexdigest(),'screenshots':records}
    (ROOT/'docs/evidence/shortcut-captures.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2))


if __name__=='__main__':
    main()
