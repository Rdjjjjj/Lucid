#!/usr/bin/env python3
"""Read an extracted Qingjian app without executing it; emit local static observations."""
import argparse
import hashlib
import json
import math
import plistlib
import struct
from pathlib import Path


def sha256(path):
    digest = hashlib.sha256()
    with path.open('rb') as source:
        for block in iter(lambda: source.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()


def inspect_container(path, relative):
    data = path.read_bytes()
    if len(data) < 32:
        raise ValueError(f'{relative}: truncated header')
    magic, version, kind, count = struct.unpack_from('<8sHHI', data)
    if magic != b'QINGJIAN' or version != 1 or 32 + count * 24 > len(data):
        raise ValueError(f'{relative}: unexpected container header')
    sections = {}
    for index in range(count):
        tag, reserved, offset, size = struct.unpack_from('<4sIQQ', data, 32 + index * 24)
        tag = tag.decode('ascii')
        if (reserved or offset % 8 or offset < 32 + count * 24
                or offset + size > len(data) or tag in sections):
            raise ValueError(f'{relative}: invalid section {tag}')
        sections[tag] = {'offset': offset, 'bytes': size}
    ranges = sorted((s['offset'], s['offset'] + s['bytes']) for s in sections.values())
    if any(left[1] > right[0] for left, right in zip(ranges, ranges[1:])):
        raise ValueError(f'{relative}: overlapping sections')

    def payload(tag):
        section = sections[tag]
        return data[section['offset']:section['offset'] + section['bytes']]

    result = {'path': str(relative), 'bytes': len(data), 'sha256': sha256(path),
              'format_version': version, 'kind': kind, 'sections': sections}
    if 'META' in sections:
        # Preserve the file's declarations as text, without authenticating them.
        result['declared_metadata_toml'] = payload('META').decode('utf-8')
    if 'CONF' in sections:
        result['model_config'] = json.loads(payload('CONF'))
    if 'SAFT' in sections:
        weights = payload('SAFT')
        if len(weights) < 8:
            raise ValueError(f'{relative}: truncated safetensors header')
        header_length, = struct.unpack_from('<Q', weights)
        if header_length > len(weights) - 8:
            raise ValueError(f'{relative}: safetensors header out of bounds')
        header = json.loads(weights[8:8 + header_length])
        tensors = {key: value for key, value in header.items() if key != '__metadata__'}
        widths = {'F16': 2, 'F32': 4, 'BF16': 2}
        for tensor in tensors.values():
            start, end = tensor['data_offsets']
            elements = math.prod(tensor['shape'])
            if (not 0 <= start <= end <= len(weights) - 8 - header_length
                    or tensor['dtype'] not in widths
                    or end - start != elements * widths[tensor['dtype']]):
                raise ValueError(f'{relative}: invalid tensor range or shape')
        result['safetensors_observations'] = {
            'tensor_count': len(tensors),
            'stored_parameter_elements': sum(math.prod(t['shape']) for t in tensors.values()),
            'dtypes': sorted({t['dtype'] for t in tensors.values()}),
        }
    return result


def inspect(app):
    contents = app / 'Contents'
    plist = plistlib.loads((contents / 'Info.plist').read_bytes())
    executable = contents / 'MacOS' / plist['CFBundleExecutable']
    binary = executable.read_bytes()
    inventory = [{'path': str(p.relative_to(app)), 'bytes': p.stat().st_size,
                  'sha256': sha256(p)} for p in sorted(app.rglob('*')) if p.is_file()]
    selectors = ['QingjianInputController', 'handleEvent:client:', 'inputText:client:',
                 'inputText:key:modifiers:client:', 'activateServer:', 'deactivateServer:',
                 'commitComposition:', 'setMarkedText:selectionRange:replacementRange:',
                 'insertText:replacementRange:', 'IsSecureEventInputEnabled',
                 'objc_allocateClassPair', 'objc_registerClassPair']
    offsets = {}
    for text in selectors:
        needle = text.encode('ascii')
        found, start = [], 0
        while True:
            position = binary.find(needle, start)
            if position < 0:
                break
            found.append(hex(position))
            start = position + len(needle)
        offsets[text] = found
    resources = contents / 'Resources'
    containers = [inspect_container(p, p.relative_to(resources))
                  for p in sorted(resources.rglob('*')) if p.suffix in ('.qj', '.qjm')]
    return {'method': 'Independent Python file parsing; not REA Evidence or runtime observation',
            'binary_string_scope': 'Exact UTF-8 byte searches in the entire selected executable; '
                                   'presence does not establish a method implementation or execution',
            'bundle_info': plist,
            'executable': {'bytes': len(binary), 'sha256': sha256(executable)},
            'total_regular_file_bytes': sum(entry['bytes'] for entry in inventory),
            'files': inventory, 'binary_string_offsets': offsets, 'containers': containers}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('app', type=Path)
    args = parser.parse_args()
    print(json.dumps(inspect(args.app.resolve()), ensure_ascii=False, indent=2))
