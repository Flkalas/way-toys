#!/usr/bin/env python3
"""Exercise real X11 frontends on a private Xvfb/KWin/D-Bus desktop.

Requires Xvfb, kwin_x11, xauth, xclip, xdotool, xrandr, qdbus6, Python
dbus/gi/Pillow and built workspace binaries. No host shortcuts are changed.
The screen-lock service is a fixture; this does not test the real lock screen.
Clipboard scopes use the caller's systemd user manager with the isolated DISPLAY.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]

def wait_for(check, timeout=10):
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        try:
            value = check()
            if value:
                return value
        except (subprocess.SubprocessError, ValueError, OSError) as error:
            last = error
        time.sleep(.1)
    raise RuntimeError(f'Timed out: {last}')

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--xvfb', default='Xvfb')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--split-monitors', action='store_true', help='Also exercise two synthetic XRandR monitors')
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    processes = []
    handles = []
    results = {}
    shutil.copyfile(__file__, out/'test-x11.py')
    source_files = [p for folder in ('crates','kwin','scripts') for p in (ROOT/folder).rglob('*') if p.is_file() and p.suffix in ('.rs','.toml','.cpp','.js','.py','.sh')]
    source_files += [ROOT/'Cargo.toml', ROOT/'Cargo.lock', ROOT/'rust-toolchain.toml']
    manifest = {str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in source_files}
    (out/'source-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    (out/'environment.json').write_text(json.dumps({'time':time.strftime('%Y-%m-%dT%H:%M:%S%z'),'command':sys.argv,'cwd':str(ROOT),'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'os':Path('/etc/os-release').read_text(),'kernel':os.uname().release,'rustc':subprocess.check_output(['rustc','--version'],cwd=ROOT,text=True).strip(),'python':sys.version},indent=2)+'\n')
    with tempfile.TemporaryDirectory(prefix='waytoys-x11-') as temporary:
        work = Path(temporary)
        env = os.environ.copy()
        env.pop('WAYLAND_DISPLAY', None)
        env.update(XDG_SESSION_TYPE='x11', QT_QPA_PLATFORM='xcb', XDG_CURRENT_DESKTOP='KDE',
                   XDG_CONFIG_HOME=str(work/'config'), XDG_DATA_HOME=str(work/'data'),
                   XDG_STATE_HOME=str(work/'state'), XDG_CACHE_HOME=str(work/'cache'),
                   XDG_RUNTIME_DIR=str(work/'runtime'), LIBGL_ALWAYS_SOFTWARE='1',
                   KWIN_COMPOSE='O2')
        (work/'runtime').mkdir(mode=0o700)
        # Only systemd-run is routed to the real user manager. Every desktop app
        # and the KWin scripting adapter use the private D-Bus session below.
        real_systemd = shutil.which('systemd-run')
        env['WAYTOYS_TEST_SYSTEMD_BUS'] = env.get('DBUS_SESSION_BUS_ADDRESS', '')
        env['WAYTOYS_TEST_SYSTEMD_RUNTIME'] = os.environ.get('XDG_RUNTIME_DIR', f'/run/user/{os.getuid()}')
        (work/'bin').mkdir()
        wrapper = work/'bin/systemd-run'
        wrapper.write_text('#!/bin/sh\nexport DBUS_SESSION_BUS_ADDRESS="$WAYTOYS_TEST_SYSTEMD_BUS"\nexport XDG_RUNTIME_DIR="$WAYTOYS_TEST_SYSTEMD_RUNTIME"\nexec '+real_systemd+' "$@"\n')
        wrapper.chmod(0o700)
        env['PATH'] = str(work/'bin')+os.pathsep+env['PATH']

        def record_command(command):
            record = [str(x) for x in command]
            if record[0] == 'xauth':
                record[-1] = '<ephemeral cookie>'
            with (out/'commands.jsonl').open('a') as log:
                log.write(json.dumps(record,ensure_ascii=False)+'\n')
        def run(*command, **kwargs):
            record_command(command)
            return subprocess.run([str(x) for x in command], cwd=ROOT, env=env,
                                  check=True, capture_output=True, timeout=15, **kwargs)

        def start(name, *command):
            record_command(command)
            log = open(out/(name+'.log'), 'wb')
            handles.append(log)
            process = subprocess.Popen([str(x) for x in command], cwd=ROOT, env=env,
                                       stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
            processes.append(process)
            return process

        def dbus(service, path, method, *values):
            return run('qdbus6', service, path, method, *values).stdout.decode().strip()

        def runtime(method='Status', *values):
            return json.loads(dbus('org.waytoys.Runtime1', '/Waytoys', 'org.waytoys.Runtime1.'+method, *values))

        def window(name):
            return run('xdotool', 'search', '--onlyvisible', '--name', '^'+name+'$').stdout.decode().splitlines()[-1]

        def keys(name, key):
            run('xdotool', 'windowactivate', '--sync', window(name))
            run('xdotool', 'key', '--clearmodifiers', key)
            time.sleep(.2)

        def screenshot(name):
            run('/usr/bin/python3', '-c', 'from PIL import ImageGrab; import os,sys; ImageGrab.grab(xdisplay=os.environ["DISPLAY"]).save(sys.argv[1])', out/(name+'.png'))

        try:
            number = next(n for n in range(90, 200) if not Path(f'/tmp/.X{n}-lock').exists() and not Path(f'/tmp/.X11-unix/X{n}').exists())
            env['DISPLAY'] = f':{number}'
            env['XAUTHORITY'] = str(work/'auth')
            (work/'auth').touch(mode=0o600)
            run('xauth', '-f', work/'auth', 'add', env['DISPLAY'], '.', os.urandom(16).hex())
            start('xvfb', args.xvfb, env['DISPLAY'], '-screen', '0', '1280x720x24', '-nolisten', 'tcp', '-auth', work/'auth')
            wait_for(lambda: run('xdpyinfo'))
            bus = subprocess.Popen(['dbus-daemon', '--session', '--nofork', '--print-address=1'], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
            processes.append(bus)
            env['DBUS_SESSION_BUS_ADDRESS'] = bus.stdout.readline().strip()
            start('lock-fixture', '/usr/bin/python3', ROOT/'scripts/x11-session-fixture.py')
            start('kwin', 'kwin_x11', '--replace')
            wait_for(lambda: dbus('org.kde.KWin', '/KWin', 'org.kde.KWin.supportInformation'))
            runtime_process = start('runtime', ROOT/'target/debug/waytoys', '--background', '--app-config-dir', work/'app', '--config-dir', work/'zones')
            wait_for(lambda: runtime())
            result = runtime('SetZonesEnabled', 'true')
            assert result['ready'], result
            results['runtime_ready'] = True
            outputs = json.loads(run(ROOT/'target/debug/waytoys-zones-editor', '--list-outputs', '--json').stdout)
            assert outputs[0]['logical_size'] == [1280,720], outputs
            results['outputs'] = outputs

            editor = start('editor', ROOT/'target/debug/waytoys-zones-editor', '--config-dir', work/'zones')
            wait_for(lambda: window('waytoys-zones-editor'))
            run('xdotool', 'mousemove', '200', '250', 'click', '1')
            time.sleep(.2)
            screenshot('editor')
            run('xdotool', 'key', '--clearmodifiers', 'ctrl+s')
            assert editor.wait(timeout=5) == 0
            layout = json.loads(next((work/'zones/outputs').glob('*.json')).read_text())
            results['saved_layout'] = layout

            start('capture-probe', ROOT/'target/capture-probe')
            wait_for(lambda: window('WaytoysCaptureProbe'))
            def geometry(place=None):
                command = ['/usr/bin/python3', ROOT/'scripts/kwin-window-probe.py', '--caption', 'WaytoysCaptureProbe']
                if place:
                    command += ['--place', *place]
                return json.loads(run(*command).stdout)['windows'][0]['geometry']
            layout_path = next((work/'zones/outputs').glob('*.json'))
            preview = json.loads(run(ROOT/'target/debug/waytoys-zones', 'preview', layout_path, '0', '1280', '720').stdout)
            target = preview['zones'][-1]['rect']
            tx,ty = target['x']+target['width']//2,target['y']+target['height']//2
            geometry([300,150,500,320])
            run('xdotool', 'mousemove', '400', '165', 'mousedown', '1')
            time.sleep(.1)
            run('xdotool', 'mousemove', '420', '185', 'keydown', 'Shift_L', 'mousemove', str(tx), str(ty))
            time.sleep(.4)
            screenshot('zones-drag')
            run('xdotool', 'mouseup', '1')
            time.sleep(.3)
            run('xdotool', 'keyup', 'Shift_L')
            actual = geometry()
            expected = [target[k] for k in ('x','y','width','height')]
            assert all(abs(a-b)<=1 for a,b in zip(actual,expected)), (actual,expected)
            results['zones_snap'] = actual
            # Esc must restore the initial native frame without applying a zone.
            origin = geometry([300,150,500,320])
            run('xdotool', 'mousemove', '400', '165', 'mousedown', '1')
            time.sleep(.1)
            run('xdotool', 'mousemove', '420', '185', 'keydown', 'Shift_L', 'mousemove', str(tx), str(ty))
            time.sleep(.2)
            run('xdotool', 'key', 'Escape', 'mouseup', '1', 'keyup', 'Shift_L')
            time.sleep(.2)
            cancelled = geometry()
            results['zones_escape_frames'] = {'before':origin,'after':cancelled}
            assert cancelled == origin, (cancelled,origin)
            results['zones_escape'] = True

            start('paste-probe', ROOT/'target/paste-probe', out/'pasted.jsonl')
            wait_for(lambda: window('WaytoysPasteProbe'))
            runtime('SetPasteEnabled', 'true')
            wait_for(lambda: runtime()['paste_ready'])
            keys('WaytoysPasteProbe', 'ctrl+1')
            def history():
                return json.loads(run(ROOT/'target/debug/waytoys-paste', '--status').stdout)['listing']
            wait_for(lambda: history().get('entries'))
            results['text_history'] = history()
            keys('WaytoysPasteProbe', 'ctrl+2')
            wait_for(lambda: len(history()['entries']) >= 2)
            before = len(history()['entries'])
            keys('WaytoysPasteProbe', 'ctrl+3')
            time.sleep(.4)
            assert len(history()['entries']) == before
            results['sensitive_excluded'] = True

            start('settings', ROOT/'target/debug/waytoys-settings', '--app-config-dir', work/'app', '--config-dir', work/'zones')
            wait_for(lambda: window('Waytoys'))
            screenshot('settings')
            dbus('org.waytoys.Settings1', '/Settings', 'org.waytoys.Settings1.Close', '')
            geometry([100,100,500,320])
            time.sleep(.4)
            screenshot('capture-before')

            capture = start('capture', ROOT/'target/debug/waytoys-capture')
            wait_for(lambda: window('waytoys-capture'))
            run('xdotool', 'mousemove', '150', '150', 'mousedown', '1', 'mousemove', '450', '350')
            time.sleep(.2)
            screenshot('capture-selection')
            run('xdotool', 'mouseup', '1')
            assert capture.wait(timeout=8) == 0
            png = run('xclip', '-selection', 'clipboard', '-out', '-target', 'image/png').stdout
            (out/'captured.png').write_bytes(png)
            from PIL import Image
            with Image.open(out/'captured.png') as im:
                assert im.size == (300,200), im.size
                results['capture_size'] = list(im.size)
                with Image.open(out/'capture-before.png') as before_image:
                    assert im.convert('RGB').tobytes() == before_image.crop((150,150,450,350)).convert('RGB').tobytes()
            wait_for(lambda: len(history()['entries']) > before)
            keys('WaytoysPasteProbe', 'ctrl+v')
            pasted = json.loads((out/'pasted.jsonl').read_text().splitlines()[-1])
            with Image.open(out/'captured.png') as im:
                assert pasted['sha256'] == hashlib.sha256(im.convert('RGBA').tobytes()).hexdigest()
            results['qt_image_paste'] = pasted
            cancel = start('capture-cancel', ROOT/'target/debug/waytoys-capture')
            wait_for(lambda: window('waytoys-capture'))
            run('xdotool', 'key', 'Escape')
            assert cancel.wait(timeout=5) == 0
            assert png == run('xclip', '-selection', 'clipboard', '-out', '-target', 'image/png').stdout
            results['capture_cancel_preserves_clipboard'] = True
            # Exercise the X11 Iced picker and the standard UTF8_STRING copy
            # target with a real Qt consumer, including Korean and line breaks.
            text_id = results['text_history']['entries'][0]['id']
            dbus('org.waytoys.Paste1', '/Paste', 'org.waytoys.Paste1.Pin', str(text_id), 'true')
            runtime('ShowPaste', '')
            wait_for(lambda: window('Waytoys Clipboard'))
            time.sleep(.3)
            screenshot('picker')
            run('xdotool', 'windowactivate', '--sync', window('Waytoys Clipboard'))
            run('xdotool', 'type', '--clearmodifiers', 'Waytoys clipboard')
            time.sleep(.3)
            keys('Waytoys Clipboard', 'Return')
            def picker_closed():
                try:
                    window('Waytoys Clipboard')
                    return False
                except subprocess.CalledProcessError:
                    return True
            wait_for(picker_closed)
            keys('WaytoysPasteProbe', 'ctrl+v')
            text_paste = json.loads((out/'pasted.jsonl').read_text().splitlines()[-1])
            expected_text = 'Waytoys clipboard 한글 확인\n두 번째 줄 · 27'.encode()
            assert text_paste == {'kind':'text', 'bytes':len(expected_text), 'sha256':hashlib.sha256(expected_text).hexdigest()}, text_paste
            results['picker_text_paste'] = text_paste
            image_id = next(e['id'] for e in history()['entries'] if (e['width'],e['height']) == (300,200))
            dbus('org.waytoys.Paste1', '/Paste', 'org.waytoys.Paste1.Copy', str(image_id))
            assert png == run('xclip', '-selection', 'clipboard', '-out', '-target', 'image/png').stdout
            results['history_image_copy'] = True
            wait_for(lambda: runtime()['ready'])

            # This fixture tests reactions to the D-Bus lock state; it does not
            # impersonate a real lock screen or lock the user's desktop.
            dbus('org.freedesktop.ScreenSaver', '/ScreenSaver', 'org.freedesktop.ScreenSaver.SetActive', 'true')
            wait_for(lambda: not runtime()['ready'])
            assert not history()['entries']
            denied = subprocess.run(['qdbus6','org.waytoys.Paste1','/Paste','org.waytoys.Paste1.Copy',str(text_id)],env=env,capture_output=True)
            assert denied.returncode != 0
            dbus('org.freedesktop.ScreenSaver', '/ScreenSaver', 'org.freedesktop.ScreenSaver.SetActive', 'false')
            wait_for(lambda: runtime()['ready'])
            results['lock_state_fixture'] = True

            runtime('Quit', '')
            assert runtime_process.wait(timeout=6) == 0
            assert png == run('xclip', '-selection', 'clipboard', '-out', '-target', 'image/png').stdout
            results['clipboard_survives_runtime_exit'] = True
            start('runtime-restart', ROOT/'target/debug/waytoys', '--background', '--app-config-dir', work/'app', '--config-dir', work/'zones')
            wait_for(lambda: runtime()['ready'] and runtime()['paste_ready'])
            assert history()['pinned'] == 1
            results['pinned_after_restart'] = True

            if args.split_monitors:
                run('xrandr', '--setmonitor', 'X11-left', '640/200x720/220+0+0', 'screen')
                run('xrandr', '--setmonitor', 'X11-right', '640/200x720/220+640+0', 'none')
                outputs = json.loads(run(ROOT/'target/debug/waytoys-zones-editor', '--list-outputs', '--json').stdout)
                assert len(outputs) == 2, outputs
                results['split_outputs'] = outputs
                geometry([400,100,500,320])
                time.sleep(.3)
                screenshot('split-before')
                capture = start('capture-split', ROOT/'target/debug/waytoys-capture')
                wait_for(lambda: window('waytoys-capture'))
                run('xdotool', 'mousemove', '600', '150', 'mousedown', '1', 'mousemove', '800', '300', 'mouseup', '1')
                assert capture.wait(timeout=8) == 0
                (out/'captured-split.png').write_bytes(run('xclip','-selection','clipboard','-out','-target','image/png').stdout)
                with Image.open(out/'captured-split.png') as im, Image.open(out/'split-before.png') as original:
                    assert im.size == (200,150)
                    assert im.convert('RGB').tobytes() == original.crop((600,150,800,300)).convert('RGB').tobytes()
                results['cross_monitor_capture'] = True
            wait_for(lambda: runtime()['ready'])
            results['final_runtime'] = runtime()
            results['passed'] = True
            print('X11 integration passed')
        finally:
            # Tell clipboard owners on the isolated server to relinquish data.
            # Closing Xvfb also disconnects any remaining fixture clients.
            for process in reversed(processes):
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=4)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
            for handle in handles:
                handle.close()
            (out/'results.json').write_text(json.dumps(results, ensure_ascii=False, indent=2)+'\n')

if __name__ == '__main__':
    main()
