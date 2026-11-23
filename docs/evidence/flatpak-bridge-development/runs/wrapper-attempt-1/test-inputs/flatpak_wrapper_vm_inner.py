#!/usr/bin/env python3
"""Real Flatpak HostCommand handoff; a disposable guest and private PPD only.

Temporarily overlays the development CLI in this guest, then restores the
installed package bytes. The root service keeps its installed sandbox/polkit.
The fixture app is explicitly local; it is neither a store launcher nor a game.
"""
import hashlib
import json
import os
from pathlib import Path
import pwd
import re
import shutil
import socket
import subprocess
import time

RESULT=Path('/var/tmp/omen-vm-result.json')
BASE=Path('/run/omen-flatpak-wrapper-fixture')
HELPER=Path('/usr/local/libexec/omen-managed-fixture')
DEVCLI=Path('/var/tmp/omen-flatpak-bridge-cli')
WRAPPER=Path('/var/tmp/omen-flatpak-bridge-wrapper')
APP='org.omenlinux.WrapperProbe'
PLATFORM='org.omenlinux.WrapperPlatform'
SOCKET=Path('/run/omen-linux/ctl.sock')
JOURNAL=Path('/etc/omen-linux/managed-game-sessions.json')
OVERRIDE=Path('/run/systemd/system/omen-linux.service.d')
RULE=Path('/etc/polkit-1/rules.d/00-omen-flatpak-wrapper.rules')
CLI=Path('/usr/bin/omen-cli')

def run(*args,check=True,timeout=45):
    r=subprocess.run(list(map(str,args)),capture_output=True,text=True,timeout=timeout)
    print('+',args,r.returncode,r.stdout.strip(),r.stderr.strip(),flush=True)
    if check:r.check_returncode()
    return r

def wait(predicate,message,timeout=25):
    until=time.monotonic()+timeout
    while time.monotonic()<until:
        try:
            value=predicate()
            if value:return value
        except (OSError,ValueError,subprocess.SubprocessError):pass
        time.sleep(.1)
    raise RuntimeError(message)

def sha(path):
    with path.open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()

def main():
    assert os.geteuid()==0 and run('systemd-detect-virt','--vm').stdout.strip() in ['qemu','kvm']
    assert all(p.is_file() for p in [RESULT,HELPER,DEVCLI,WRAPPER,CLI])
    assert all(not p.exists() for p in [BASE,OVERRIDE,RULE,JOURNAL])
    account=pwd.getpwnam('ubuntu');home=Path(account.pw_dir)
    config_dir=home/'.config/omen-linux'
    created=[home/'.local/share/flatpak',home/'.cache/flatpak',home/'.var/app'/APP,config_dir]
    assert all(not p.exists() for p in created),'wrapper test requires a fresh guest user installation'
    state=json.loads(RESULT.read_text())
    data={'result':'failed','checks_passed':[],'physical_hardware_writes':False,
        'real_game_tested':False,'real_store_launcher_tested':False,'local_fixture_only':True,
        'production_root_service_sandbox':True,'production_polkit':True,'private_ppd':True,
        'development_cli_sha256':sha(DEVCLI),'installed_cli_before_sha256':sha(CLI),
        'wrapper_sha256':sha(WRAPPER),'helper_sha256':sha(HELPER),'source_sha256':sha(Path(__file__))}
    BASE.mkdir(mode=0o755)
    env={'HOME':str(home),'LC_ALL':'C.UTF-8','XDG_RUNTIME_DIR':'/run/user/1000',
        'DBUS_SESSION_BUS_ADDRESS':'unix:path=/run/user/1000/bus'}
    children=[];logs=[];instances=set();overlaid=False;helper_was_active=None
    def args(*command):
        return ['runuser','-u','ubuntu','--','env',*[k+'='+v for k,v in env.items()],*map(str,command)]
    def user(*command,check=True):return run(*args(*command),check=check)
    def start(command,name,as_user=True):
        log=(BASE/(name+'.log')).open('w');logs.append(log)
        p=subprocess.Popen(args(*command) if as_user else command,stdout=log,stderr=subprocess.STDOUT,text=True)
        children.append(p);return p
    def request(operation,arguments=None):
        code="""import json,socket,sys
with socket.socket(socket.AF_UNIX) as s:
 s.settimeout(6);s.connect('/run/omen-linux/ctl.sock')
 op={'op':sys.argv[1]}
 if len(sys.argv)>2:op['args']=json.loads(sys.argv[2])
 s.sendall(json.dumps({'v':1,'id':1,'op':op}).encode()+b'\\n')
 r=json.loads(s.makefile('rb').readline());assert r['ok'],r;print(json.dumps(r['data']))
"""
        return json.loads(user('python3','-c',code,operation,*([json.dumps(arguments)] if arguments is not None else [])).stdout)
    def passed(message):
        data['checks_passed'].append(message);print('PASS',message,flush=True)
    def running():
        result=user('flatpak','ps','--columns=instance,pid,child-pid,application').stdout
        values=[dict(zip(['instance','pid','child_pid','app'],line.split())) for line in result.splitlines() if APP in line]
        for item in values:instances.add(item['instance'])
        return values
    def begin(name,mode='normal',direct=False,deny=False):
        command=['flatpak','run','--env=OMEN_BRIDGE_TEST_DYNAMIC=sandbox dynamic value']
        if deny:command+=['--no-talk-name=org.freedesktop.Flatpak']
        if direct:command+=['--command=omen-flatpak-game']
        command+=[APP]
        if direct:command+=['custom/wrapper','--','/app/bin/game',mode]
        else:command+=[mode]
        command+=['two words','','literal $HOME','α','--flag']
        process=start(command,name)
        row=wait(lambda:next(iter(running()),None),'Flatpak wrapper app did not start')
        return process,row,BASE/(name+'.log')
    def kill(process,row):
        user('flatpak','kill',row['instance'],check=False)
        process.wait(timeout=15);instances.discard(row['instance'])
        wait(lambda:not running(),'fixture instance did not exit')
    def held():
        return next((s for s in request('get_game_sessions')['sessions'] if s['profile_held']),None)
    def terminal(token,phase='ended'):
        return next((s for s in request('get_game_sessions')['sessions'] if s['token']==token and s['phase']==phase and not s['profile_held']),None)
    try:
        run('loginctl','enable-linger','ubuntu');run('systemctl','start','user@1000.service')
        helper_was_active=user('systemctl','--user','is-active','flatpak-session-helper.service',check=False).stdout.strip()=='active'
        # The distro installs flatpak-spawn as a runtime-building helper.
        spawn=Path('/usr/libexec/flatpak-xdg-utils/flatpak-spawn');assert spawn.is_file()
        runtime=BASE/'runtime';app=BASE/'app';repo=BASE/'repo'
        (runtime/'usr/bin').mkdir(parents=True);(runtime/'usr/lib').mkdir();(runtime/'files').mkdir()
        (runtime/'usr/lib64').symlink_to('lib')
        (runtime/'metadata').write_text('[Runtime]\nname='+PLATFORM+'\n')
        libraries=set()
        binaries={name:Path(shutil.which(name)).resolve() for name in ['bash','cat','sleep','env','mkdir','readlink','pwd']}
        binaries['flatpak-spawn']=spawn
        for name,source in binaries.items():
            shutil.copy2(source,runtime/'usr/bin'/name)
            for line in run('ldd',source).stdout.splitlines():
                match=re.search(r'(/[^ ()]+)',line)
                if match:libraries.add(Path(match[1]))
        for path in libraries:shutil.copy2(path,runtime/'usr/lib'/path.name)
        (runtime/'usr/bin/sh').symlink_to('bash')
        shutil.copy2(shutil.which('ldconfig'),runtime/'usr/bin/ldconfig')
        (app/'files/bin').mkdir(parents=True)
        (app/'files/share/applications').mkdir(parents=True)
        (app/'metadata').write_text('[Application]\nname='+APP+'\nruntime='+PLATFORM+'/x86_64/stable\ncommand=launcher\n')
        (app/'files/share/applications'/ (APP+'.desktop')).write_text('[Desktop Entry]\nType=Application\nName=OMEN wrapper fixture\nExec=launcher\n')
        shutil.copy2(WRAPPER,app/'files/bin/omen-flatpak-game')
        (app/'files/bin/omen-flatpak-game').chmod(0o755)
        (app/'files/bin/launcher').write_text('''#!/bin/sh
mkdir -p "$XDG_DATA_HOME/working directory"
cd "$XDG_DATA_HOME/working directory" || exit 20
export OMEN_BRIDGE_TEST_DYNAMIC="sandbox dynamic value"
printf 'LAUNCHER_CWD=%s\n' "$PWD"
/app/bin/omen-flatpak-game custom/wrapper -- /app/bin/game "$@"
status=$?
printf 'LAUNCHER_EXIT=%s\n' "$status"
exec /usr/bin/sleep 120
''')
        (app/'files/bin/game').write_text('''#!/bin/sh
printf 'GAME_PID=%s\nGAME_CWD=%s\nGAME_ENV=%s\n' "$$" "$PWD" "$OMEN_BRIDGE_TEST_DYNAMIC"
printf 'ARG=%s\\0' "$@"
printf '\\n'
cat /proc/self/cgroup
cat /proc/self/status
mode=$1
case "$mode" in
 normal) sleep 3; exit 7 ;;
 detached) sleep 6 & exit 7 ;;
 long) sleep 8; exit 7 ;;
 signal) kill -TERM "$$" ;;
 *) exit 2 ;;
esac
''')
        for name in ['launcher','game']:(app/'files/bin'/name).chmod(0o755)
        run('chown','-R','1000:1000',BASE)
        user('flatpak','build-export','--disable-sandbox','--runtime',repo,runtime,'stable')
        user('flatpak','build-finish','--talk-name=org.freedesktop.Flatpak',app)
        user('flatpak','build-export','--disable-sandbox',repo,app,'stable')
        user('flatpak','remote-add','--user','--no-gpg-verify','omen-wrapper-fixture',repo)
        user('flatpak','install','--user','--assumeyes','--noninteractive','omen-wrapper-fixture',PLATFORM,APP)
        data['flatpak_version']=user('flatpak','--version').stdout.strip()
        # Default paths are exercised by the unchanged wrapper and real HostCommand.
        config_dir.mkdir(parents=True,mode=0o700)
        config=(config_dir/'config.toml')
        config.write_text('version=1\n[games]\nmanaged_game_ids=["custom/wrapper"]\n[games.per_game_profiles]\n"custom/wrapper"="performance"\n')
        config.chmod(0o600);run('chown','-R','1000:1000',config_dir)
        shutil.copy2(CLI,BASE/'original-cli');shutil.copy2(DEVCLI,BASE/'development-cli')
        os.replace(BASE/'development-cli',CLI);overlaid=True
        buslog=(BASE/'private-bus.log').open('w');logs.append(buslog)
        bus=subprocess.Popen(['dbus-daemon','--session','--nofork','--print-address=1','--address=unix:path='+str(BASE/'bus')],
            stdout=subprocess.PIPE,stderr=buslog,text=True);children.append(bus);address=bus.stdout.readline().strip();assert address.startswith('unix:')
        ppdlog=(BASE/'private-ppd.log').open('w');logs.append(ppdlog)
        ppd=subprocess.Popen([str(HELPER),'ppd-service',address],stdout=subprocess.PIPE,stderr=ppdlog,text=True);children.append(ppd)
        assert ppd.stdout.readline().strip()=='READY'
        run('systemctl','stop','omen-linux.service');OVERRIDE.mkdir()
        (OVERRIDE/'fixture.conf').write_text('[Service]\nExecStart=\nExecStart='+str(HELPER)+' fixture-service '+str(SOCKET)+' '+str(JOURNAL)+' '+address+' authorized\n')
        RULE.write_text('polkit.addRule(function(action, subject) { if (subject.user == "ubuntu" && action.id == "com.omenlinux.control") return polkit.Result.YES; });\n')
        run('systemctl','daemon-reload');run('systemctl','start','omen-linux.service')
        wait(lambda:SOCKET.is_socket() and request('get_profile')=='balanced','private installed service unavailable')
        passed('a signed distro flatpak-spawn and unchanged wrapper run against the installed root service sandbox, normal polkit and a private PPD provider')
        process,row,log=begin('permission-denied',deny=True)
        wait(lambda:'LAUNCHER_EXIT=' in log.read_text(),'missing host permission did not fail')
        assert 'GAME_PID=' not in log.read_text() and request('get_profile')=='balanced' and not held()
        kill(process,row)
        passed('missing org.freedesktop.Flatpak permission prevents command execution and profile ownership; no override is added')
        process,row,log=begin('direct-exit',direct=True)
        wait(lambda:'GAME_PID=' in log.read_text(),'real wrapper did not start the actual command')
        active=wait(held,'real wrapper did not acquire an authorized hold')
        assert request('get_profile')=='performance'
        process.wait(timeout=20);instances.discard(row['instance']);assert process.returncode==7
        wait(lambda:request('get_profile')=='balanced' and terminal(active['token']),'exit 7 did not release the hold')
        passed('real HostCommand handoff executes the actual sandbox command and returns its nonzero exit 7 unchanged')
        process,row,log=begin('detached',mode='detached')
        wait(lambda:'LAUNCHER_EXIT=7' in log.read_text(),'sandbox launcher did not observe game exit 7')
        active=wait(held,'detached game child lost profile ownership')
        assert process.poll() is None and request('get_profile')=='performance'
        content=log.read_text()
        assert 'GAME_ENV=sandbox dynamic value' in content and 'GAME_CWD=' in content
        assert 'ARG=two words\0ARG=\0ARG=literal $HOME\0ARG=α\0ARG=--flag\0' in content
        assert 'NoNewPrivs:\t1' in content and 'Seccomp:\t2' in content and 'omen-game-'+active['token']+'.scope' in content
        wait(lambda:request('get_profile')=='balanced' and terminal(active['token']),'final detached child did not release the hold')
        assert process.poll() is None
        data['detached_observation']={'session':active,'launcher_remained_alive':True,'output':content}
        kill(process,row)
        passed('actual arguments, dynamic environment, sandbox syscall filtering and game scope survive; the hold follows a detached child and ends while the launcher remains alive')
        process,row,log=begin('explicit-release',mode='long')
        active=wait(held,'release scenario did not acquire hold');wait(lambda:'GAME_PID=' in log.read_text(),'release game did not run')
        request('release_game_session',{'token':active['token']})
        wait(lambda:request('get_profile')=='balanced','explicit release did not select the previous profile')
        assert 'LAUNCHER_EXIT=' not in log.read_text() and process.poll() is None
        wait(lambda:'LAUNCHER_EXIT=7' in log.read_text(),'explicit release stopped or lost the game')
        wait(lambda:terminal(active['token']),'released session did not reach normal history')
        kill(process,row)
        passed('explicit release ends only profile ownership; the real sandbox command continues and later reaches ended history')
        process,row,log=begin('service-restart',mode='long')
        active=wait(held,'restart scenario did not acquire hold');wait(lambda:'GAME_PID=' in log.read_text(),'restart game did not run')
        run('systemctl','restart','omen-linux.service')
        wait(lambda:request('get_profile')=='balanced' and terminal(active['token'],'interrupted'),'restart did not recover Interrupted')
        assert 'LAUNCHER_EXIT=' not in log.read_text() and process.poll() is None
        wait(lambda:'LAUNCHER_EXIT=7' in log.read_text(),'service restart stopped the sandbox game')
        assert request('get_profile')=='balanced'
        kill(process,row)
        passed('service restart releases the private provider peer and records Interrupted without stopping the sandbox game or reapplying a profile')
        RULE.unlink();time.sleep(1)
        process,row,log=begin('authorization-denied')
        wait(lambda:'LAUNCHER_EXIT=' in log.read_text(),'authorization refusal did not complete')
        assert 'GAME_PID=' not in log.read_text() and request('get_profile')=='balanced' and not held()
        kill(process,row)
        passed('normal polkit refusal prevents execution of the actual command and leaves no profile hold')
        assert JOURNAL.stat().st_uid==0 and JOURNAL.stat().st_mode&0o777==0o600
        data['result']='passed'
    except BaseException as error:
        data['error']=type(error).__name__+': '+str(error);raise
    finally:
        for instance in instances:user('flatpak','kill',instance,check=False)
        run('systemctl','stop','omen-linux.service',check=False)
        shutil.rmtree(OVERRIDE,ignore_errors=True);RULE.unlink(missing_ok=True);JOURNAL.unlink(missing_ok=True)
        if overlaid:os.replace(BASE/'original-cli',CLI)
        data['installed_cli_restored']=sha(CLI)==data['installed_cli_before_sha256']
        run('systemctl','daemon-reload',check=False);run('systemctl','start','omen-linux.service',check=False)
        for process in reversed(children):
            if process.poll() is None:
                process.terminate()
                try:process.wait(timeout=10)
                except subprocess.TimeoutExpired:process.kill();process.wait(timeout=5)
        user('flatpak','uninstall','--user','--assumeyes',APP,PLATFORM,check=False)
        user('flatpak','remote-delete','--user','omen-wrapper-fixture',check=False)
        if helper_was_active is False:user('systemctl','--user','stop','flatpak-session-helper.service',check=False)
        for log in logs:log.close()
        data['logs']={p.name:p.read_text() for p in BASE.glob('*.log')}
        for path in created:
            if path.exists():shutil.rmtree(path)
        shutil.rmtree(BASE)
        for path in [HELPER,DEVCLI,WRAPPER]:path.unlink(missing_ok=True)
        data['fixture_cleanup_complete']=data['installed_cli_restored'] and not BASE.exists() and all(not p.exists() for p in created)
        state['flatpak_wrapper_probe']=data
        if data['result']=='passed':state['checks_passed'].extend(data['checks_passed'])
        RESULT.write_text(json.dumps(state,indent=2)+'\n')

if __name__=='__main__':main()
