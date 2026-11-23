#!/usr/bin/env python3
"""Build the sandbox-side helper with an explicitly supplied static musl sysroot.

This development helper is not installed by build-deb.sh. Its libc/loader must
not depend on the host distribution or any particular Flatpak runtime.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--musl-sysroot', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--cc', default='cc')
    args = parser.parse_args()
    source = Path(__file__).resolve().with_name('omen-flatpak-game.c')
    root = args.musl_sysroot.resolve(strict=True)
    output = args.output.absolute()
    manifest = output.with_name(output.name + '.json')
    if output.exists() or output.is_symlink() or manifest.exists() or manifest.is_symlink():
        parser.error('output and build manifest must be new paths')
    compiler = shutil.which(args.cc)
    if not compiler:
        parser.error('the requested C compiler is unavailable')
    specs = list(root.glob('usr/lib/*-linux-musl/musl-gcc.specs'))
    if len(specs) != 1:
        parser.error('the sysroot must contain exactly one musl-gcc.specs')
    library = specs[0].parent
    architecture = library.name
    include = root / 'usr/include' / architecture
    copyright_file = root / 'usr/share/doc/musl/copyright'
    inputs = [source, specs[0], library / 'libc.a', library / 'rcrt1.o', library / 'Scrt1.o', library / 'crti.o', library / 'crtn.o', copyright_file]
    if not include.is_dir() or any(not path.is_file() for path in inputs):
        parser.error('incomplete musl sysroot; static libc, headers, startup files and copyright are required')
    original = specs[0].read_text()
    relocated = original.replace('/usr/include/' + architecture, str(include)).replace('/usr/lib/' + architecture, str(library))
    if relocated == original:
        parser.error('unsupported musl compiler specifications')
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.omen-flatpak-build-', dir=output.parent) as directory:
        work = Path(directory)
        local_specs = work / 'musl-gcc.specs'
        local_specs.write_text(relocated)
        binary = work / 'omen-flatpak-game'
        command = [compiler, '-specs=' + str(local_specs), '-static-pie', '-O2', '-std=c11', '-Wall', '-Wextra', '-Werror', '-fstack-protector-strong', '-D_FORTIFY_SOURCE=2', str(source), '-o', str(binary)]
        subprocess.run(command, check=True)
        program_headers = subprocess.check_output(['readelf', '-lW', binary], text=True)
        dynamic = subprocess.check_output(['readelf', '-dW', binary], text=True)
        if 'INTERP' in program_headers or '(NEEDED)' in dynamic or not any('GNU_STACK' in line and ' RW ' in line and 'RWE' not in line for line in program_headers.splitlines()):
            raise RuntimeError('the wrapper must be static with a non-executable stack')
        report = {
            'source_sha256': digest(source), 'binary_sha256': digest(binary),
            'binary_size': binary.stat().st_size, 'static_pie': True,
            'compiler': compiler, 'compiler_version': subprocess.check_output([compiler, '--version'], text=True).splitlines()[0],
            'command': command, 'musl_sysroot': str(root),
            'inputs': {str(path): digest(path) for path in inputs},
            'musl_headers': {str(path.relative_to(root)): digest(path) for path in sorted(include.rglob('*')) if path.is_file()},
            'copyright_notice': str(copyright_file),
            'installed_or_packaged': False,
        }
        # Hard-link publication refuses a raced existing destination as well.
        os.link(binary, output)
        try:
            with manifest.open('x') as stream:
                json.dump(report, stream, indent=2)
                stream.write('\n')
        except BaseException:
            output.unlink()
            raise
    print(json.dumps({'binary': str(output), 'sha256': report['binary_sha256'], 'size': report['binary_size']}))


if __name__ == '__main__':
    main()
