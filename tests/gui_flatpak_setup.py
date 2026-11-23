#!/usr/bin/env python3
"""Keyboard-driven Flatpak setup in a private desktop; no host Flatpak changes."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import tomllib

from native_desktop import Desktop, ROOT

PROFILE=os.environ.get('OMEN_TEST_PROFILE','debug')
BIN=ROOT/'target'/PROFILE
OUT=Path(os.environ.get('OMEN_TEST_EVIDENCE_DIR',ROOT/'target/flatpak-setup-ui'))

def main():
    os.umask(0o077)
    OUT.mkdir(parents=True,exist_ok=True)
    result={'result':'failed','checks_passed':[],'fixture_only':True,'physical_hardware_writes':False,
        'host_keyboard_events':False,'gui_sha256':hashlib.file_digest((BIN/'omen-gui').open('rb'),'sha256').hexdigest()}
    with Desktop(OUT/'process') as d:
        home=d.root/'home';home.mkdir();d.env['HOME']=str(home)
        catalog=home/'.var/app/net.lutris.Lutris/data/lutris';catalog.mkdir(parents=True)
        import sqlite3
        with sqlite3.connect(catalog/'pga.db') as db:
            db.execute('create table games (id integer, name text, installed integer)')
            db.execute('insert into games values (1, "Flatpak keyboard fixture", 1)')
        config=d.root/'user/config.toml'
        config.write_text('version=1\n[general]\nonboarding_dismissed=true\npoll_interval_ms=500\n[games.per_game_profiles]\n"lutris/flatpak/1"="performance"\n')
        bins=d.root/'bin';bins.mkdir();permission=d.root/'permission'
        permission.write_text('[Session Bus Policy]\norg.freedesktop.Flatpak=talk\n')
        (bins/'flatpak').write_text('#!/bin/sh\ncat "$OMEN_SETUP_PERMISSIONS"\n');(bins/'flatpak').chmod(0o700)
        d.env.update(PATH=str(bins)+os.pathsep+os.defpath,OMEN_SETUP_PERMISSIONS=str(permission))
        def passed(message):result['checks_passed'].append(message);print('PASS',message,flush=True)
        def named(name):return lambda n:n.get_name()==name and n.get_role_name()!='table cell'
        def visible(fragment):return any(fragment in n.get_name() and n.get_state_set().contains(d.Atspi.StateType.SHOWING) for n in d.walk(app))
        def click(name):d.tab_to(app,named(name),'keyboard action unreachable: '+name);d.key('Return')
        def ids():return tomllib.loads(config.read_text()).get('games',{}).get('managed_game_ids',[])
        try:
            service=d.launch([str(BIN/'omen-daemon'),'--mock-omen'],'daemon',True);d.env['OMEN_SOCK']=service.stdout.readline().strip()
            gui=d.launch([str(BIN/'omen-gui'),'--new-instance','--page','games'],'gui');app=d.attach(gui);d.focus_window()
            d.key('f','Control_L');d.type_text('lutris/flatpak/1')
            d.wait(lambda:sum(n.get_name()=='Profile setup' and n.get_role_name()=='button' for n in d.walk(app))==1,'search result absent')
            d.tab_to(app,named('Profile setup'),'setup disclosure unreachable');d.key('space')
            d.wait(lambda:any('Install the launcher helper' in n.get_name() for n in d.walk(app)),'setup result absent')
            passed('actual keyboard search/disclosure reaches Flatpak setup and its permission guidance')
            click('Install launcher helper')
            d.wait(lambda:visible('Helper and host-command permission are ready'),'helper installation did not settle')
            helper=home/'.var/app/net.lutris.Lutris/data/omen-linux/managed-v1/omen-flatpak-game'
            assert helper.read_bytes()==(BIN/'omen-flatpak-game').read_bytes()
            passed('keyboard installation copies the exact shipped helper without stalling GTK')
            click('Copy wrapper command');d.wait(lambda:visible('Wrapper command copied.'),'copy feedback absent')
            d.tab_to(app,named('Game wrapper command'),'wrapper text unreachable');d.key('Tab')
            assert d.describe_focus(app)['name']!='Game wrapper command'
            passed('copy action and named wrapper text are keyboard accessible and Tab escapes the read-only command')
            click('Enable automatic restore');d.wait(lambda:'lutris/flatpak/1' in ids(),'automatic mode was not saved')
            passed('keyboard enables automatic mode only after helper and permission readiness')
            permission.write_text('[Session Bus Policy]\norg.freedesktop.Flatpak=none\n')
            click('Check Flatpak setup');d.wait(lambda:visible('cannot use flatpak-spawn --host'),'denied permission not explained')
            click('Use manual restore');d.wait(lambda:'lutris/flatpak/1' not in ids(),'manual mode was not saved')
            button=next(n for n in d.walk(app) if n.get_role_name()=='button' and n.get_name()=='Enable automatic restore')
            # Returning to manual mode must retain the failed readiness gate.
            assert not button.get_state_set().contains(d.Atspi.StateType.SENSITIVE),'permission denial can be bypassed by toggling manual mode'
            passed('permission loss is recoverable and manual mode retains the unavailable automatic-mode gate')
            permission.write_text('[Session Bus Policy]\norg.freedesktop.Flatpak=talk\n')
            click('Check Flatpak setup');d.wait(lambda:visible('Helper and host-command permission are ready'),'permission recovery did not settle')
            click('Enable automatic restore');d.wait(lambda:'lutris/flatpak/1' in ids(),'reenable did not save')
            click('Remove launcher helper');d.wait(lambda:not helper.exists() and 'lutris/flatpak/1' not in ids(),'removal did not finish')
            assert visible('Remove its command prefix in the launcher')
            passed('keyboard recheck, reenable and shared removal recover without changing other app data')
            for cycle in range(3):
                d.key('f','Control_L');d.key('a','Control_L');d.key('BackSpace');d.type_text('no-matching-game')
                d.wait(lambda:not any(n.get_role_name()=='button' and n.get_name()=='Profile setup' for n in d.walk(app)),'filtered setup row did not detach')
                d.key('a','Control_L');d.key('BackSpace');d.type_text('lutris/flatpak/1')
                d.wait(lambda:any(n.get_role_name()=='button' and n.get_name()=='Install launcher helper' for n in d.walk(app)),'expanded setup did not rebind')
                click('Install launcher helper');d.wait(lambda:helper.exists() and visible('Helper and host-command permission are ready'),'recycled setup install did not finish')
                click('Remove launcher helper');d.wait(lambda:not helper.exists(),'recycled setup removal did not finish')
            passed('expanded setup detaches during filtering and remains interactive after three native row rebindings')
            gui.terminate();gui.wait(timeout=8)
            result['keys']=d.keys;result['result']='passed'
        finally:
            if result['result']!='passed':result['failure_tree']=[{'role':n.get_role_name(),'name':n.get_name()} for n in d.walk(locals().get('app'))]
            (OUT/'keyboard.json').write_text(json.dumps(result,indent=2)+'\n')
    result['cleanup_complete']=True;(OUT/'keyboard.json').write_text(json.dumps(result,indent=2)+'\n')
if __name__=='__main__':main()
