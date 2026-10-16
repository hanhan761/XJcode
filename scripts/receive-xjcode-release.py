"""Bounded HTTPS release transfer, with a shared lock and automatic route cleanup.
Run only through the verified yun target. The config is private and contains a
one-use bearer token, byte length and checksum; it is never a release asset.
"""
import fcntl, hashlib, hmac, http.server, json, os, pathlib, shutil, signal, subprocess, sys, time
root = pathlib.Path('/var/tmp/xjcode-transfer-013')
spec = json.loads((root/'transfer.json').read_text())
archive = pathlib.Path('/var/tmp/xjcode-publish-0.1.3.zip')
lock = open('/var/lock/wenshanzhike-deploy.lock','a')
fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
usage = shutil.disk_usage(root)
assert usage.free - spec['size'] * 3 > max(usage.total * .15, 2 * 1024**3)
config = pathlib.Path('/opt/wenshanzhike/deploy/Caddyfile.prod')
original = config.read_bytes()
marker = b'xjcode.wenshanzhike.com {\n  import security_headers\n'
assert original.count(marker) == 1
route_path = '/_release-upload-013'
route = b'  handle /_release-upload-013 {\n    reverse_proxy 172.19.0.1:18794\n  }\n'
updated = original.replace(marker, marker + route)
(root/'Caddyfile.rollback').write_bytes(original)
def caddy(action):
    subprocess.run(['docker','exec','wenshanzhike-edge-1','caddy',action,'--config','/etc/caddy/Caddyfile','--adapter','caddyfile'],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=30)
def stop(*_): raise RuntimeError('transfer interrupted')
signal.signal(signal.SIGTERM,stop)
signal.signal(signal.SIGINT,stop)
deadline = time.monotonic() + 1200
class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self,*_): pass
    def authorized(self):
        return self.path == route_path and hmac.compare_digest(self.headers.get('Authorization',''), 'Bearer ' + spec['token'])
    def do_HEAD(self):
        self.send_response(204 if self.authorized() else 404); self.end_headers()
    def do_PUT(self):
        if not self.authorized(): self.send_error(404); return
        if self.headers.get('Content-Length') != str(spec['size']): self.send_error(400); return
        left = spec['size']; digest = hashlib.sha256()
        self.connection.settimeout(60)
        try:
            with (root/'archive.part').open('wb') as stream:
                while left:
                    if time.monotonic() > deadline: raise TimeoutError('transfer deadline')
                    data = self.rfile.read(min(left, 1024*1024))
                    if not data: raise RuntimeError('truncated transfer')
                    stream.write(data); digest.update(data); left -= len(data)
            if digest.hexdigest() != spec['sha256']: raise RuntimeError('checksum mismatch')
            os.replace(root/'archive.part', archive)
            self.server.complete = True
            self.send_response(201); self.end_headers()
        except Exception:
            self.send_error(400, 'Transfer not accepted')
server = http.server.HTTPServer(('172.19.0.1',18794), Handler)
server.timeout = 3; server.complete = False
try:
    config.write_bytes(updated)
    caddy('validate'); caddy('reload')
    print('TRANSFER_READY',flush=True)
    while not server.complete and time.monotonic() < deadline: server.handle_request()
    assert server.complete, 'upload deadline exceeded'
finally:
    server.server_close()
    assert config.read_bytes() == updated, 'concurrent configuration change; preserve for review'
    config.write_bytes(original); caddy('reload')
    (root/'transfer.json').unlink(missing_ok=True)
print('TRANSFER_VERIFIED_AND_ROUTE_REMOVED',flush=True)
archive.with_suffix('.zip.ready').write_text('verified HTTPS upload completed\n')
subprocess.run(['python3','/var/tmp/xjcode-transfer-013/deploy.py',str(archive),spec['sha256']],env={**os.environ,'XJCODE_DEPLOY_LOCK_FD':str(lock.fileno())},pass_fds=(lock.fileno(),),check=True)
