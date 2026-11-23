#!/usr/bin/env python3
"""Capture only the Vitals widget tree, on read-only live and isolated fixture data."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT=Path(__file__).resolve().parents[1]
BIN=ROOT/'target/release'


def main():
    records=[]
    with tempfile.TemporaryDirectory(prefix='omen-network-capture-',dir=ROOT/'target') as directory:
        temp=Path(directory)
        env=dict(os.environ,GDK_BACKEND='wayland',GTK_A11Y='none',GSETTINGS_BACKEND='memory',OMEN_REPORT_READY='1',
                 OMEN_CONFIG_DIR=str(temp/'user'),OMEN_SYSTEM_CONFIG_DIR=str(temp/'system'),OMEN_HOME=str(temp/'games'),
                 OMEN_SOCK=str(temp/'missing.sock'),TMPDIR=str(temp),XDG_CONFIG_HOME=str(temp/'config'),XDG_DATA_HOME=str(temp/'data'),
                 XDG_CACHE_HOME=str(temp/'cache'))
        def capture(name,fixture):
            path=ROOT/'docs/screenshots'/name
            completed=subprocess.run([str(BIN/'omen-gui'),'--page','vitals'],env=dict(env,OMEN_CAPTURE_PATH=str(path)),
                                     check=True,capture_output=True,text=True,timeout=20)
            display=next(line.split(' ',1)[1] for line in completed.stderr.splitlines() if line.startswith('OMEN_UI_BACKEND '))
            assert display=='GdkWaylandDisplay'
            records.append({'path':str(path.relative_to(ROOT)),'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),
                            'fixture':fixture,'gtk_display_type':display})
        capture('vitals-network.png',False)
        with (temp/'daemon.log').open('w') as log:
            daemon=subprocess.Popen([str(BIN/'omen-daemon'),'--mock'],env=env,stdout=subprocess.PIPE,stderr=log,text=True)
            try:
                env['OMEN_SOCK']=daemon.stdout.readline().strip();assert Path(env['OMEN_SOCK']).name==f'omen-mock-{daemon.pid}.sock'
                capture('vitals-network-fixture.png',True)
            finally:
                daemon.terminate();assert daemon.wait(timeout=8)==0
    result={'method':'application-only GTK widget captures; no desktop framebuffer capture','hardware_control_writes':False,
            'host_network_policy_changed':False,'cleanup_complete':True,'screenshots':records,
            'gui_sha256':hashlib.sha256((BIN/'omen-gui').read_bytes()).hexdigest()}
    (ROOT/'docs/evidence/network-captures.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2))


if __name__=='__main__':main()
