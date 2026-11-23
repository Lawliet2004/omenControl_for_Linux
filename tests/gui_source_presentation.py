#!/usr/bin/env python3
"""One batched source-provenance presentation matrix, app-only captures."""
import hashlib
import json
import os
import socketserver
import threading

from PIL import Image
from native_desktop import Desktop, ROOT
from gui_control_recovery import ipc
from gui_presentation import measure

PROFILE=os.environ.get('OMEN_TEST_PROFILE','release')
OUT=__import__('pathlib').Path(os.environ['OMEN_TEST_EVIDENCE_DIR']).resolve()


def main():
    OUT.mkdir(parents=True,exist_ok=True)
    result={'result':'failed','fixture_only':True,'physical_hardware_writes':False,'cases':[],
        'gui_sha256':hashlib.file_digest((ROOT/'target'/PROFILE/'omen-gui').open('rb'),'sha256').hexdigest()}
    try:
        with Desktop(OUT/'process') as d:
            settings=d.root/'config/gtk-4.0'; settings.mkdir()
            (d.root/'user/config.toml').write_text('version=1\n[general]\nonboarding_dismissed=true\npoll_interval_ms=500\noverlay_enabled=true\n')
            service=d.launch([str(ROOT/'target'/PROFILE/'omen-daemon'),'--mock-omen'],'daemon',True)
            upstream=service.stdout.readline().strip()
            class Handler(socketserver.StreamRequestHandler):
                def handle(self):
                    while True:
                        try:
                            line=self.rfile.readline()
                            if not line: return
                            message=json.loads(line); reply=ipc(upstream,message)
                            if message['op']['op']=='get_info' and reply['ok']: reply['data']['mock']=False
                            self.wfile.write(json.dumps(reply).encode()+b'\n'); self.wfile.flush()
                        except (OSError,ValueError): return
            class Server(socketserver.ThreadingUnixStreamServer): daemon_threads=True
            proxy=Server(str(d.root/'unverified.sock'),Handler)
            threading.Thread(target=proxy.serve_forever,daemon=True).start()
            try:
                for source,endpoint,page in [('fixture',upstream,'settings'),('unverified',str(d.root/'unverified.sock'),'dashboard'),('unavailable',str(d.root/'absent.sock'),'settings')]:
                    for width,font,dark in [(1240,10,False),(600,20,True)]:
                        name=source+('-narrow-large-dark' if dark else '-wide-light')
                        (settings/'settings.ini').write_text('[Settings]\ngtk-font-name=Sans '+str(font)+'\n')
                        png,report,overlay=OUT/(name+'.png'),OUT/(name+'.json'),OUT/(name+'-overlay.png')
                        gui=d.launch([str(ROOT/'target'/PROFILE/'omen-gui'),'--new-instance','--page',page],name,
                            extra={'OMEN_SOCK':endpoint,'OMEN_CAPTURE_PATH':str(png),'OMEN_UI_REPORT_PATH':str(report),
                                'OMEN_CAPTURE_OVERLAY_PATH':str(overlay),'OMEN_CAPTURE_WIDTH':str(width),'OMEN_CAPTURE_HEIGHT':'1100',
                                'ADW_DEBUG_COLOR_SCHEME':'prefer-dark' if dark else 'prefer-light'})
                        gui.wait(timeout=25); assert gui.returncode==0
                        geometry=json.loads(report.read_text())
                        analysis=measure(geometry,Image.open(png).convert('RGBA'))
                        result['cases'].append({'name':name,'source':source,'page':page,'font':geometry['gtk_font_name'],
                            'capture':png.name,'report':report.name,'overlay_capture':overlay.name,**analysis})
                        print(name,'clipping',len(analysis['horizontal_clipping']),'contrast failures',len(analysis['contrast_failures']),flush=True)
            finally: proxy.shutdown(); proxy.server_close()
        result['cleanup_complete']=True
        result['result']='passed' if all(not c['horizontal_clipping'] and not c['collapsed_text_allocations'] and not c['contrast_failures'] for c in result['cases']) else 'issues-found'
    finally: (OUT/'source-presentation.json').write_text(json.dumps(result,indent=2)+'\n')


if __name__=='__main__': main()
