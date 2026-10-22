"""Bounded pinned-SFTP transfer and no-build activation under the shared lock.
No temporary HTTP upload route, Caddy configuration edit, or server build.
"""
import fcntl,hashlib,json,os,pathlib,shutil,subprocess,time
root=pathlib.Path('/var/tmp/xjcode-transfer-015')
spec=json.loads((root/'transfer.json').read_text())
archive=pathlib.Path('/var/tmp/xjcode-publish-0.1.5.zip')
lock=open('/var/lock/wenshanzhike-deploy.lock','a')
fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
usage=shutil.disk_usage(root)
assert usage.free-spec['size']*3>max(usage.total*.15,2*1024**3),'disk reserve'
assert not archive.exists(),'immutable archive already exists'
print('TRANSFER_LOCKED_READY',flush=True)
deadline=time.monotonic()+1800
while not (root/'upload.ready').exists():
 assert time.monotonic()<deadline,'upload deadline exceeded'
 time.sleep(2)
partial=archive.with_suffix('.zip.part')
sha=hashlib.sha256();total=0
with partial.open('xb') as target:
 for item in spec['chunks']:
  name=item['name'];assert name.startswith('part-') and name[5:].isdigit() and len(name)==8
  file=root/name;assert file.resolve().parent==root and file.stat().st_size==item['size']
  part_sha=hashlib.sha256()
  with file.open('rb') as source:
   for block in iter(lambda:source.read(1024*1024),b''):
    target.write(block);sha.update(block);part_sha.update(block);total+=len(block)
  assert part_sha.hexdigest()==item['sha256'],'chunk checksum'
 target.flush();os.fsync(target.fileno())
assert total==spec['size'] and sha.hexdigest()==spec['sha256'],'archive checksum'
os.replace(partial,archive)
archive.with_suffix('.zip.ready').write_text('verified pinned-SFTP upload completed\n')
print('SFTP_TRANSFER_VERIFIED',flush=True)
subprocess.run(['python3',str(root/'deploy.py'),str(archive),spec['sha256']],env={**os.environ,'XJCODE_DEPLOY_LOCK_FD':str(lock.fileno())},pass_fds=(lock.fileno(),),check=True)
# Only remove the exact verified upload chunks after successful observation.
for item in spec['chunks']:
 file=root/item['name'];assert file.resolve().parent==root;file.unlink()
print('RELEASE_COMPLETE_CHUNKS_REMOVED',flush=True)
