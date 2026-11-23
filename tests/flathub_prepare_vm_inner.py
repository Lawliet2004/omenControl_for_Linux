#!/usr/bin/env python3
"""Download and export the real signed Lutris Flatpak in a disposable VM only."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import signal
import time

BASE=Path('/var/tmp/omen-flathub-prepare')
USER_HOME=Path('/home/ubuntu')
APP='net.lutris.Lutris'


def run(*command, timeout=90, capture=True):
    print('+', ' '.join(map(str,command)), flush=True)
    process=subprocess.Popen(list(map(str,command)), stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE if capture else None, text=True, start_new_session=True)
    try:
        stdout,stderr=process.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid,signal.SIGTERM)
        try:process.communicate(timeout=10)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid,signal.SIGKILL);process.communicate(timeout=10)
        raise
    if capture:print(stdout,stderr,flush=True)
    if process.returncode:raise subprocess.CalledProcessError(process.returncode,command,stdout,stderr)
    return stdout if capture else ''



def main():
    assert os.geteuid()==0 and os.environ.get('OMEN_FLATHUB_PREPARE_VM')=='1'
    assert run('systemd-detect-virt','--vm').strip() in ['qemu','kvm']
    assert not BASE.exists() and not (USER_HOME/'.local/share/flatpak').exists()
    proxy=os.environ['HTTPS_PROXY'];assert proxy=='http://127.0.0.1:49177'
    BASE.mkdir(mode=0o755);os.chown(BASE,1000,1000)
    run('loginctl','enable-linger','ubuntu');run('systemctl','start','user@1000.service')
    env={'HOME':str(USER_HOME),'XDG_RUNTIME_DIR':'/run/user/1000',
        'DBUS_SESSION_BUS_ADDRESS':'unix:path=/run/user/1000/bus','LC_ALL':'C.UTF-8',
        'http_proxy':proxy,'https_proxy':proxy,'HTTP_PROXY':proxy,'HTTPS_PROXY':proxy,'NO_PROXY':''}
    def user(*args,**kw):return run('runuser','-u','ubuntu','--','env',*[k+'='+v for k,v in env.items()],*args,**kw)
    data={'result':'failed','purpose':'signed Flathub asset preparation only','application_tests_executed':False,
        'host_packages_installed':False,'physical_hardware_writes':False,'app':APP,
        'source_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
    try:
        user('flatpak','remote-add','--user','flathub','https://dl.flathub.org/repo/flathub.flatpakrepo',timeout=180)
        # The official https://dl.flathub.org/repo/config declares this
        # collection. Explicitly bind it for verified offline USB exports;
        # flathub.flatpakrepo does not set it by default.
        user('flatpak','remote-modify','--user','--collection-id=org.flathub.Stable','flathub')
        user('flatpak','install','--user','--assumeyes','--noninteractive','--no-related','flathub',APP,timeout=3600,capture=False)
        data['app_info']=user('flatpak','info','--user',APP)
        data['app_commit']=user('flatpak','info','--user','--show-commit',APP).strip()
        data['app_metadata']=user('flatpak','info','--user','--show-metadata',APP)
        data['installed_refs']=user('flatpak','list','--user','--columns=ref,origin,active')
        data['remotes']=user('flatpak','remotes','--user','--show-details')
        repo=USER_HOME/'.local/share/flatpak/repo'
        user('ostree','fsck','--repo='+str(repo),timeout=300,capture=False)
        # The official USB export includes required runtime and related refs.
        usb=BASE/'usb';usb.mkdir();os.chown(usb,1000,1000)
        user('flatpak','create-usb','--user','--allow-partial',usb,APP,timeout=600,capture=False)
        exported=usb/'.ostree/repo';assert exported.is_dir()
        data['export_refs']=user('ostree','refs','--repo='+str(exported))
        user('ostree','fsck','--repo='+str(exported),timeout=300,capture=False)
        archive=BASE/'flathub-lutris-cache.tar.gz'
        user('tar','-czf',archive,'-C',usb,'.ostree',timeout=600,capture=False)
        with archive.open('rb') as stream:data['archive_sha256']=hashlib.file_digest(stream,'sha256').hexdigest()
        data['archive_bytes']=archive.stat().st_size
        data['archive_path']=str(archive)
        data['result']='prepared'
    except BaseException as error:
        data['error']=type(error).__name__+': '+str(error)
        raise
    finally:
        (BASE/'result.json').write_text(json.dumps(data,indent=2)+'\n')
        print(json.dumps(data,indent=2),flush=True)

if __name__=='__main__':main()
