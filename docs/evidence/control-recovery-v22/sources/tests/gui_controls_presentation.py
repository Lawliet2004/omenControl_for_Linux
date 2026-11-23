#!/usr/bin/env python3
"""Bounded app-only control page scenes with fresh ordinary-file fixtures."""
import hashlib
import json
import os
from pathlib import Path

from PIL import Image
from gui_presentation import measure
from native_desktop import Desktop, ROOT
from gui_control_recovery import ipc

PROFILE = os.environ.get('OMEN_TEST_PROFILE', 'release')
OUT = Path(os.environ['OMEN_TEST_EVIDENCE_DIR']).resolve()


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    result = {'result':'failed','fixture_only':True,'physical_hardware_writes':False,
        'gui_sha256':hashlib.file_digest((ROOT/'target'/PROFILE/'omen-gui').open('rb'),'sha256').hexdigest(),
        'cases':[]}
    try:
        with Desktop(OUT/'process') as d:
            settings = d.root/'config/gtk-4.0'
            settings.mkdir()
            (d.root/'user/config.toml').write_text('version=1\n[general]\nonboarding_dismissed=true\npoll_interval_ms=500\n')
            service = d.launch([str(ROOT/'target'/PROFILE/'omen-daemon'),'--mock-omen'],'daemon',True)
            d.env['OMEN_SOCK'] = service.stdout.readline().strip()
            curve=ipc(d.env['OMEN_SOCK'],{'v':1,'id':1,'op':{'op':'get_fan_curve'}})['data']
            curve['points'][0]['temp_c']=0
            curve['points'][-1]['temp_c']=110
            assert ipc(d.env['OMEN_SOCK'],{'v':1,'id':2,'op':{'op':'set_fan_curve','args':{'curve':curve}}})['ok']
            fixture = next(p.parent.parent for p in (d.root/'tmp').glob('*/sys/class') if (p/'hwmon/hwmon0/pwm1').exists())
            rgb = fixture/'sys/devices/platform/omen-rgb-keyboard/rgb_zones'
            rgb.mkdir(parents=True)
            for name,value in {'zone00':'FF0044','zone01':'22CC99','brightness':'60','animation_mode':'static'}.items(): (rgb/name).write_text(value)
            (fixture/'sys/class/power_supply/BAT0/charge_control_end_threshold').write_text('80')
            gpu = fixture/'sys/bus/pci/devices/0000:01:00.0'
            (gpu/'power').mkdir(parents=True)
            for name,value in {'class':'0x030200','vendor':'0x10de','power/runtime_status':'suspended'}.items(): (gpu/name).write_text(value)
            matrix = [(page, font, width, dark, bottom) for page in ['performance','fan','lighting','graphics','battery']
                for font,width,dark,bottom in [(10,1240,False,False),(20,600,True,False)]]
            matrix += [('fan',10,1240,False,True),('fan',20,600,True,True)]
            for page,font,width,dark,bottom in matrix:
                name = page + ('-narrow-large-dark' if dark else '-wide-light') + ('-curve-bottom' if bottom else '')
                (settings/'settings.ini').write_text('[Settings]\ngtk-font-name=Sans '+str(font)+'\n')
                png, report_path = OUT/(name+'.png'), OUT/(name+'.json')
                gui = d.launch([str(ROOT/'target'/PROFILE/'omen-gui'),'--new-instance','--page',page],name,
                    extra={'OMEN_CAPTURE_PATH':str(png),'OMEN_UI_REPORT_PATH':str(report_path),
                        'OMEN_CAPTURE_WIDTH':str(width),'OMEN_CAPTURE_HEIGHT':'1100',
                        'ADW_DEBUG_COLOR_SCHEME':'prefer-dark' if dark else 'prefer-light'})
                app = d.attach(gui)
                if page == 'fan':
                    expander = d.wait(lambda:next(n for n in d.walk(app) if n.get_name()=='Edit fan curve'), 'curve disclosure absent')
                    action = expander.get_action_iface()
                    assert action.do_action(next(i for i in range(action.get_n_actions()) if action.get_action_name(i) in ['activate','click','press']))
                    d.drain(.15)
                    if bottom:
                        for node in d.walk(app):
                            if node.get_role_name()=='scroll bar' and node.get_state_set().contains(d.Atspi.StateType.VERTICAL) and node.get_state_set().contains(d.Atspi.StateType.SHOWING):
                                value=node.get_value_iface(); value.set_current_value(value.get_maximum_value())
                gui.wait(timeout=20)
                assert gui.returncode==0
                report=json.loads(report_path.read_text())
                assert report['gtk_font_name']=='Sans '+str(font) and report['dark']==dark
                analysis=measure(report,Image.open(png).convert('RGBA'))
                result['cases'].append({'name':name,'page':page,'font':report['gtk_font_name'],'window_size':report['window_size'],
                    'capture':png.name,'report':report_path.name,'capture_sha256':hashlib.file_digest(png.open('rb'),'sha256').hexdigest(),**analysis})
                print(name, 'clipping',len(analysis['horizontal_clipping']),'contrast failures',len(analysis['contrast_failures']),flush=True)
        result['cleanup_complete']=True
        result['result']='passed' if all(not c['horizontal_clipping'] and not c['collapsed_text_allocations'] and not c['contrast_failures'] for c in result['cases']) else 'issues-found'
    finally:
        (OUT/'controls-presentation.json').write_text(json.dumps(result,indent=2)+'\n')


if __name__=='__main__':
    main()
