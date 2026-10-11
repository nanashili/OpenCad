#!/usr/bin/env python3
"""Package already-built native binaries; never invoke or inspect AutoCAD."""

import argparse
import hashlib
import platform
import plistlib
import re
import shutil
import subprocess
import tarfile
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {
    'aarch64-apple-darwin': ('macos-arm64', 'Darwin'),
    'x86_64-apple-darwin': ('macos-x64', 'Darwin'),
    'x86_64-pc-windows-msvc': ('windows-x64', 'Windows'),
    'x86_64-unknown-linux-gnu': ('linux-x64', 'Linux'),
}


def app_bundle(binary, destination, version):
    executable = destination / 'Contents/MacOS/opencad-ui'
    executable.parent.mkdir(parents=True)
    (destination / 'Contents/Resources').mkdir()
    shutil.copy2(binary, executable)
    executable.chmod(0o755)
    info = {
        'CFBundleExecutable': 'opencad-ui',
        'CFBundleIdentifier': 'org.opendesk.opencad',
        'CFBundleName': 'OpenCad',
        'CFBundleDisplayName': 'OpenCad',
        'CFBundlePackageType': 'APPL',
        'CFBundleShortVersionString': version.split('-')[0],
        'CFBundleVersion': version.split('-')[0],
        'LSMinimumSystemVersion': '13.0',
        'NSHighResolutionCapable': True,
        'NSHumanReadableCopyright': 'Copyright 2026 OpenCad contributors. MIT License.',
    }
    (destination / 'Contents/Info.plist').write_bytes(plistlib.dumps(info))
    subprocess.run(['codesign', '--force', '--sign', '-', str(destination)], check=True)
    subprocess.run(['codesign', '--verify', '--strict', str(destination)], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target', required=True, choices=TARGETS)
    parser.add_argument('--version', required=True)
    parser.add_argument('--binary-dir', required=True, type=Path)
    parser.add_argument('--output', type=Path, default=ROOT / 'dist')
    parser.add_argument('--app-only', action='store_true')
    args = parser.parse_args()
    version = args.version.removeprefix('v')
    if not re.fullmatch(r'\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?', version):
        parser.error('Version must be a semantic version without path characters')
    label, host = TARGETS[args.target]
    if platform.system() != host:
        parser.error(f'{args.target} must be packaged on {host}')
    args.output.mkdir(parents=True, exist_ok=True)
    suffix = '.exe' if host == 'Windows' else ''
    binaries = {name: args.binary_dir / (name + suffix) for name in ['opencad', 'opencad-ui']}
    required = ['opencad-ui'] if args.app_only else binaries
    for name in required:
        if not binaries[name].is_file():
            parser.error(f'Missing binary: {binaries[name]}')
    if args.app_only:
        if host != 'Darwin':
            parser.error('--app-only requires macOS')
        bundle = args.output / 'OpenCad.app'
        if bundle.exists():
            shutil.rmtree(bundle)
        app_bundle(binaries['opencad-ui'], bundle, version)
        print(bundle.resolve())
        return
    stem = f'OpenCad-{version}-{label}'
    with tempfile.TemporaryDirectory(prefix='opencad-release-') as temporary:
        package = Path(temporary) / stem
        package.mkdir()
        if host == 'Darwin':
            app_bundle(binaries['opencad-ui'], package / 'OpenCad.app', version)
            shutil.copy2(binaries['opencad'], package / 'opencad')
            (package / 'opencad').chmod(0o755)
        else:
            for name, binary in binaries.items():
                destination = package / (name + suffix)
                shutil.copy2(binary, destination)
                if host == 'Linux':
                    destination.chmod(0o755)
        for name in ['README.md', 'LICENSE', 'THIRD_PARTY_NOTICES.md']:
            shutil.copy2(ROOT / name, package / name)
        licenses = args.output / 'DEPENDENCY_LICENSES.txt'
        if not licenses.is_file():
            parser.error('Generate dist/DEPENDENCY_LICENSES.txt with scripts/dependency-notices.py first')
        shutil.copy2(licenses, package / licenses.name)
        shutil.copytree(ROOT / 'docs', package / 'docs')
        shutil.copytree(ROOT / 'examples', package / 'examples')
        (package / 'BUILD.txt').write_text(f'OpenCad {version}\nTarget: {args.target}\nSource: https://github.com/nanashili/OpenCad\n', encoding='utf-8')
        if host == 'Linux':
            archive = args.output / (stem + '.tar.gz')
            with tarfile.open(archive, 'w:gz') as tar:
                tar.add(package, arcname=stem)
        else:
            archive = args.output / (stem + '.zip')
            with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED) as zipped:
                for path in sorted(package.rglob('*')):
                    zipped.write(path, path.relative_to(package.parent).as_posix())
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    (args.output / (stem + '.sha256')).write_text(f'{digest}  {archive.name}\n', encoding='utf-8', newline='\n')
    print(archive.resolve())


if __name__ == '__main__':
    main()
