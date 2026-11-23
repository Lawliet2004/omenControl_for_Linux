#!/usr/bin/env python3
"""Sensor groups/filter/sort/identity and real private keyboard copy workflows."""
import copy
import hashlib
import json
import os
from pathlib import Path
import socketserver
import subprocess
import threading
import time

from native_desktop import Desktop, ROOT
from gui_control_recovery import ipc

PROFILE=os.environ.get('OMEN_TEST_PROFILE','debug')
OUT=Path(os.environ['OMEN_TEST_EVIDENCE_DIR']).resolve()
GUI=ROOT/'target'/PROFILE/'omen-gui'


def sample(identifier,label,kind,value,device=None,stale=False):
    return {'id':identifier,'label':label,'kind':kind,'value':value,'text':None,
        'source':'mock','device_id':device,'measured_unix_ms':1 if stale else 0}


def main():
    OUT.mkdir(parents=True,exist_ok=True)
    result={'result':'failed','fixture_only':True,'physical_hardware_writes':False,'host_keyboard_events':False,
        'gui_sha256':hashlib.file_digest(GUI.open('rb'),'sha256').hexdigest(),'checks_passed':[]}
    readings=[sample('gpu/a','Zeta GPU temperature','temperature',40,'0000:03:00.0'),
        sample('gpu/b','Alpha GPU temperature','temperature',80,'0000:03:00.0'),
        sample('gpu/power','GPU power','power',12,'0000:03:00.0'),
        sample('cpu/temp','CPU package','temperature',50),sample('fan/speed','CPU fan','fan_rpm',1200),
        sample('missing','Unavailable current','current',None),sample('stale','Old temperature','temperature',999,stale=True)]
    lock=threading.Lock(); phase={'empty':False}
    with Desktop(OUT/'process') as d:
        (d.root/'user/config.toml').write_text('version=1\n[general]\nonboarding_dismissed=true\npoll_interval_ms=500\n')
        daemon=d.launch([str(ROOT/'target'/PROFILE/'omen-daemon'),'--mock-omen'],'daemon',True)
        upstream=daemon.stdout.readline().strip()
        class Handler(socketserver.StreamRequestHandler):
            def handle(self):
                while True:
                    try:
                        line=self.rfile.readline()
                        if not line:return
                        message=json.loads(line)
                        if message['op']['op']=='get_sensors':
                            with lock: data=[] if phase['empty'] else copy.deepcopy(readings)
                            for reading in data:
                                if reading['measured_unix_ms']==0:reading['measured_unix_ms']=int(time.time()*1000)
                            reply={'id':message['id'],'ok':True,'data':data}
                        else:reply=ipc(upstream,message)
                        self.wfile.write(json.dumps(reply).encode()+b'\n');self.wfile.flush()
                    except (OSError,ValueError):return
        class Server(socketserver.ThreadingUnixStreamServer):daemon_threads=True
        proxy=Server(str(d.root/'sensors.sock'),Handler);threading.Thread(target=proxy.serve_forever,daemon=True).start()
        d.env['OMEN_SOCK']=str(d.root/'sensors.sock')
        # Log actual Orca utterances without a Speech Dispatcher or host input.
        assert not subprocess.run(['pgrep','-u',str(os.getuid()),'-x','orca'],capture_output=True).stdout
        orca=d.launch(['/usr/bin/orca','--debug-file',str(d.root/'orca-debug.log')],'orca')
        d.drain(1)
        gui=d.launch([str(GUI),'--new-instance','--page','vitals'],'gui');app=d.attach(gui);d.focus_window()
        def nodes():return list(d.walk(app))
        def label(fragment):return any(n.get_role_name()=='label' and fragment in n.get_name() for n in nodes())
        def named(name,roles=None):return next(n for n in nodes() if n.get_name()==name and (roles is None or n.get_role_name() in roles))
        def prefix(text):return next(n for n in nodes() if n.get_role_name()=='label' and n.get_name().startswith(text))
        def search(text):
            d.key('f','Control_L');d.key('a','Control_L');d.key('BackSpace');d.type_text(text);d.drain(.35)
        def focus(name):return d.tab_to(app,lambda n:n.get_name()==name,'native control unreachable: '+name,limit=90)
        def combo(name,index):
            d.key('f','Control_L');focus(name);d.key('space')
            if name=='Device':
                d.type_text('0000:03:00.0');d.drain(.4)
                result.setdefault('device_search_observations',[]).append({'focus':d.describe_focus(app),
                    'text':d.Atspi.Text.get_text(d.focused(app).get_text_iface(),0,-1)})
                d.key('Down')
            else:
                d.key('Home')
                for _ in range(index):d.key('Down')
            d.key('Return');d.drain(.2)
        def clear():
            d.key('f','Control_L');focus('Clear sensor filters');d.key('Return');d.drain(.2)
        def passed(text):result['checks_passed'].append(text);print('PASS',text,flush=True)
        try:
            d.wait(lambda:label('Device 0000:03:00.0 · Temperatures'),'reported device/type group missing')
            assert label('mock · device not reported · Temperatures')
            assert label('Device 0000:03:00.0 · Power')
            assert prefix('Old temperature: ').get_name()=='Old temperature: Stale'
            passed('reported device/type groups and explicit unknown-device groups retain raw IDs; stale numbers are replaced by Stale')
            search('gpu 0000:03')
            d.wait(lambda:label('Showing 3 of 7 sensor readings.'),'search did not intersect device and name')
            combo('Type',1)
            d.wait(lambda:label('Showing 2 of 7 sensor readings.'),'keyboard type filter failed')
            combo('Order',1)
            order=[n.get_name() for n in nodes() if n.get_role_name()=='label' and n.get_name().startswith(('Alpha GPU temperature:','Zeta GPU temperature:'))]
            assert order[0].startswith('Alpha'),order
            passed('real keyboard combines search/type filtering and sorts comparable values within their device/type group')
            clear();combo('Device',2)
            d.wait(lambda:label('Showing 3 of 7 sensor readings.'),'reported device filter failed')
            combo('Availability',2)
            d.wait(lambda:label('No sensors match these filters.'),'empty filter state missing')
            clear();combo('Availability',2)
            d.wait(lambda:label('Showing 2 of 7 sensor readings.'),'unavailable/stale filter failed')
            assert not label('Alpha GPU temperature:')
            passed('device and availability filters compose, explain an empty result and Clear sensor filters restores defaults')
            clear();search('gpu/b')
            alpha=d.wait(lambda:prefix('Alpha GPU temperature:'),'stable row absent')
            d.tab_to(app,lambda n:n==alpha,'sensor reading unreachable by keyboard')
            d.key('a','Control_L');assert alpha.get_text_iface().get_n_selections()==1
            old=alpha.get_name()
            with lock:readings[1]['value']=85
            d.wait(lambda:label('Sensor reading held for selection.'),'copy snapshot did not explain held data')
            d.drain(.8)
            assert alpha.get_name()==old and d.focused(app)==alpha
            assert alpha.get_text_iface().get_n_selections()==1
            d.key('Right')
            d.wait(lambda:alpha.get_name().endswith('85.0 °C'),'clearing selection did not resume readings')
            passed('keyboard copy holds the named reading and focus across acquisitions; clearing selection resumes the current value')
            search('')
            alpha=d.wait(lambda:prefix('Alpha GPU temperature:'),'alpha missing after clearing search')
            with lock:
                readings.insert(0,sample('gpu/first','Aardvark GPU temperature','temperature',20,'0000:03:00.0'))
            d.wait(lambda:label('Aardvark GPU temperature:'),'new sensor absent')
            assert prefix('Alpha GPU temperature:')==alpha
            with lock:next(r for r in readings if r['id']=='gpu/b')['device_id']='0000:04:00.0'
            d.wait(lambda:prefix('Alpha GPU temperature:')!=alpha,'changed device retained stale row identity')
            passed('inserting a preceding sensor preserves native identity; a changed reported device replaces it')
            combo('Device',2)
            with lock:
                readings[:]=[r for r in readings if r.get('device_id')!='0000:03:00.0']
            d.wait(lambda:label('0000:03:00.0 (not reporting)'),'missing selected device was silently retargeted')
            d.wait(lambda:label('No sensors match these filters.'),'missing selected device did not stay selected')
            passed('a disappeared selected device remains explicitly selected and marked not reporting')
            clear();search('gpu/b')
            alpha=d.wait(lambda:prefix('Alpha GPU temperature:'),'second device row missing')
            d.tab_to(app,lambda n:n==alpha,'new row unreachable');d.key('a','Control_L')
            with lock:phase['empty']=True
            d.wait(lambda:label('No sensor readings available.'),'missing report did not remove sensor rows')
            assert not label('Alpha GPU temperature:')
            with lock:phase['empty']=False
            d.wait(lambda:label('Alpha GPU temperature:'),'sensors did not recover')
            passed('missing data removes held rows and a recovered report repopulates the current search')
            began=time.monotonic()
            with lock:
                readings[:]=[sample(f'load/{i:04d}',f'Load fixture sensor {i:04d}','temperature',i%100,'0000:06:00.0') for i in range(1000)]
            search('load/0999')
            d.wait(lambda:label('Showing 1 of 1000 sensor readings.'),'1000-reading search did not resolve the final identifier',timeout=30)
            assert prefix('Load fixture sensor 0999:').get_name().endswith('99.0 °C')
            result['thousand_reading_observation']={'elapsed_seconds':round(time.monotonic()-began,3),
                'vm_rss_kib':int(next(line.split()[1] for line in Path('/proc/'+str(gui.pid)+'/status').read_text().splitlines() if line.startswith('VmRSS:')))}
            passed('a 1000-reading report remains searchable through the real keyboard and resolves the final raw identifier')
            speech_path=d.root/'orca-debug.log'
            d.wait(lambda:any('Type' in line for line in speech_path.read_text().splitlines() if 'SPEECH OUTPUT:' in line),'Orca did not name the sensor filter')
            result['speech_output']=[line.split('SPEECH OUTPUT:',1)[1].strip() for line in speech_path.read_text().splitlines() if 'SPEECH OUTPUT:' in line]
            passed('Orca generates native filter names during actual private keyboard traversal; no audible synthesis is claimed')
            result['result']='passed'
        except BaseException as error:
            result['error']=type(error).__name__+': '+str(error)
            result['last_nodes']=[{'role':n.get_role_name(),'name':n.get_name()} for n in nodes()]
            result['focus']=d.describe_focus(app);raise
        finally:
            result['key_events']=d.keys
            gui.terminate();gui.wait(timeout=10);orca.terminate();orca.wait(timeout=10);proxy.shutdown();proxy.server_close()
            (OUT/'sensor-browser.json').write_text(json.dumps(result,indent=2)+'\n')
    result['cleanup_complete']=True;(OUT/'sensor-browser.json').write_text(json.dumps(result,indent=2)+'\n')


if __name__=='__main__':main()
