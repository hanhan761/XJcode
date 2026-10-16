"""No-build activation on the existing XJCode host; run via the pinned yun target."""
import argparse, fcntl, hashlib, json, os, pathlib, shutil, subprocess, time, zipfile
parser = argparse.ArgumentParser()
parser.add_argument('archive', type=pathlib.Path)
parser.add_argument('sha256')
args = parser.parse_args()
base = pathlib.Path('/opt/wenshanzhike/deploy/edge-assets/app')
def digest(path):
    h = hashlib.sha256()
    with path.open('rb') as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b''): h.update(chunk)
    return h.hexdigest()
def point(link, target):
    candidate = link.with_name('.' + link.name + '-pending')
    os.symlink(target, candidate)
    os.replace(candidate, link)
def get(url):
    return subprocess.check_output(['curl','--noproxy','*','-fsS','--max-time','15',url])
lock_path = '/var/lock/wenshanzhike-deploy.lock'
inherited = os.environ.get('XJCODE_DEPLOY_LOCK_FD')
lock = os.fdopen(int(inherited), 'a', closefd=False) if inherited else open(lock_path, 'a')
assert os.fstat(lock.fileno()).st_ino == os.stat(lock_path).st_ino
fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
# The controller writes this marker only after the bounded upload has completed.
deadline = time.monotonic() + 1800
while not args.archive.with_suffix(args.archive.suffix + '.ready').exists():
    assert time.monotonic() < deadline, 'upload readiness timeout'
    time.sleep(2)
assert digest(args.archive) == args.sha256, 'archive checksum'
usage = shutil.disk_usage(base)
with zipfile.ZipFile(args.archive) as archive:
    manifest = json.loads(archive.read('deployment.json'))
    version = manifest['version']
    assert version == '0.1.2'
    release_id = manifest['release']
    assert release_id == '20261016-0.1.2'
    total = sum(entry.file_size for entry in archive.infolist())
    assert usage.free - total > max(usage.total * .15, 2 * 1024**3), 'disk reserve'
    package_link = base / 'xjcode'
    guide_link = base / 'xjcode-site/current'
    previous_package = os.readlink(package_link)
    previous_guide = os.readlink(guide_link)
    assert previous_package == 'xjcode-releases/20261016-0.1.0', 'unexpected package release'
    assert previous_guide == 'releases/20261016-v3', 'unexpected guide release'
    get('https://www.wenshanzhike.com/api/health')
    release = base / 'xjcode-releases' / release_id
    guide = base / 'xjcode-site/releases/20261016-v4'
    assert not release.exists() and not guide.exists(), 'immutable release already exists'
    release.mkdir(parents=True)
    for entry in archive.infolist():
        target = (release / entry.filename).resolve()
        assert release in target.parents and not entry.is_dir(), 'unsafe archive member'
        assert entry.filename in manifest['files'] or entry.filename == 'deployment.json'
    archive.extractall(release)
for name, expected in manifest['files'].items(): assert digest(release / name) == expected, name
old = (base / previous_package).resolve()
node_name = 'node-v24.22.0-win-x64.zip'
old_node = old / 'releases/0.1.0' / node_name
release_manifest = json.loads((release / 'releases/0.1.2/release-manifest.json').read_text())
assert digest(old_node) == release_manifest['node']['sha256']
os.link(old_node, release / 'releases/0.1.2' / node_name)
os.symlink('../../20261016-0.1.0/releases/0.1.0', release / 'releases/0.1.0')
guide.mkdir(parents=True)
for name in ['index.html','style.css','app.js','favicon.svg','install.ps1']:
    shutil.copyfile(release / name, guide / name)
record = {'previousPackage':previous_package,'previousGuide':previous_guide,'artifactSha256':args.sha256,'sourceRevision':manifest['sourceRevision'],'observationSeconds':60}
try:
    point(package_link, 'xjcode-releases/' + release_id)
    point(guide_link, 'releases/20261016-v4')
    for name in ['index.html','install.ps1','releases/0.1.2/release-manifest.json']:
        assert hashlib.sha256(get('https://xjcode.wenshanzhike.com/' + name)).hexdigest() == manifest['files'][name], name
    for name in ['XJCode-Setup.exe','releases/0.1.2/xjcode-0.1.2.tgz','releases/0.1.2/xjcode-windows-x64.zip']:
        with (release / name).open('rb') as f: expected = f.read(1024)
        actual = subprocess.check_output(['curl','--noproxy','*','-fsS','--range','0-1023','--max-time','15','https://xjcode.wenshanzhike.com/' + name])
        assert actual == expected, name
    get('https://xjcode.wenshanzhike.com/releases/0.1.0/release-manifest.json')
    for _ in range(6):
        for url in ['https://xjcode.wenshanzhike.com/','https://www.wenshanzhike.com/api/health']:
            get(url)
        time.sleep(10)
except Exception:
    point(guide_link, previous_guide)
    point(package_link, previous_package)
    raise
ops = base / 'xjcode-site/ops/20261016-v4'
ops.mkdir(parents=True)
(ops / 'verified.json').write_text(json.dumps(record, indent=2))
print('XJCode 0.1.2 activated; hashes, old URL compatibility and 60-second health checks passed')
