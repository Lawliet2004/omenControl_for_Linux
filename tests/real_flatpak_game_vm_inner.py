#!/usr/bin/env python3
"""Restore verified Flathub sideload objects, then exercise actual Lutris/OpenTTD."""
import importlib.util
import os
from pathlib import Path
import pwd
import subprocess


def main():
    assert os.geteuid()==0 and os.environ.get('OMEN_REAL_FLATPAK_VM')=='1'
    home=Path(pwd.getpwnam('ubuntu').pw_dir)
    cache=Path('/var/tmp/omen-real-flathub-cache');assert not cache.exists()
    cache.mkdir(mode=0o755);os.chown(cache,1000,1000)
    env={'HOME':str(home),'XDG_RUNTIME_DIR':'/run/user/1000','DBUS_SESSION_BUS_ADDRESS':'unix:path=/run/user/1000/bus',
        'http_proxy':'http://127.0.0.1:49177','https_proxy':'http://127.0.0.1:49177',
        'HTTP_PROXY':'http://127.0.0.1:49177','HTTPS_PROXY':'http://127.0.0.1:49177','NO_PROXY':'','LC_ALL':'C.UTF-8'}
    def user(*args,timeout=600):
        subprocess.run(['runuser','-u','ubuntu','--','env',*[k+'='+v for k,v in env.items()],*map(str,args)],check=True,timeout=timeout)
    subprocess.run(['loginctl','enable-linger','ubuntu'],check=True)
    subprocess.run(['systemctl','start','user@1000.service'],check=True)
    user('tar','--no-same-owner','-xzf','/var/tmp/omen-flathub-lutris-cache.tar.gz','-C',cache)
    user('flatpak','remote-add','--user','flathub','/var/tmp/omen-flathub.flatpakrepo')
    user('flatpak','remote-modify','--user','--collection-id=org.flathub.Stable','flathub')
    # Metadata is fetched over a restricted HTTPS tunnel; object installation is
    # from the cache. TLS, Flathub GPG signatures and commit verification remain on.
    user('flatpak','install','--user','--assumeyes','--noninteractive','--no-related','--sideload-repo='+str(cache),
        'flathub','net.lutris.Lutris',timeout=1200)
    user('ostree','fsck','--repo='+str(home/'.local/share/flatpak/repo'),timeout=300)
    spec=importlib.util.spec_from_file_location('native_game','/var/tmp/omen-native-real-game.py')
    module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    module.main()

if __name__=='__main__':main()
