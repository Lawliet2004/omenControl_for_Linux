#!/usr/bin/env python3
"""Real managed CLI, private PPD, inherited descriptors and foreground PTYs."""
import fcntl
import hashlib
import json
import os
from pathlib import Path
import pty
import signal
import subprocess
import tempfile
import termios
import time

from game_formats import call, wait

ROOT=Path(__file__).resolve().parents[1]
PROFILE=os.environ.get('OMEN_TEST_PROFILE','release')
CLI=ROOT/'target'/PROFILE/'omen-cli'
HELPER=Path(os.environ['OMEN_NATIVE_GAME_TEST_BIN'])
OUT=Path(os.environ['OMEN_TEST_EVIDENCE_DIR'])

def main():
    OUT.mkdir(parents=True,exist_ok=True)
    result={'result':'failed','fixture_only':True,'physical_hardware_writes':False,'system_power_bus_used':False,
        'checks_passed':[],'cli_sha256':hashlib.file_digest(CLI.open('rb'),'sha256').hexdigest()}
    children=[];logs=[];ptys=[]
    try:
        with tempfile.TemporaryDirectory(prefix='omen-job-control-',dir=ROOT/'target') as folder:
            root=Path(folder);user=root/'user';user.mkdir();system=root/'system';system.mkdir()
            env=dict(os.environ,OMEN_CONFIG_DIR=str(user),OMEN_SYSTEM_CONFIG_DIR=str(system))
            (user/'config.toml').write_text('version=1\n[games]\nmanaged_game_ids=["custom/job"]\n[games.per_game_profiles]\n"custom/job"="performance"\n')
            def start(command,name,pipe=False,**kwargs):
                log=(root/(name+'.log')).open('w');logs.append(log)
                p=subprocess.Popen(list(map(str,command)),env=env,stdout=subprocess.PIPE if pipe else log,stderr=log,text=True,**kwargs);children.append(p);return p
            bus=start(['dbus-daemon','--session','--nofork','--print-address=1'],'bus',True);address=bus.stdout.readline().strip()
            ppd=start([HELPER,'ppd-service',address],'ppd',True);assert ppd.stdout.readline().strip()=='READY'
            endpoint=root/'control.sock';service=start([HELPER,'fixture-service',endpoint,root/'sessions.json',address,'mock'],'daemon',True)
            assert service.stdout.readline().strip()=='READY';env['OMEN_SOCK']=str(endpoint)
            def passed(message):result['checks_passed'].append(message);print('PASS',message,flush=True)
            def held():return any(s['profile_held'] for s in call(endpoint,'get_game_sessions')['sessions'])
            def released():return not held() and call(endpoint,'get_profile')=='balanced'
            script=root/'game.py';marker=root/'identity.json';done=root/'done';descriptor=root/'descriptor'
            script.write_text('import json,os,pathlib,signal,sys,time\n'
                'p=pathlib.Path(sys.argv[1]); data={"pid":os.getpid(),"pgid":os.getpgrp(),"tty":os.isatty(0)}\n'
                'if len(sys.argv)>3:data["descriptor"]=os.read(int(sys.argv[3]),128).decode()\n'
                'if data["tty"]:data["foreground"]=os.tcgetpgrp(0)\n'
                'p.write_text(json.dumps(data))\n'
                'while not pathlib.Path(sys.argv[2]).exists():time.sleep(.05)\n')
            descriptor.write_text('inherited descriptor bytes')
            with descriptor.open('rb') as fd:
                p=start([CLI,'game-run','custom/job','--','python3',script,marker,done,str(fd.fileno())],'descriptor',pass_fds=(fd.fileno(),))
                wait(marker.exists,'managed command did not inherit its file descriptor');identity=json.loads(marker.read_text());assert identity['descriptor']=='inherited descriptor bytes'
                wait(held,'descriptor command has no hold');done.touch();assert p.wait(timeout=10)==0;wait(released,'descriptor exit leaked profile')
            passed('an inherited nonstandard descriptor reaches the actual command and profile ownership ends after exit')
            marker.unlink();done.unlink();master,slave=pty.openpty();ptys.extend([master,slave])
            def terminal():os.setsid();fcntl.ioctl(slave,termios.TIOCSCTTY,0)
            p=subprocess.Popen([str(CLI),'game-run','custom/job','--','python3',str(script),str(marker),str(done)],env=env,
                stdin=slave,stdout=slave,stderr=slave,preexec_fn=terminal);children.append(p)
            wait(marker.exists,'managed command did not inherit the foreground terminal');identity=json.loads(marker.read_text())
            assert identity['tty'] and identity['pgid']==identity['foreground']
            wait(held,'terminal command has no hold')
            os.killpg(identity['pgid'],signal.SIGSTOP)
            wait(lambda:Path('/proc/'+str(identity['pid'])+'/status').read_text().split('State:')[1].lstrip().startswith('T'),'game did not stop')
            assert held();os.killpg(identity['pgid'],signal.SIGCONT)
            wait(lambda:not Path('/proc/'+str(identity['pid'])+'/status').read_text().split('State:')[1].lstrip().startswith('T'),'game did not resume')
            done.touch();assert p.wait(timeout=10)==0;wait(released,'resumed exit leaked profile')
            passed('foreground terminal identity survives managed wrapping; stopping/resuming the process group retains its hold until normal exit')
            result['terminal_identity']=identity;result['result']='passed'
    finally:
        for p in reversed(children):
            if p.poll() is None:
                p.send_signal(signal.SIGCONT);p.terminate();p.wait(timeout=8)
        for fd in ptys:os.close(fd)
        for stream in logs:stream.close()
        result['cleanup_complete']=True;(OUT/'job-control.json').write_text(json.dumps(result,indent=2)+'\n')
if __name__=='__main__':main()
