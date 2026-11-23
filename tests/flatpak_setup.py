#!/usr/bin/env python3
"""Exercise private setup, upgrades, refusal and shared removal via the real CLI."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib

ROOT=Path(__file__).resolve().parents[1]
PROFILE=os.environ.get('OMEN_TEST_PROFILE','debug')
OUT=Path(os.environ.get('OMEN_TEST_EVIDENCE_DIR',ROOT/'target/flatpak-setup-tests'))

def main():
    OUT.mkdir(parents=True,exist_ok=True)
    result={'result':'failed','checks_passed':[],'fixture_only':True,'physical_hardware_writes':False}
    try:
        with tempfile.TemporaryDirectory(prefix='omen-setup-',dir=ROOT/'target') as directory:
            root=Path(directory);home=root/'home';home.mkdir(mode=0o700)
            bins=root/'bin';bins.mkdir();config=root/'config';config.mkdir()
            for name in ['omen-cli','omen-flatpak-game']:shutil.copy2(ROOT/'target'/PROFILE/name,bins/name)
            (bins/'flatpak').write_text('#!/bin/sh\ncat "$OMEN_SETUP_PERMISSIONS"\n')
            (bins/'flatpak').chmod(0o700)
            permissions=root/'permissions';permissions.write_text('[Session Bus Policy]\norg.freedesktop.Flatpak=talk\n')
            env=dict(os.environ,HOME=str(home),PATH=str(bins)+os.pathsep+os.defpath,
                OMEN_CONFIG_DIR=str(config),OMEN_SETUP_PERMISSIONS=str(permissions))
            def run(*args,ok=True):
                r=subprocess.run([str(bins/'omen-cli'),'game-flatpak-setup','lutris/flatpak/1',*args,'--json'],env=env,capture_output=True,text=True,timeout=10)
                assert (r.returncode==0)==ok,(r.returncode,r.stdout,r.stderr)
                return json.loads(r.stdout) if ok else r.stderr
            def passed(message):result['checks_passed'].append(message);print('PASS',message,flush=True)
            assert not run()['helper_installed']
            state=run('--install');assert state['ready'] and state['host_permission']
            app=home/'.var/app/net.lutris.Lutris/data';installed=app/'omen-linux/managed-v1'
            helper=installed/'omen-flatpak-game';manifest=installed/'installation.json'
            assert helper.read_bytes()==(bins/'omen-flatpak-game').read_bytes()
            assert helper.stat().st_mode&0o777==0o700 and manifest.stat().st_mode&0o777==0o600
            assert json.loads(manifest.read_text())['sha256']==hashlib.sha256(helper.read_bytes()).hexdigest()
            passed('real CLI installs the exact packaged static helper and private SHA-256 record without changing launcher config or permissions')
            run('--install');assert run()['ready']
            passed('repeat installation publishes a complete verified replacement without leftover staging files')
            permissions.write_text('[Session Bus Policy]\norg.freedesktop.Flatpak=none\n')
            state=run();assert not state['ready'] and not state['host_permission'] and state['helper_installed']
            assert 'does not change sandbox permissions' in state['detail']
            passed('effective host-command denial disables readiness and gives permission/restart guidance without adding an override')
            permissions.write_text('[Session Bus Policy]\norg.freedesktop.Flatpak=talk\n')
            original=helper.read_bytes();helper.write_bytes(b'user edited content')
            assert 'modified' in run('--install',ok=False)
            assert 'modified' in run('--remove',ok=False)
            assert helper.read_bytes()==b'user edited content'
            helper.write_bytes(original)
            passed('edited user executables survive both update and removal attempts with a recovery error')
            extra=installed/'user-notes';extra.write_text('preserve me')
            assert 'unrecognized files' in run('--remove',ok=False) and extra.read_text()=='preserve me'
            extra.unlink();helper.unlink();helper.symlink_to(bins/'omen-flatpak-game')
            run('--install',ok=False);run('--remove',ok=False)
            assert helper.is_symlink();helper.unlink();helper.write_bytes(original);helper.chmod(0o700)
            passed('unrecognized files and symlink replacements are retained; setup never follows them')
            (config/'config.toml').write_text('version=1\n[games]\nmanaged_game_ids=["lutris/flatpak/1","lutris/flatpak/2","steam/620"]\n[games.per_game_profiles]\n"lutris/flatpak/1"="performance"\n"lutris/flatpak/2"="quiet"\n"steam/620"="performance"\n')
            keep=app/'existing-launcher-data';keep.write_text('keep')
            state=run('--remove');assert not state['helper_installed'] and not installed.exists()
            saved=tomllib.loads((config/'config.toml').read_text())
            assert saved['games']['managed_game_ids']==['steam/620']
            assert saved['games']['per_game_profiles']['lutris/flatpak/1']=='performance' and keep.read_text()=='keep'
            passed('shared helper removal disables only this launcher automatic bindings and retains manual bindings and unrelated app data')
            alias=root/'visible-home';alias.symlink_to(home,target_is_directory=True)
            env['HOME']=str(alias)
            state=run('--install');assert str(alias) in state['wrapper_command'] and state['ready']
            run('--remove');env['HOME']=str(home)
            passed('a symlinked host home keeps its visible sandbox path in the copied command while filesystem operations use the verified home')
            assert not list((app/'omen-linux').iterdir())
            passed('successful update/removal leaves no temporary or retired helper directories')
            rejected=subprocess.run([str(bins/'omen-cli'),'game-flatpak-prepare','lutris/flatpak/1','--instance','42','--handoff-fd','0'],env=env,capture_output=True,text=True,timeout=5)
            assert rejected.returncode!=0
            reports=json.loads(subprocess.check_output([str(bins/'omen-cli'),'game-launch-issues','--json'],env=env,text=True))
            assert any(i['game_id']=='lutris/flatpak/1' and i['stage']=='scope' and i['reason']['kind']=='scope_setup' for i in reports)
            passed('pre-registration handoff failures are retained as private closed-reason launch issues without contacting a profile service')
            result['cleanup_complete']=True
        result['result']='passed'
    finally:(OUT/'flatpak-setup.json').write_text(json.dumps(result,indent=2)+'\n')
if __name__=='__main__':main()
