#!/usr/bin/env python3
"""List reviewed Xbox exports and direct title imports that still need verification.

The optional provider comparison reads its PE export table without loading code.
An exported name or ordinal shows linkability only; it does not prove behavior.
"""
import argparse
import json
from pathlib import Path
import sys

import pefile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from project_tuxbox.inspect import export_table


def gap_rows(status, inventory, name, provider=None):
    """Compare occupied reference slots, preserving aliases and import spellings."""
    key = name.casefold()
    reviewed = status['libraries'] + status.get('other_reviewed_libraries', [])
    library = next((row for row in reviewed if row['name'].casefold() == key), None)
    image = next((row for row in inventory['libraries'] if row['name'].casefold() == key), None)
    if image is None:
        raise ValueError(f'{name}: missing reference inventory')
    coverage = library['export_coverage'] if library else None
    entries = image['exports']['entries']
    if not image.get('sha256') or (coverage and coverage.get('binary_sha256') != image['sha256']):
        raise ValueError(f'{name}: reference hashes differ; rescan before comparing')
    if coverage and coverage['total'] != len(entries):
        raise ValueError(f'{name}: reference export count differs')
    by_spelling = {}
    for entry in entries:
        for spelling in [f"#{entry['ordinal']}", *entry['names']]:
            if spelling in by_spelling:
                raise ValueError(f'{name}: duplicate reference export {spelling}')
            by_spelling[spelling] = entry['ordinal']
    verified = set()
    for spelling in (coverage['verified'] if coverage else []):
        if spelling not in by_spelling:
            raise ValueError(f'{name}: unknown verified export {spelling}')
        ordinal = by_spelling[spelling]
        if ordinal in verified:
            raise ValueError(f'{name}: duplicate verified export ordinal {ordinal}')
        verified.add(ordinal)
    imported = {}
    unresolved = []
    for imp in inventory['title']['imports']:
        if imp['module'].casefold() != key:
            continue
        for spelling in imp['symbols']:
            ordinal = by_spelling.get(spelling)
            if ordinal is None:
                unresolved.append(spelling)
            else:
                imported.setdefault(ordinal, set()).add(f"{imp['table']}:{spelling}")
    provider_slots = export_table(provider)[0] if provider is not None else None
    provider_names = {n for aliases in provider_slots.values() for n in aliases} if provider_slots else set()
    rows = []
    for entry in entries:
        ordinal, aliases = entry['ordinal'], entry['names']
        row = {'ordinal': ordinal, 'names': aliases, 'kind': entry['kind'],
               'verified': ordinal in verified, 'imports': sorted(imported.get(ordinal, set()))}
        if provider_slots is not None:
            actual = provider_slots.get(ordinal)
            row['provider_name'] = any(alias in provider_names for alias in aliases)
            row['provider_ordinal'] = actual is not None and (not aliases or bool(set(aliases) & actual))
        rows.append(row)
    return {'library': image['name'], 'reviewed': library is not None, 'reference_sha256': image['sha256'],
            'provider': str(provider) if provider is not None else None,
            'unresolved_imports': sorted(set(unresolved)), 'exports': rows}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('libraries', nargs='+', help='catalogued DLL names, e.g. xmem.dll')
    parser.add_argument('--provider', type=Path, help='compare one built provider PE export table')
    parser.add_argument('--all', action='store_true', help='include already verified exports')
    parser.add_argument('--json', action='store_true', help='write machine-readable output')
    args = parser.parse_args()
    if args.provider and len(args.libraries) != 1:
        parser.error('--provider requires exactly one library')
    try:
        status = json.loads((ROOT / 'evidence/library-status.json').read_text())
        inventory = json.loads((ROOT / 'evidence/frosty-xbox-library-inventory.json').read_text())
        reports = [gap_rows(status, inventory, n, args.provider) for n in args.libraries]
    except (ValueError, OSError, KeyError, pefile.PEFormatError) as exc:
        parser.exit(1, f'{exc}\n')
    if args.json:
        print(json.dumps(reports, indent=2))
        return
    for report in reports:
        print(f"{report['library']} · reference {report['reference_sha256'][:12]}")
        if not report['reviewed']:
            print('Package inventory only. This library has no current provider review.')
        if report['provider']:
            print('Provider exports show linkability only; behavior still needs verification.')
        for row in report['exports']:
            if row['verified'] and not args.all:
                continue
            label = ', '.join(row['names']) or '(ordinal only)'
            state = 'verified' if row['verified'] else 'unverified'
            imported = f"  Frosty: {', '.join(row['imports'])}" if row['imports'] else ''
            linked = ''
            if report['provider']:
                linked = f"  provider name={'yes' if row['provider_name'] else 'no'}, ordinal={'yes' if row['provider_ordinal'] else 'no'}"
            print(f"  #{row['ordinal']:<3} {label}  [{row['kind']}, {state}]{imported}{linked}")
        if report['unresolved_imports']:
            print('  Imports absent from reference inventory: ' + ', '.join(report['unresolved_imports']))


if __name__ == '__main__':
    main()
