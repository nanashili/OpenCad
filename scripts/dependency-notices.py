#!/usr/bin/env python3
"""Collect dependency license declarations and upstream notice files for releases."""

import argparse
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target', required=True)
    parser.add_argument('--output', type=Path, default=ROOT / 'dist/DEPENDENCY_LICENSES.txt')
    args = parser.parse_args()
    result = subprocess.run(['cargo', 'metadata', '--locked', '--format-version', '1',
                             '--features', 'desktop', '--filter-platform', args.target],
                            cwd=ROOT, check=True, capture_output=True, text=True)
    metadata = json.loads(result.stdout)
    resolved = {node['id'] for node in metadata['resolve']['nodes']}
    sections = ['OpenCad binary dependency notices',
                'Exact dependency versions: Cargo.lock in the source repository.',
                'Each dependency retains its upstream license(s).\n']
    count = 0
    for package in sorted(metadata['packages'], key=lambda p: (p['name'], p['version'])):
        if package['id'] not in resolved or package['source'] is None:
            continue
        root = Path(package['manifest_path']).parent
        declared = package.get('license') or 'See upstream license file'
        sections.append(f"\n{'=' * 72}\n{package['name']} {package['version']}\nLicense: {declared}\nRepository: {package.get('repository') or ''}\n")
        paths = set()
        # Font and vendored subdirectories can have separate attribution files.
        for path in root.rglob('*'):
            name = path.name.upper()
            if path.is_file() and any(name.startswith(prefix) for prefix in ['LICENSE', 'LICENCE', 'COPYING', 'NOTICE', 'OFL', 'COPYRIGHT']):
                paths.add(path)
        if package.get('license_file'):
            paths.add(root / package['license_file'])
        for path in sorted(paths):
            sections.append(f'--- {path.relative_to(root).as_posix()} ---\n')
            sections.append(path.read_text(encoding='utf-8', errors='replace'))
        if not paths:
            sections.append('No standalone notice file included in this crate package.\n')
        count += 1
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text('\n'.join(sections), encoding='utf-8')
    print(f'Collected notices for {count} resolved dependency packages in {args.output}')


if __name__ == '__main__':
    main()
