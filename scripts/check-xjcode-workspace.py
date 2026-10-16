"""Real Windows sandbox write regression; no model request or visible console."""
import argparse, json, os, pathlib, subprocess, tempfile, uuid
p = argparse.ArgumentParser()
p.add_argument('exe')
p.add_argument('workspace', type=pathlib.Path)
a = p.parse_args()
workspace = a.workspace.resolve(strict=True)
name = '.xjcode-probe-' + uuid.uuid4().hex + '.tmp'
inside = workspace / name
ps = pathlib.Path(os.environ['SystemRoot']) / 'System32/WindowsPowerShell/v1.0/powershell.exe'
def quoted(path): return "'" + str(path).replace("'", "''") + "'"
with tempfile.TemporaryDirectory(prefix='xjcode-outside-') as outside:
    denied = pathlib.Path(outside) / name
    script = "$ErrorActionPreference='Stop'; [IO.File]::WriteAllText(" + quoted(inside) + ", 'XJCODE_WORKSPACE_OK'); try { [IO.File]::WriteAllText(" + quoted(denied) + ", 'unexpected'); exit 31 } catch { }; exit 0"
    # The OS temp directory is normally writable. Exclude it explicitly to make
    # the outside-write assertion meaningful, while preserving project access.
    cmd = [str(pathlib.Path(a.exe).resolve()), 'sandbox', '-P', ':workspace',
           '-c', 'sandbox_workspace_write.exclude_tmpdir_env_var=true',
           '-c', 'sandbox_workspace_write.exclude_slash_tmp=true', '--', str(ps),
           '-NoProfile', '-NonInteractive', '-WindowStyle', 'Hidden', '-Command', script]
    try:
        r = subprocess.run(cmd, cwd=workspace, capture_output=True, timeout=45,
                           creationflags=subprocess.CREATE_NO_WINDOW)
        result = {'exitCode': r.returncode, 'projectWritten': inside.exists() and inside.read_text() == 'XJCODE_WORKSPACE_OK', 'outsideDenied': not denied.exists()}
        print(json.dumps(result), flush=True)
        if not all([r.returncode == 0, result['projectWritten'], result['outsideDenied']]):
            print(r.stderr.decode('utf-8', errors='replace'))
            raise SystemExit(1)
    finally:
        inside.unlink(missing_ok=True)
