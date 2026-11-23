import ctypes as C, json, os, subprocess
os.environ.update(XDG_RUNTIME_DIR='/run/user/1000', WAYLAND_DISPLAY='wayland-0', GDK_BACKEND='wayland')
gtk=C.CDLL('libgtk-4.so.1');gio=C.CDLL('libgio-2.0.so.0')
def signature(name, arguments, result):
    f=getattr(gtk,name);f.argtypes=arguments;f.restype=result;return f
assert signature('gtk_init_check',[],C.c_int)()
display=signature('gdk_display_get_default',[],C.c_void_p)()
theme=signature('gtk_icon_theme_get_for_display',[C.c_void_p],C.c_void_p)(display)
theme_name=signature('gtk_icon_theme_get_theme_name',[C.c_void_p],C.c_char_p)(theme).decode()
exists=signature('gtk_icon_theme_has_icon',[C.c_void_p,C.c_char_p],C.c_int)
lookup=signature('gtk_icon_theme_lookup_icon',[C.c_void_p,C.c_char_p,C.POINTER(C.c_char_p),C.c_int,C.c_int,C.c_int,C.c_int],C.c_void_p)
file_for=signature('gtk_icon_paintable_get_file',[C.c_void_p],C.c_void_p)
name_for=signature('gtk_icon_paintable_get_icon_name',[C.c_void_p],C.c_char_p)
gio.g_file_get_uri.argtypes=[C.c_void_p];gio.g_file_get_uri.restype=C.c_char_p
out={'theme':theme_name,'icons':[], 'installed_svg_loader':subprocess.run(['dpkg-query','-W','-f=${Status} ${Version}','librsvg2-common'],capture_output=True,text=True).stdout}
for value in ['computer-symbolic','view-grid-symbolic','applications-system-symbolic','speedometer-symbolic','view-refresh-symbolic','weather-clear-night-symbolic','video-display-symbolic','battery-symbolic','applications-games-symbolic','preferences-system-symbolic']:
    icon=lookup(theme,value.encode(),None,16,1,0,0);file=file_for(icon);name=name_for(icon)
    out['icons'].append({'requested':value,'exists':bool(exists(theme,value.encode())),'resolved_name':name.decode() if name else None,'uri':gio.g_file_get_uri(file).decode() if file else None})
print(json.dumps(out,indent=2))
