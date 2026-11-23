#!/usr/bin/env python3
"""Bounded sensor-browser scenes on private fixture IPC and private Xwayland."""
import copy
import hashlib
import json
import os
from pathlib import Path
import socketserver
import threading
import time

from PIL import Image
from native_desktop import Desktop,ROOT
from gui_control_recovery import ipc
from gui_sensor_browser import sample
from gui_presentation import measure

PROFILE=os.environ.get('OMEN_TEST_PROFILE','release')
OUT=Path(os.environ['OMEN_TEST_EVIDENCE_DIR']).resolve()


def main():
    OUT.mkdir(parents=True,exist_ok=True)
    result={'result':'failed','fixture_only':True,'physical_hardware_writes':False,'cases':[],
        'gui_sha256':hashlib.file_digest((ROOT/'target'/PROFILE/'omen-gui').open('rb'),'sha256').hexdigest()}
    readings=[sample('gpu/temperature','GPU temperature','temperature',58,'0000:03:00.0'),
        sample('gpu/power','GPU power','power',17,'0000:03:00.0'),sample('cpu/temperature','CPU package temperature','temperature',62),
        sample('unknown','Unavailable provider current','current',None),sample('stale','Previous provider temperature','temperature',90,stale=True),
        sample('long/provider-identifier-'+'x'*100,'A long provider label that remains readable when the device name and sensor identifier wrap','temperature',51,'0000:05:00.0-'+('reported-device-'*10))]
    with Desktop(OUT/'process') as d:
        settings=d.root/'config/gtk-4.0';settings.mkdir()
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
                            data=copy.deepcopy(readings)
                            for sensor in data:
                                if sensor['measured_unix_ms']==0:sensor['measured_unix_ms']=int(time.time()*1000)
                            reply={'id':message['id'],'ok':True,'data':data}
                        else:reply=ipc(upstream,message)
                        self.wfile.write(json.dumps(reply).encode()+b'\n');self.wfile.flush()
                    except (OSError,ValueError):return
        class Server(socketserver.ThreadingUnixStreamServer):daemon_threads=True
        proxy=Server(str(d.root/'sensors.sock'),Handler);threading.Thread(target=proxy.serve_forever,daemon=True).start()
        d.env['OMEN_SOCK']=str(d.root/'sensors.sock')
        try:
            for name,width,font,dark,contrast,mode in [('wide-light',1240,10,False,False,'all'),
                    ('narrow-large-dark',600,20,True,False,'all'),('wide-high-contrast',1240,20,False,True,'all'),
                    ('narrow-long-device',600,20,False,False,'long'),('wide-no-results',1240,10,False,False,'empty')]:
                (settings/'settings.ini').write_text('[Settings]\ngtk-font-name=Sans '+str(font)+'\n')
                png,report=OUT/(name+'.png'),OUT/(name+'.json')
                gui=d.launch([str(ROOT/'target'/PROFILE/'omen-gui'),'--new-instance','--page','vitals'],name,
                    extra={'OMEN_CAPTURE_PATH':str(png),'OMEN_UI_REPORT_PATH':str(report),'OMEN_CAPTURE_WIDTH':str(width),'OMEN_CAPTURE_HEIGHT':'1100',
                        'ADW_DEBUG_COLOR_SCHEME':'prefer-dark' if dark else 'prefer-light','ADW_DEBUG_HIGH_CONTRAST':'1' if contrast else '0'})
                app=d.attach(gui);d.focus_window();d.key('f','Control_L')
                if mode=='empty':d.type_text('no-sensor-found',paced=False)
                if mode=='long':
                    d.tab_to(app,lambda n:n.get_name()=='Device','Device picker not reachable for long-identifier scene',limit=4)
                    d.key('space');d.type_text('0000:05',paced=False);d.drain(.35);d.key('Down');d.key('Return')
                gui.wait(timeout=20);assert gui.returncode==0
                geometry=json.loads(report.read_text());analysis=measure(geometry,Image.open(png).convert('RGBA'))
                assert geometry['gtk_font_name']=='Sans '+str(font) and geometry['dark']==dark
                result['cases'].append({'name':name,'font':geometry['gtk_font_name'],'width':width,'high_contrast':geometry.get('high_contrast'),
                    'capture':png.name,'report':report.name,**analysis})
                print(name,'clipping',len(analysis['horizontal_clipping']),'contrast failures',len(analysis['contrast_failures']),flush=True)
        finally:proxy.shutdown();proxy.server_close()
    result['cleanup_complete']=True
    result['result']='passed' if all(not case['horizontal_clipping'] and not case['collapsed_text_allocations'] and not case['contrast_failures'] for case in result['cases']) else 'issues-found'
    (OUT/'sensor-presentation.json').write_text(json.dumps(result,indent=2)+'\n')


if __name__=='__main__':main()
