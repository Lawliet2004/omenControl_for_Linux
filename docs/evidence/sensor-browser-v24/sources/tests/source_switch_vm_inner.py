#!/usr/bin/env python3
"""Installed default-socket source transitions; private disposable desktop VM."""
import hashlib
import json
import os
from pathlib import Path
import pwd
import selectors
import socket
import subprocess
import sys
import time

RESULT=Path('/var/tmp/omen-vm-result.json')
SOCKET=Path('/run/omen-linux/ctl.sock')


def run(*args, check=True):
    result=subprocess.run(args,capture_output=True,text=True,timeout=60)
    print('+',' '.join(args),result.returncode,flush=True)
    if check: result.check_returncode()
    return result


def wait(predicate,message,timeout=60):
    until=time.monotonic()+timeout
    while time.monotonic()<until:
        try:
            value=predicate()
            if value: return value
        except (FileNotFoundError,StopIteration,ProcessLookupError,ConnectionRefusedError): pass
        time.sleep(.1)
    raise RuntimeError(message)


def request(op):
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(8); stream.connect(str(SOCKET))
        stream.sendall(json.dumps({'v':1,'id':1,'op':{'op':op}}).encode()+b'\n')
        reply=json.loads(stream.makefile('rb').readline()); assert reply['ok'],reply
        return reply['data']


def worker(account, incoming, outgoing, output):
    os.setgroups([]); os.setgid(account.pw_gid); os.setuid(account.pw_uid)
    os.environ.update(HOME=account.pw_dir, XDG_RUNTIME_DIR='/run/user/'+str(account.pw_uid),
        DBUS_SESSION_BUS_ADDRESS='unix:path=/run/user/'+str(account.pw_uid)+'/bus')
    os.environ.pop('AT_SPI_BUS_ADDRESS',None)
    import gi
    gi.require_version('Atspi','2.0')
    from gi.repository import Atspi, Gio, GLib
    bus=Gio.bus_get_sync(Gio.BusType.SESSION,None)
    address=bus.call_sync('org.a11y.Bus','/org/a11y/bus','org.a11y.Bus','GetAddress',None,None,Gio.DBusCallFlags.NONE,10000,None).unpack()[0]
    os.environ['AT_SPI_BUS_ADDRESS']=address
    Atspi.init(); Atspi.set_timeout(2500,5000)
    context=GLib.MainContext.default()
    state={'result':'failed','checks_passed':[],'physical_hardware_writes':False,'host_input':False,
        'default_socket':str(SOCKET),'explicit_socket_override':False}
    def drain():
        while context.pending(): context.iteration(False)
    def applications():
        drain()
        try:
            root=Atspi.get_desktop(0)
            return [root.get_child_at_index(i) for i in range(root.get_child_count())]
        except GLib.Error: return []
    def own():
        for app in applications():
            try:
                if app and app.get_name()=='omen-gui': return app
            except GLib.Error: continue
    gui=wait(own,'installed GUI absent from accessibility bus')
    gui.set_cache_mask(Atspi.Cache.NONE); gui.clear_cache()
    state['gui_pid']=gui.get_process_id()
    def walk(node):
        try:
            if node.get_state_set().contains(Atspi.StateType.DEFUNCT): return
            yield node
            for i in range(node.get_child_count()): yield from walk(node.get_child_at_index(i))
        except GLib.Error: return
    def labels(): drain(); return [n.get_name() for n in walk(gui) if n.get_role_name()=='label']
    def find(name,role): drain(); return next(n for n in walk(gui) if n.get_name()==name and n.get_role_name()==role)
    def has(text): return any(text in name for name in labels())
    def sensitive(name): return find(name,'button').get_state_set().contains(Atspi.StateType.SENSITIVE)
    def stage(command):
        outgoing.write(command+'\n'); outgoing.flush()
        assert incoming.readline().strip()=='OK',command
    def passed(text): state['checks_passed'].append(text); print('PASS',text,flush=True)
    try:
        wait(lambda:has('Source: direct read-only (daemon not running)'),'default source did not start in direct mode')
        assert not sensitive('Import service policies…')
        assert not sensitive('Export service policies…')
        assert not any(name.startswith('Model: HP OMEN') for name in labels()), 'direct source retained the fixture model identity'
        passed('installed GUI starts with default direct monitoring and disabled service tools when system service is stopped')
        stage('mock')
        wait(lambda:has('Source: MOCK — fixture data, no real hardware'),'default late root mock did not connect')
        wait(lambda:sensitive('Import service policies…'),'service tools did not recover')
        assert find('Fixture','button')
        wait(lambda:'MOCK — fixture data, no real hardware' in labels(),'overlay provenance did not follow mock source')
        passed('same GUI discovers a root-owned mock endpoint at the default path and updates Settings, sidebar, health and overlay provenance')
        stage('direct')
        wait(lambda:has('Source: direct read-only (daemon not running)'),'service loss did not restore direct monitoring')
        assert not sensitive('Import service policies…')
        wait(lambda:not has('MOCK — fixture data'),'old fixture provenance remained after direct fallback')
        passed('mock shutdown restores live direct read-only readings and removes fixture provenance without a GUI restart')
        stage('real')
        wait(lambda:has('Source: omen-daemon + local read-only GPU fallback'),'real packaged service did not connect')
        assert find('Service','button')
        wait(lambda:'omen-daemon + local read-only GPU fallback' in labels(),'overlay did not follow real service')
        assert not request('get_info')['mock']
        passed('late packaged root service restores verified nonmock provenance and service tools using the production endpoint')
        first=request('get_info')
        stage('restart')
        wait(lambda:request('get_info')['started_unix']!=first['started_unix'],'service generation did not change')
        wait(lambda:has('Source: omen-daemon + local read-only GPU fallback') and sensitive('Export service policies…'),
            'production service restart did not restore a fresh usable route')
        # A successful report requires the UI's captured pinned route to have
        # migrated; an unchanged Service badge alone cannot prove that.
        time.sleep(1.2)
        def click(name):
            node=wait(lambda:find(name,'button'),'button missing: '+name)
            action=node.get_action_iface()
            assert action.do_action(next(i for i in range(action.get_n_actions()) if action.get_action_name(i).lower() in ['click','activate','press']))
        click('Export diagnostics (JSON)')
        field=wait(lambda:find('Diagnostics report JSON','text'),'new service route could not prepare diagnostics')
        report=json.loads(Atspi.Text.get_text(field.get_text_iface(),0,-1))
        assert report['mock'] is False and report['client_source']['fixture'] is False
        assert report['client_source']['label']=='omen-daemon + local read-only GPU fallback'
        click('Close preview')
        assert gui.get_process_id()==state['gui_pid']
        passed('production daemon restart reconnects in the same installed GUI process with fresh service availability')
        state['result']='passed'
    except BaseException as error:
        state['error']=type(error).__name__+': '+str(error); state['last_labels']=labels(); raise
    finally:
        output.write_text(json.dumps(state,indent=2)+'\n')
        stage('done')


def main():
    assert os.geteuid()==0 and os.environ.get('OMEN_SOURCE_SWITCH_VM')=='1'
    assert run('systemd-detect-virt','--vm').stdout.strip() in ['kvm','qemu']
    assert RESULT.is_file()
    state=json.loads(RESULT.read_text())
    account=pwd.getpwnam('ubuntu'); home=Path(account.pw_dir)
    owned=[]; original={}; mock=None; log=None; child=None
    def user_file(path,text):
        original[path]=path.read_bytes() if path.exists() else None
        path.parent.mkdir(parents=True,exist_ok=True); path.write_text(text); path.chmod(0o600)
        os.chown(path,account.pw_uid,account.pw_gid)
        for parent in path.parents:
            if parent==home: break
            if str(parent).startswith(str(home)+'/'): os.chown(parent,account.pw_uid,account.pw_gid)
    def stop_mock():
        nonlocal mock
        if mock:
            mock.terminate(); mock.wait(timeout=15); mock=None
        if SOCKET.is_symlink(): SOCKET.unlink()
    def capture(name):
        print('OMEN_VM_CAPTURE '+name,flush=True)
        assert sys.stdin.readline().strip()=='OK'
    try:
        run('systemctl','stop','sddm.service'); run('pkill','-TERM','-x','omen-gui',check=False); run('systemctl','stop','omen-linux.service')
        user_file(home/'.config/omen-linux/config.toml','version=1\n[general]\nonboarding_dismissed=true\npoll_interval_ms=500\noverlay_enabled=true\n')
        user_file(home/'.config/plasma-workspace/env/omen-test.sh','export QT_LINUX_ACCESSIBILITY_ALWAYS_ON=1 GTK_A11Y=atspi LIBGL_ALWAYS_SOFTWARE=1 QT_QUICK_BACKEND=software\n')
        user_file(home/'.config/autostart/omen-test.desktop','[Desktop Entry]\nType=Application\nName=OMEN source validation\nExec=/usr/bin/env OMEN_REPORT_READY=1 GTK_A11Y=atspi /usr/bin/omen-gui --new-instance --page settings\n')
        sessions=[p for p in Path('/usr/share/wayland-sessions').glob('*.desktop') if 'startplasma-wayland' in p.read_text()]
        assert len(sessions)==1
        conf=Path('/etc/sddm.conf.d/10-omen-source-test.conf'); conf.parent.mkdir(exist_ok=True)
        conf.write_text('[Autologin]\nUser=ubuntu\nSession='+sessions[0].name+'\nRelogin=false\n[Theme]\nCurrent=breeze\n[X11]\nServerPath=/usr/lib/xorg/Xorg\n'); owned.append(conf)
        run('systemctl','start','sddm.service')
        output=home/'source-switch-result.json'; owned.append(output)
        root_read,user_write=os.pipe(); user_read,root_write=os.pipe()
        child=os.fork()
        if child==0:
            os.close(root_read); os.close(root_write)
            try: worker(account,os.fdopen(user_read),os.fdopen(user_write,'w'),output)
            except BaseException:
                import traceback; traceback.print_exc(); os._exit(1)
            os._exit(0)
        os.close(user_read); os.close(user_write)
        reader=os.fdopen(root_read); writer=os.fdopen(root_write,'w')
        selector=selectors.DefaultSelector(); selector.register(reader,selectors.EVENT_READ)
        until=time.monotonic()+240
        try:
            while time.monotonic()<until:
                if not selector.select(1):
                    ended,status=os.waitpid(child,os.WNOHANG)
                    if ended: child=None; raise RuntimeError('source observer exited before completion: '+str(status))
                    continue
                command=reader.readline().strip()
                if not command: raise RuntimeError('source observer closed its control pipe')
                if command=='done': writer.write('OK\n'); writer.flush(); break
                if command=='mock':
                    capture('source-direct')
                    log=Path('/var/tmp/source-mock.log').open('w')
                    mock=subprocess.Popen(['/usr/bin/omen-daemon','--mock-omen'],stdout=subprocess.PIPE,stderr=log,text=True)
                    upstream=mock.stdout.readline().strip(); assert upstream.startswith('/tmp/omen-mock-')
                    # Private guest only: permit its desktop user to read the
                    # root-owned mock socket. No physical controls are exposed.
                    Path(upstream).chmod(0o666)
                    SOCKET.parent.mkdir(exist_ok=True); SOCKET.symlink_to(upstream)
                elif command=='direct': capture('source-fixture'); stop_mock()
                elif command=='real': run('systemctl','start','omen-linux.service')
                elif command=='restart':
                    capture('source-service'); time.sleep(1.1); run('systemctl','restart','omen-linux.service')
                else: raise RuntimeError('unexpected source command: '+command)
                writer.write('OK\n'); writer.flush()
            else: raise RuntimeError('source transition deadline exceeded')
            _,status=os.waitpid(child,0); child=None
            data=json.loads(output.read_text())
            state['source_switch']=data
            assert os.waitstatus_to_exitcode(status)==0 and data['result']=='passed'
            state['source_switch_source_sha256']=hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
            state['checks_passed'].extend(data['checks_passed'])
        finally:
            selector.close(); reader.close(); writer.close()
    except BaseException:
        if 'output' in locals() and output.exists(): state['source_switch']=json.loads(output.read_text())
        raise
    finally:
        if child:
            os.kill(child,15); os.waitpid(child,0)
        run('systemctl','stop','sddm.service',check=False)
        stop_mock()
        if log: log.close()
        for path in reversed(owned): path.unlink(missing_ok=True)
        for path,data in original.items():
            if data is None: path.unlink(missing_ok=True)
            else: path.write_bytes(data); os.chown(path,account.pw_uid,account.pw_gid)
        run('systemctl','restart','omen-linux.service',check=False)
        state.setdefault('source_switch',{})['fixture_cleanup_complete']=True
        RESULT.write_text(json.dumps(state,indent=2)+'\n')


if __name__=='__main__': main()
