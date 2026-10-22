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
    assert version == '0.1.5'
    release_id = manifest['release']
    assert release_id == '20261022-0.1.5'
    total = sum(entry.file_size for entry in archive.infolist())
    assert usage.free - total > max(usage.total * .15, 2 * 1024**3), 'disk reserve'
    package_link = base / 'xjcode'
    guide_link = base / 'xjcode-site/current'
    previous_package = os.readlink(package_link)
    previous_guide = os.readlink(guide_link)
    assert previous_package == 'xjcode-releases/20261022-0.1.4', 'unexpected package release'
    assert previous_guide == 'releases/20261022-powershell-v1', 'unexpected guide release'
    get('https://www.wenshanzhike.com/api/health')
    assert int(next(line.split()[1] for line in pathlib.Path('/proc/meminfo').read_text().splitlines() if line.startswith('MemAvailable:'))) > 256000
    assert float(pathlib.Path('/proc/pressure/io').read_text().splitlines()[1].split()[1].split('=')[1]) < 5
    caddy_config = pathlib.Path('/opt/wenshanzhike/deploy/Caddyfile.prod')
    config_hash = digest(caddy_config)
    state_args = ['docker','inspect','wenshanzhike-backend-1','wenshanzhike-edge-1','wenshanzhike-postgres-1','--format','{{.Name}} {{.Image}} {{.RestartCount}} {{.State.OOMKilled}}']
    state_before = subprocess.check_output(state_args)
    release = base / 'xjcode-releases' / release_id
    guide = base / 'xjcode-site/releases/20261022-0.1.5'
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
release_manifest = json.loads((release / 'releases/0.1.5/release-manifest.json').read_text())
assert digest(old_node) == release_manifest['node']['sha256']
os.link(old_node, release / 'releases/0.1.5' / node_name)
os.symlink('../../20261016-0.1.0/releases/0.1.0', release / 'releases/0.1.0')
os.symlink('../../20261016-0.1.2/releases/0.1.2', release / 'releases/0.1.2')
os.symlink('../../20261016-0.1.3/releases/0.1.3', release / 'releases/0.1.3')
os.symlink('../../20261022-0.1.4/releases/0.1.4', release / 'releases/0.1.4')
guide.mkdir(parents=True)
for name in ['index.html','style.css','app.js','favicon.svg','install.ps1','latest.json','SHA256SUMS.txt']:
    shutil.copyfile(release / name, guide / name)
record = {'previousPackage':previous_package,'previousGuide':previous_guide,'artifactSha256':args.sha256,'sourceRevision':manifest['sourceRevision'],'observationSeconds':60}
try:
    point(package_link, 'xjcode-releases/' + release_id)
    point(guide_link, 'releases/20261022-0.1.5')
    for name in ['index.html','install.ps1','latest.json','releases/0.1.5/release-manifest.json']:
        assert hashlib.sha256(get('https://xjcode.wenshanzhike.com/' + name)).hexdigest() == manifest['files'][name], name
    for name in ['XJCode-Setup.exe','releases/0.1.5/xjcode-0.1.5.tgz','releases/0.1.5/xjcode-windows-x64.zip']:
        with (release / name).open('rb') as f: expected = f.read(1024)
        actual = subprocess.check_output(['curl','--noproxy','*','-fsS','--range','0-1023','--max-time','15','https://xjcode.wenshanzhike.com/' + name])
        assert actual == expected, name
    for old_version in ['0.1.0','0.1.2','0.1.3','0.1.4']:
        get('https://xjcode.wenshanzhike.com/releases/'+old_version+'/release-manifest.json')
    for _ in range(6):
        for url in ['https://xjcode.wenshanzhike.com/','https://www.wenshanzhike.com/api/health']:
            get(url)
        time.sleep(10)
    assert digest(caddy_config) == config_hash, 'Caddy changed'
    assert subprocess.check_output(state_args) == state_before, 'container state changed'
except Exception:
    point(guide_link, previous_guide)
    point(package_link, previous_package)
    raise
ops = base / 'xjcode-site/ops/20261022-0.1.5'
ops.mkdir(parents=True)
(ops / 'verified.json').write_text(json.dumps(record, indent=2))
print('XJCode 0.1.5 activated; hashes, old URL compatibility and 60-second health checks passed')
