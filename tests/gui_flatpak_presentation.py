#!/usr/bin/env python3
"""Bounded expanded Flatpak setup scenes, with measured text and layout."""
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import subprocess

from PIL import Image
from gui_presentation import measure
from native_desktop import Desktop, ROOT

PROFILE=os.environ.get('OMEN_TEST_PROFILE','release')
BIN=ROOT/'target'/PROFILE
OUT=Path(os.environ['OMEN_TEST_EVIDENCE_DIR']).resolve()

def main():
    os.umask(0o077);OUT.mkdir(parents=True,exist_ok=True)
    result={'result':'failed','fixture_only':True,'physical_hardware_writes':False,
        'gui_sha256':hashlib.file_digest((BIN/'omen-gui').open('rb'),'sha256').hexdigest(),'cases':[]}
    try:
        with Desktop(OUT/'process') as d:
            home=d.root/'home';home.mkdir();d.env['HOME']=str(home)
            settings=d.root/'config/gtk-4.0';settings.mkdir()
            catalog=home/'.var/app/net.lutris.Lutris/data/lutris';catalog.mkdir(parents=True)
            with sqlite3.connect(catalog/'pga.db') as db:
                db.execute('create table games (id integer, name text, installed integer)')
                db.execute('insert into games values (1,"A Flatpak game with a long descriptive name for setup and recovery",1)')
            (d.root/'user/config.toml').write_text('version=1\n[general]\nonboarding_dismissed=true\npoll_interval_ms=500\n[games.per_game_profiles]\n"lutris/flatpak/1"="performance"\n')
            bins=d.root/'bin';bins.mkdir();permission=d.root/'permissions'
            (bins/'flatpak').write_text('#!/bin/sh\ncat "$OMEN_SETUP_PERMISSIONS"\n');(bins/'flatpak').chmod(0o700)
            d.env.update(PATH=str(bins)+os.pathsep+os.defpath,OMEN_SETUP_PERMISSIONS=str(permission))
            service=d.launch([str(BIN/'omen-daemon'),'--mock-omen'],'daemon',True);d.env['OMEN_SOCK']=service.stdout.readline().strip()
            for case,font,width,dark,hc,bottom in [
                ('setup-light',10,1240,False,False,False),
                ('setup-dark',10,1240,True,False,True),
                ('permission-high-contrast',10,1240,False,True,True),
                ('setup-large-narrow',20,600,False,False,True),
                ('recovery-large-dark',20,600,True,False,True),
            ]:
                permission.write_text('[Session Bus Policy]\norg.freedesktop.Flatpak='+('none' if hc else 'talk')+'\n')
                helper=home/'.var/app/net.lutris.Lutris/data/omen-linux/managed-v1/omen-flatpak-game'
                if case=='setup-dark':subprocess.run([str(BIN/'omen-cli'),'game-flatpak-setup','lutris/flatpak/1','--install'],env=d.env,check=True,capture_output=True)
                if case=='recovery-large-dark':helper.write_bytes(b'edited user file')
                (settings/'settings.ini').write_text('[Settings]\ngtk-font-name=Sans '+str(font)+'\n')
                png,report=OUT/(case+'.png'),OUT/(case+'.json')
                gui=d.launch([str(BIN/'omen-gui'),'--new-instance','--page','games'],case,extra={
                    'OMEN_CAPTURE_PATH':str(png),'OMEN_UI_REPORT_PATH':str(report),'OMEN_CAPTURE_WIDTH':str(width),
                    'OMEN_CAPTURE_HEIGHT':'1100','OMEN_CAPTURE_DELAY_SECONDS':'12','ADW_DEBUG_COLOR_SCHEME':'prefer-dark' if dark else 'prefer-light',
                    'ADW_DEBUG_HIGH_CONTRAST':'1' if hc else '0'})
                app=d.attach(gui)
                search=d.wait(lambda:next(n for n in d.walk(app) if n.get_name()=='Search games' and n.get_role_name() in ['text','entry']),'search absent')
                search.get_editable_text_iface().set_text_contents('lutris/flatpak/1')
                node=d.wait(lambda:next(n for n in d.walk(app) if n.get_name()=='Profile setup' and n.get_role_name()=='button'),'setup button absent')
                assert node.get_action_iface().do_action(0)
                d.wait(lambda:any('Check Flatpak setup'==n.get_name() for n in d.walk(app)),'setup did not expand')
                d.drain(.4)
                if bottom:
                    for n in d.walk(app):
                        if n.get_role_name()=='scroll bar' and n.get_state_set().contains(d.Atspi.StateType.VERTICAL) and n.get_state_set().contains(d.Atspi.StateType.SHOWING):
                            value=n.get_value_iface();value.set_current_value(value.get_maximum_value())
                d.park_pointer()
                gui.wait(timeout=20);assert gui.returncode==0
                data=json.loads(report.read_text());assert data['dark']==dark and data['high_contrast']==hc
                wrapper=next(w for w in data['widgets'] if w['kind']=='text_view' and 'omen-flatpak-game' in w['text'])
                import re
                actual=float(re.search(r'([0-9.]+)(?:px)?$',wrapper['font']).group(1))
                points=actual*72/96 if wrapper['font'].endswith('px') else actual
                assert abs(points-font)<.02,wrapper['font']
                analysis=measure(data,Image.open(png).convert('RGBA'))
                result['cases'].append({'name':case,'capture':png.name,'report':report.name,'capture_sha256':hashlib.file_digest(png.open('rb'),'sha256').hexdigest(),**analysis})
                print(case,'clipping',len(analysis['horizontal_clipping']),'contrast',len(analysis['contrast_failures']),flush=True)
        result['cleanup_complete']=True
        result['result']='passed' if all(not c['horizontal_clipping'] and not c['contrast_failures'] and not c['collapsed_text_allocations'] for c in result['cases']) else 'issues-found'
    finally:(OUT/'presentation.json').write_text(json.dumps(result,indent=2)+'\n')
if __name__=='__main__':main()
